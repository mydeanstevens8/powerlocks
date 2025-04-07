mod api;
pub use api::*;

use crate::primitives::{
    CoreThreadEnv, LockResult, PoisonError, ShouldBlock, ThreadEnv, TryLockError, TryLockResult,
};
use core::{
    cell::UnsafeCell,
    marker::PhantomData,
    ops::{Deref, DerefMut},
    panic::{RefUnwindSafe, UnwindSafe},
    sync::atomic::{AtomicBool, Ordering},
};

#[derive(Debug)]
#[must_use = "if unused the `BaseMutex` will immediately unlock"]
pub struct BaseMutexGuard<'a, T, Hook, Env>
where
    T: ?Sized,
    Hook: MutexHook,
    Env: ThreadEnv,
{
    lock: &'a BaseMutex<T, Hook, Env>,
    // It may seem as if we could get away with `&mut`, but no! While we are `drop`ping this guard,
    // `data` may still be live and some other thread could immediately lock the mutex while we are
    // dropping this guard (since we are releasing the lock during `drop`) and then create another
    // live `&mut`, which is undefined behavior due to it being a `noalias` violation. So use a raw
    // `*mut` to prevent references etc. living during the `drop` call after the release.
    data: *mut T,
}

impl<'a, T, Hook, Env> BaseMutexGuard<'a, T, Hook, Env>
where
    T: ?Sized,
    Hook: MutexHook,
    Env: ThreadEnv,
{
    unsafe fn new(lock: &'a BaseMutex<T, Hook, Env>) -> Self {
        Self {
            lock,
            data: lock.data.get(),
        }
    }
}

impl<T, Hook, Env> Drop for BaseMutexGuard<'_, T, Hook, Env>
where
    T: ?Sized,
    Hook: MutexHook,
    Env: ThreadEnv,
{
    fn drop(&mut self) {
        // SAFETY: We're dropping, so we won't use `data` again.
        unsafe {
            self.lock.unlock(Env::panicking());
        };

        self.lock.hook.after_lock();
    }
}

impl<T, Hook, Env> Deref for BaseMutexGuard<'_, T, Hook, Env>
where
    T: ?Sized,
    Hook: MutexHook,
    Env: ThreadEnv,
{
    type Target = T;
    fn deref(&self) -> &Self::Target {
        // SAFETY: `data` is aligned and is guaranteed to point to valid memory via
        // `UnsafeCell::get`. Caller of `new` must guarantee that we have no writing access.
        unsafe { &*self.data }
    }
}

impl<T, Hook, Env> DerefMut for BaseMutexGuard<'_, T, Hook, Env>
where
    T: ?Sized,
    Hook: MutexHook,
    Env: ThreadEnv,
{
    fn deref_mut(&mut self) -> &mut Self::Target {
        // SAFETY: `data` is aligned and is guaranteed to point to valid memory via
        // `UnsafeCell::get`. Caller of `new` must guarantee that we have exclusive access.
        unsafe { &mut *self.data }
    }
}

// SAFETY: Unlike `MutexGuard`, we are `Send`. The primary reason why `MutexGuard` is not `Send` is
// because it uses the C `pthread_mutex_unlock` call that requires locks to be released on the same
// thread that called `pthread_mutex_lock`. Unlike `MutexGuard` though, it is safe to release our
// `BaseMutexGuard` on another thread, as we don't depend on the `pthread` library.
// Furthermore, we only care about if we are locked, not which thread has locked us.
unsafe impl<T, Hook, Env> Send for BaseMutexGuard<'_, T, Hook, Env>
where
    T: ?Sized + Send,
    Hook: MutexHook,
    Env: ThreadEnv,
{
}
unsafe impl<T, Hook, Env> Sync for BaseMutexGuard<'_, T, Hook, Env>
where
    T: ?Sized + Sync,
    Hook: MutexHook,
    Env: ThreadEnv,
{
}

