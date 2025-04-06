mod api;
pub use api::*;

use crate::primitives::{CoreHandle, Handle, LockResult, PoisonError, TryLockError, TryLockResult};
use core::{
    cell::UnsafeCell,
    marker::PhantomData,
    ops::{Deref, DerefMut},
    panic::{RefUnwindSafe, UnwindSafe},
    sync::atomic::{AtomicBool, Ordering},
};

#[derive(Debug)]
#[must_use = "if unused the `BaseMutex` will immediately unlock"]
pub struct BaseMutexGuard<'a, T: ?Sized, H: Handle> {
    lock: &'a BaseMutex<T, H>,
    // It may seem as if we could get away with `&mut`, but no! While we are `drop`ping this guard,
    // `data` may still be live and some other thread could immediately lock the mutex while we are
    // dropping this guard (since we are releasing the lock during `drop`) and then create another
    // live `&mut`, which is undefined behavior due to it being a `noalias` violation. So use a raw
    // `*mut` to prevent references etc. living during the `drop` call after the release.
    data: *mut T,
}

impl<'a, T: ?Sized, H: Handle> BaseMutexGuard<'a, T, H> {
    unsafe fn new(lock: &'a BaseMutex<T, H>) -> Self {
        Self {
            lock,
            data: lock.data.get(),
        }
    }
}

impl<T: ?Sized, H: Handle> Drop for BaseMutexGuard<'_, T, H> {
    fn drop(&mut self) {
        // SAFETY: We're dropping, so we won't use `data` again.
        unsafe {
            self.lock.release_locker(H::dumb().panicking());
        };
    }
}

impl<T: ?Sized, H: Handle> Deref for BaseMutexGuard<'_, T, H> {
    type Target = T;
    fn deref(&self) -> &Self::Target {
        // SAFETY: `data` is aligned and is guaranteed to point to valid memory via
        // `UnsafeCell::get`. Caller of `new` must guarantee that we have no writing access.
        unsafe { &*self.data }
    }
}

impl<T: ?Sized, H: Handle> DerefMut for BaseMutexGuard<'_, T, H> {
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
unsafe impl<T: ?Sized + Send, H: Handle> Send for BaseMutexGuard<'_, T, H> {}
unsafe impl<T: ?Sized + Sync, H: Handle> Sync for BaseMutexGuard<'_, T, H> {}

#[derive(Debug)]
pub struct BaseMutex<T: ?Sized, H: Handle> {
    locker: AtomicBool,
    poison: AtomicBool,
    creator: PhantomData<H>,
    data: UnsafeCell<T>,
}

fn wrap_lock_result<T>(poisoned: bool, t: T) -> LockResult<T> {
    if poisoned {
        Err(PoisonError::new(t))
    } else {
        Ok(t)
    }
}

impl<T: Sized, H: Handle> BaseMutex<T, H> {
    pub const fn new(data: T) -> Self {
        Self {
            locker: AtomicBool::new(false),
            poison: AtomicBool::new(false),
            creator: PhantomData,
            data: UnsafeCell::new(data),
        }
    }

    pub fn into_inner(self) -> LockResult<T> {
        wrap_lock_result(self.is_poisoned(), self.data.into_inner())
    }
}

impl<T: ?Sized, H: Handle> BaseMutex<T, H> {
    pub fn get_mut(&mut self) -> LockResult<&mut T> {
        wrap_lock_result(self.is_poisoned(), self.data.get_mut())
    }

    pub fn is_poisoned(&self) -> bool {
        self.poison.load(Ordering::Acquire)
    }

    pub fn clear_poison(&self) {
        self.poison.store(false, Ordering::Release);
    }

    unsafe fn release_locker(&self, poison: bool) {
        self.locker.store(false, Ordering::Release);
        self.poison.fetch_or(poison, Ordering::Release);
    }

    unsafe fn do_lock(&self) -> LockResult<BaseMutexGuard<T, H>> {
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
            self.locker
                .compare_exchange(false, true, Ordering::AcqRel, Ordering::Acquire)
        } else {
            self.locker
                .compare_exchange_weak(false, true, Ordering::AcqRel, Ordering::Acquire)
        };

        compare_result.is_ok()
    }

    pub fn lock(&self) -> LockResult<BaseMutexGuard<T, H>> {
        const STRONG_ATTEMPT_DIVIDER: usize = 32;
        let mut attempts = 0_usize;

        // Try a strong acquire once in a while to prevent being stuck on spurious failures.
        // Otherwise, stay weak in order to conserve efficiency. Guarantee though that the first
        // acquire is strong.
        while !self.try_acquire_locker(attempts % STRONG_ATTEMPT_DIVIDER == 0) {
            H::dumb().yield_now();
            attempts = attempts.wrapping_add(1);
        }
        // SAFETY: Repeating `try_acquire_locker` until success guarantees us exclusive access.
        unsafe { self.do_lock() }
    }

    pub fn try_lock(&self) -> TryLockResult<BaseMutexGuard<T, H>> {
        if self.try_acquire_locker(true) {
            // SAFETY: `try_acquire_locker`'s success guarantees us exclusive access.
            unsafe { self.do_lock() }.map_err(TryLockError::Poisoned)
        } else {
            Err(TryLockError::WouldBlock)
        }
    }
}

impl<T: Sized + Default, H: Handle> Default for BaseMutex<T, H> {
    fn default() -> Self {
        Self::new(T::default())
    }
}

impl<T: Sized, H: Handle> From<T> for BaseMutex<T, H> {
    fn from(value: T) -> Self {
        Self::new(value)
    }
}

// `T` needs to be `Send` for `BaseMutex` to be `Send`. Otherwise, that means transferring `T`
// itself across thread boundaries. Like `T` for example being a `MutexGuard`.
unsafe impl<T: ?Sized + Send, H: Handle> Send for BaseMutex<T, H> {}
unsafe impl<T: ?Sized + Send, H: Handle> Sync for BaseMutex<T, H> {}

impl<T: ?Sized, H: Handle> UnwindSafe for BaseMutex<T, H> {}
impl<T: ?Sized, H: Handle> RefUnwindSafe for BaseMutex<T, H> {}

impl<'a, T: 'a + ?Sized, H: Handle> MutexGuardApi<'a, T> for BaseMutexGuard<'a, T, H> {}

impl<T: ?Sized, H: Handle> MutexApi<T> for BaseMutex<T, H> {
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

pub type CoreMutex<T> = BaseMutex<T, CoreHandle>;
pub type CoreMutexGuard<'a, T> = BaseMutexGuard<'a, T, CoreHandle>;

#[cfg(feature = "std")]
mod std_types {
    use super::{BaseMutex, BaseMutexGuard};
    use crate::primitives::StdHandle;

    pub type StdMutex<T> = BaseMutex<T, StdHandle>;
    pub type StdMutexGuard<'a, T> = BaseMutexGuard<'a, T, StdHandle>;
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