#[derive(Debug)]
pub struct BaseMutex<T, Hook, Env>
where
    T: ?Sized,
    Hook: MutexHook,
    Env: ThreadEnv,
{
    lock: AtomicBool,
    poison: AtomicBool,
    hook: Hook,
    thread_env: PhantomData<Env>,
    data: UnsafeCell<T>,
}

fn wrap_lock_result<T>(poisoned: bool, t: T) -> LockResult<T> {
    if poisoned {
        Err(PoisonError::new(t))
    } else {
        Ok(t)
    }
}

impl<T, Env> BaseMutex<T, (), Env>
where
    T: Sized,
    Env: ThreadEnv,
{
    pub const fn new_unhooked(data: T) -> Self {
        Self {
            lock: AtomicBool::new(false),
            poison: AtomicBool::new(false),
            hook: (),
            thread_env: PhantomData,
            data: UnsafeCell::new(data),
        }
    }
}

impl<T, Hook, Env> BaseMutex<T, Hook, Env>
where
    T: ?Sized,
    Hook: MutexHook,
    Env: ThreadEnv,
{
    pub fn new(data: T) -> Self
    where
        Self: Sized,
        T: Sized,
    {
        Self {
            lock: AtomicBool::new(false),
            poison: AtomicBool::new(false),
            hook: Hook::new(),
            thread_env: PhantomData,
            data: UnsafeCell::new(data),
        }
    }

    pub fn into_inner(self) -> LockResult<T>
    where
        Self: Sized,
        T: Sized,
    {
        wrap_lock_result(self.is_poisoned(), self.data.into_inner())
    }

    pub fn get_mut(&mut self) -> LockResult<&mut T> {
        wrap_lock_result(self.is_poisoned(), self.data.get_mut())
    }

    pub fn is_poisoned(&self) -> bool {
        self.poison.load(Ordering::Acquire)
    }

    pub fn clear_poison(&self) {
        self.poison.store(false, Ordering::Release);
    }

    unsafe fn unlock(&self, poison: bool) {
        self.lock.store(false, Ordering::Release);
        self.poison.fetch_or(poison, Ordering::Release);
    }

    unsafe fn do_lock(&self) -> LockResult<BaseMutexGuard<T, Hook, Env>> {
        // SAFETY: Caller promises that we have the exclusive lock.
        let guard = unsafe { BaseMutexGuard::new(self) };
        if self.is_poisoned() {
            Err(PoisonError::new(guard))
        } else {
            Ok(guard)
        }
    }

    fn try_acquire_locker(&self, strong: bool) -> bool {
        let compare_result = if strong {
            self.lock
                .compare_exchange(false, true, Ordering::AcqRel, Ordering::Acquire)
        } else {
            self.lock
                .compare_exchange_weak(false, true, Ordering::AcqRel, Ordering::Acquire)
        };

        compare_result.is_ok()
    }

    pub fn lock(&self) -> LockResult<BaseMutexGuard<T, Hook, Env>> {
        while let ShouldBlock::Block = self.hook.try_lock() {}

        const STRONG_ATTEMPT_DIVIDER: usize = 32;
        let mut attempts = 0_usize;

        // Try a strong acquire once in a while to prevent being stuck on spurious failures.
        // Otherwise, stay weak in order to conserve efficiency. Guarantee though that the first
        // acquire is strong.
        while !self.try_acquire_locker(attempts % STRONG_ATTEMPT_DIVIDER == 0) {
            Env::yield_now();
            attempts = attempts.wrapping_add(1);
        }
        // SAFETY: Repeating `try_acquire_locker` until success guarantees us exclusive access.
        unsafe { self.do_lock() }
    }

    pub fn try_lock(&self) -> TryLockResult<BaseMutexGuard<T, Hook, Env>> {
        self.hook.try_lock().to_result()?;

        if self.try_acquire_locker(true) {
            // SAFETY: `try_acquire_locker`'s success guarantees us exclusive access.
            unsafe { self.do_lock() }.map_err(TryLockError::Poisoned)
        } else {
            Err(TryLockError::WouldBlock)
        }
    }
}

impl<T, Hook, Env> Default for BaseMutex<T, Hook, Env>
where
    T: Default,
    Hook: MutexHook,
    Env: ThreadEnv,
{
    fn default() -> Self {
        Self::new(T::default())
    }
}

impl<T, Hook, Env> From<T> for BaseMutex<T, Hook, Env>
where
    T: Sized,
    Hook: MutexHook,
    Env: ThreadEnv,
{
    fn from(value: T) -> Self {
        Self::new(value)
    }
}

// `T` needs to be `Send` for `BaseMutex` to be `Send`. Otherwise, that means transferring `T`
// itself across thread boundaries. Like `T` for example being a `MutexGuard`.
unsafe impl<T, Hook, Env> Send for BaseMutex<T, Hook, Env>
where
    T: ?Sized + Send,
    Hook: MutexHook,
    Env: ThreadEnv,
{
}
unsafe impl<T, Hook, Env> Sync for BaseMutex<T, Hook, Env>
where
    T: ?Sized + Send,
    Hook: MutexHook,
    Env: ThreadEnv,
{
}

impl<T, Hook, Env> UnwindSafe for BaseMutex<T, Hook, Env>
where
    T: ?Sized,
    Hook: MutexHook,
    Env: ThreadEnv,
{
}
impl<T, Hook, Env> RefUnwindSafe for BaseMutex<T, Hook, Env>
where
    T: ?Sized,
    Hook: MutexHook,
    Env: ThreadEnv,
{
}

impl<'a, T, Hook, Env> MutexGuardApi<'a, T> for BaseMutexGuard<'a, T, Hook, Env>
where
    T: 'a + ?Sized,
    Hook: MutexHook,
    Env: ThreadEnv,
{
}

impl<T, Hook, Env> MutexApi<T> for BaseMutex<T, Hook, Env>
where
    T: ?Sized,
    Hook: MutexHook,
    Env: ThreadEnv,
{
    fn try_lock<'a>(&'a self) -> TryLockResult<impl MutexGuardApi<'a, T>>
    where
        T: 'a,
    {
        self.try_lock()
    }

    fn lock<'a>(&'a self) -> LockResult<impl MutexGuardApi<'a, T>>
    where
        T: 'a,
    {
        self.lock()
    }

    fn is_poisoned(&self) -> bool {
        self.is_poisoned()
    }

    fn clear_poison(&self) {
        self.clear_poison();
    }

    fn get_mut(&mut self) -> LockResult<&mut T> {
        self.get_mut()
    }

    fn new(t: T) -> Self
    where
        Self: Sized,
        T: Sized,
    {
        Self::new(t)
    }

    fn into_inner(self) -> LockResult<T>
    where
        Self: Sized,
        T: Sized,
    {
        self.into_inner()
    }
}

pub type CoreMutex<T> = BaseMutex<T, (), CoreThreadEnv>;
pub type CoreMutexGuard<'a, T> = BaseMutexGuard<'a, T, (), CoreThreadEnv>;

#[cfg(feature = "std")]
mod std_types {
    use super::{BaseMutex, BaseMutexGuard};
    use crate::primitives::StdThreadEnv;

    pub type StdMutex<T> = BaseMutex<T, (), StdThreadEnv>;
    pub type StdMutexGuard<'a, T> = BaseMutexGuard<'a, T, (), StdThreadEnv>;
}

#[cfg(feature = "std")]
pub use std_types::*;

#[cfg(not(feature = "std"))]
mod types {
    use super::{CoreMutex, CoreMutexGuard};
    pub type Mutex<T> = CoreMutex<T>;
    pub type MutexGuard<'a, T> = CoreMutexGuard<'a, T>;
}

#[cfg(feature = "std")]
mod types {
    use super::{StdMutex, StdMutexGuard};
    pub type Mutex<T> = StdMutex<T>;
    pub type MutexGuard<'a, T> = StdMutexGuard<'a, T>;
}

pub use types::*;
