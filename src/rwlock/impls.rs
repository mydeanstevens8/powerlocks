use super::api::{RwLockApi, RwLockHook, RwLockReadGuardApi, RwLockWriteGuardApi};
use core::{
    cell::UnsafeCell,
    marker::PhantomData,
    ops::{Deref, DerefMut},
    panic::{RefUnwindSafe, UnwindSafe},
    ptr::NonNull,
    sync::atomic::{AtomicBool, Ordering},
};

use crate::primitives::{
    CoreThreadEnv, LockResult, PoisonError, ThreadEnv, TryLockError, TryLockResult,
};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
enum Method {
    Read,
    Write,
}

impl Method {
    #[inline]
    fn switch<T>(&self, read: impl FnOnce() -> T, write: impl FnOnce() -> T) -> T {
        match self {
            Method::Read => read(),
            Method::Write => write(),
        }
    }
}

#[repr(transparent)]
#[derive(Debug, PartialEq, Eq, Hash, Clone, Copy, Default)]
struct State(usize);

impl State {
    const fn new() -> Self {
        Self(usize::MIN)
    }

    fn alloc(&mut self, method: Method) -> bool {
        let available = method.switch(|| self.0 < usize::MAX - 1, || self.0 == usize::MIN);
        if available {
            self.0 = method.switch(|| self.0 + 1, || usize::MAX);
        }
        available
    }

    fn free(&mut self, method: Method) {
        method.switch(
            || assert!(usize::MIN < self.0 && self.0 < usize::MAX),
            || assert_eq!(self.0, usize::MAX),
        );
        self.0 = method.switch(|| self.0 - 1, || usize::MIN);
    }
}

#[derive(Debug)]
struct BaseRwLockInner<Hook: RwLockHook, Env: ThreadEnv> {
    mutex: AtomicBool,
    state: UnsafeCell<State>,
    poison: AtomicBool,
    hook: Hook,
    thread_env: PhantomData<Env>,
}

impl<Env: ThreadEnv> BaseRwLockInner<(), Env> {
    const fn new_unhooked() -> Self {
        Self {
            mutex: AtomicBool::new(false),
            state: UnsafeCell::new(State::new()),
            poison: AtomicBool::new(false),
            hook: (),
            thread_env: PhantomData,
        }
    }
}

impl<Hook: RwLockHook, Env: ThreadEnv> BaseRwLockInner<Hook, Env> {
    fn new() -> Self {
        Self {
            mutex: AtomicBool::new(false),
            state: UnsafeCell::new(State::new()),
            poison: AtomicBool::new(false),
            hook: Hook::new(),
            thread_env: PhantomData,
        }
    }

    #[inline]
    fn is_poisoned(&self) -> bool {
        self.poison.load(Ordering::Acquire)
    }

    #[inline]
    fn clear_poison(&self) {
        self.poison.store(false, Ordering::Release);
    }

    fn critical_section<T>(&self, f: impl FnOnce(&mut State) -> T) -> T {
        while self
            .mutex
            .compare_exchange(false, true, Ordering::Acquire, Ordering::Acquire)
            .is_err()
        {
            Env::yield_now();
        }
        // SAFETY: `critical_section` enforces exclusive access via `mutex`. Box the reference in a
        // nested scope to prevent theoretical lifetime escape.
        let result = { f(unsafe { &mut *self.state.get() }) };
        self.mutex.store(false, Ordering::Release);
        result
    }

    fn try_lock(&self, method: Method) -> TryLockResult<()> {
        match (
            self.critical_section(|state| state.alloc(method)),
            !self.is_poisoned(),
        ) {
            (false, _) => Err(TryLockError::WouldBlock),
            (true, false) => Err(TryLockError::Poisoned(PoisonError::new(()))),
            (true, true) => Ok(()),
        }
    }

    unsafe fn unlock(&self, method: Method, poison: bool) {
        self.critical_section(|state| state.free(method));
        self.poison.fetch_or(poison, Ordering::AcqRel);
    }
}

// SAFETY: `critical_section` enforces access to the `state` cell variable.
unsafe impl<Hook: RwLockHook, Env: ThreadEnv> Sync for BaseRwLockInner<Hook, Env> {}

impl<Hook: RwLockHook, Env: ThreadEnv> UnwindSafe for BaseRwLockInner<Hook, Env> {}
impl<Hook: RwLockHook, Env: ThreadEnv> RefUnwindSafe for BaseRwLockInner<Hook, Env> {}

#[derive(Debug)]
pub struct BaseRwLock<T, Hook, Env>
where
    T: ?Sized,
    Hook: RwLockHook,
    Env: ThreadEnv,
{
    inner: BaseRwLockInner<Hook, Env>,
    data: UnsafeCell<T>,
}

macro_rules! wrap_poison {
    ($poisoned:expr, $data:expr) => {{
        let (poisoned, data) = ($poisoned, $data);
        if poisoned {
            Err(PoisonError::new(data))
        } else {
            Ok(data)
        }
    }};
}

fn map_ok_and_poisoned<T, U>(r: TryLockResult<T>, f: impl FnOnce(T) -> U) -> TryLockResult<U> {
    match r {
        Ok(t) => Ok(f(t)),
        Err(TryLockError::Poisoned(poison_error)) => Err(TryLockError::Poisoned(PoisonError::new(
            f(poison_error.into_inner()),
        ))),
        Err(TryLockError::WouldBlock) => Err(TryLockError::WouldBlock),
    }
}

fn block_try_lock<T>(mut routine: impl FnMut() -> TryLockResult<T>) -> LockResult<T> {
    loop {
        match routine() {
            Ok(t) => break Ok(t),
            Err(TryLockError::Poisoned(poison)) => break Err(poison),
            Err(TryLockError::WouldBlock) => continue,
        }
    }
}

impl<T, Env> BaseRwLock<T, (), Env>
where
    T: Sized,
    Env: ThreadEnv,
{
    pub const fn new_unhooked(t: T) -> Self {
        Self {
            inner: BaseRwLockInner::new_unhooked(),
            data: UnsafeCell::new(t),
        }
    }
}

impl<T, Hook, Env> BaseRwLock<T, Hook, Env>
where
    T: ?Sized,
    Hook: RwLockHook,
    Env: ThreadEnv,
{
    pub fn new(t: T) -> Self
    where
        Self: Sized,
        T: Sized,
    {
        Self {
            inner: BaseRwLockInner::new(),
            data: UnsafeCell::new(t),
        }
    }

    pub fn get_mut(&mut self) -> LockResult<&mut T> {
        wrap_poison!(self.is_poisoned(), self.data.get_mut())
    }

    pub fn into_inner(self) -> LockResult<T>
    where
        Self: Sized,
        T: Sized,
    {
        wrap_poison!(self.is_poisoned(), self.data.into_inner())
    }

    #[inline]
    pub fn is_poisoned(&self) -> bool {
        self.inner.is_poisoned()
    }

    #[inline]
    pub fn clear_poison(&self) {
        self.inner.clear_poison();
    }

    pub fn try_read(&self) -> TryLockResult<BaseRwLockReadGuard<'_, T, Hook, Env>> {
        self.inner.hook.try_read().to_result()?;

        // SAFETY: The lock is acquired before guard creation by `try_lock`.
        map_ok_and_poisoned(self.inner.try_lock(Method::Read), |_| unsafe {
            BaseRwLockReadGuard::new(self)
        })
    }

    pub fn read(&self) -> LockResult<BaseRwLockReadGuard<'_, T, Hook, Env>> {
        block_try_lock(|| self.try_read())
    }

    pub fn try_write(&self) -> TryLockResult<BaseRwLockWriteGuard<'_, T, Hook, Env>> {
        self.inner.hook.try_write().to_result()?;

        // SAFETY: The lock is acquired before guard creation by `try_lock`.
        map_ok_and_poisoned(self.inner.try_lock(Method::Write), |_| unsafe {
            BaseRwLockWriteGuard::new(self)
        })
    }

    pub fn write(&self) -> LockResult<BaseRwLockWriteGuard<'_, T, Hook, Env>> {
        block_try_lock(|| self.try_write())
    }
}

impl<T, Hook, Env> RwLockApi<T> for BaseRwLock<T, Hook, Env>
where
    T: ?Sized,
    Hook: RwLockHook,
    Env: ThreadEnv,
{
    fn is_poisoned(&self) -> bool {
        self.is_poisoned()
    }

    fn clear_poison(&self) {
        self.clear_poison();
    }

    fn get_mut(&mut self) -> LockResult<&mut T> {
        self.get_mut()
    }

    fn into_inner(self) -> LockResult<T>
    where
        Self: Sized,
        T: Sized,
    {
        self.into_inner()
    }

    fn new(t: T) -> Self
    where
        Self: Sized,
        T: Sized,
    {
        Self::new(t)
    }

    fn try_read<'a>(&'a self) -> TryLockResult<impl RwLockReadGuardApi<'a, T>>
    where
        T: 'a,
    {
        self.try_read()
    }

    fn read<'a>(&'a self) -> LockResult<impl RwLockReadGuardApi<'a, T>>
    where
        T: 'a,
    {
        self.read()
    }

    fn try_write<'a>(&'a self) -> TryLockResult<impl RwLockWriteGuardApi<'a, T>>
    where
        T: 'a,
    {
        self.try_write()
    }

    fn write<'a>(&'a self) -> LockResult<impl RwLockWriteGuardApi<'a, T>>
    where
        T: 'a,
    {
        self.write()
    }
}

unsafe impl<T, Hook, Env> Send for BaseRwLock<T, Hook, Env>
where
    T: ?Sized + Send,
    Hook: RwLockHook,
    Env: ThreadEnv,
{
}
unsafe impl<T, Hook, Env> Sync for BaseRwLock<T, Hook, Env>
where
    T: ?Sized + Send + Sync,
    Hook: RwLockHook,
    Env: ThreadEnv,
{
}

impl<T, Hook, Env> UnwindSafe for BaseRwLock<T, Hook, Env>
where
    T: ?Sized,
    Hook: RwLockHook,
    Env: ThreadEnv,
{
}
impl<T, Hook, Env> RefUnwindSafe for BaseRwLock<T, Hook, Env>
where
    T: ?Sized,
    Hook: RwLockHook,
    Env: ThreadEnv,
{
}

impl<T, Hook, Env> Default for BaseRwLock<T, Hook, Env>
where
    T: Default,
    Hook: RwLockHook,
    Env: ThreadEnv,
{
    fn default() -> Self {
        Self::new(T::default())
    }
}

impl<T, Hook, Env> From<T> for BaseRwLock<T, Hook, Env>
where
    Hook: RwLockHook,
    Env: ThreadEnv,
{
    fn from(value: T) -> Self {
        Self::new(value)
    }
}

#[derive(Debug)]
#[must_use = "if unused the read-write-lock will immediately unlock"]
pub struct BaseRwLockReadGuard<'a, T, Hook, Env>
where
    T: 'a + ?Sized,
    Hook: RwLockHook,
    Env: ThreadEnv,
{
    inner: &'a BaseRwLockInner<Hook, Env>,
    // Use a raw pointer instead of a reference to prevent aliasing violations during `drop` when
    // the lock is released and then acquired by another thread before `drop` completes.
    data: NonNull<T>,
}

impl<'a, T, Hook, Env> BaseRwLockReadGuard<'a, T, Hook, Env>
where
    T: ?Sized,
    Hook: RwLockHook,
    Env: ThreadEnv,
{
    unsafe fn new(lock: &'a BaseRwLock<T, Hook, Env>) -> Self {
        Self {
            inner: &lock.inner,
            // SAFETY: `UnsafeCell::get` never returns a null pointer.
            data: unsafe { NonNull::new_unchecked(lock.data.get()) },
        }
    }
}

impl<T, Hook, Env> Deref for BaseRwLockReadGuard<'_, T, Hook, Env>
where
    T: ?Sized,
    Hook: RwLockHook,
    Env: ThreadEnv,
{
    type Target = T;
    fn deref(&self) -> &Self::Target {
        unsafe { self.data.as_ref() }
    }
}

impl<T, Hook, Env> Drop for BaseRwLockReadGuard<'_, T, Hook, Env>
where
    T: ?Sized,
    Hook: RwLockHook,
    Env: ThreadEnv,
{
    fn drop(&mut self) {
        unsafe { self.inner.unlock(Method::Read, false) };
        self.inner.hook.after_read();
    }
}

unsafe impl<T, Hook, Env> Send for BaseRwLockReadGuard<'_, T, Hook, Env>
where
    T: ?Sized + Send,
    Hook: RwLockHook,
    Env: ThreadEnv,
{
}
unsafe impl<T, Hook, Env> Sync for BaseRwLockReadGuard<'_, T, Hook, Env>
where
    T: ?Sized + Sync,
    Hook: RwLockHook,
    Env: ThreadEnv,
{
}

impl<'a, T, Hook, Env> RwLockReadGuardApi<'a, T> for BaseRwLockReadGuard<'a, T, Hook, Env>
where
    T: 'a + ?Sized,
    Hook: RwLockHook,
    Env: ThreadEnv,
{
}

#[derive(Debug)]
#[must_use = "if unused the read-write-lock will immediately unlock"]
pub struct BaseRwLockWriteGuard<'a, T, Hook, Env>
where
    T: 'a + ?Sized,
    Hook: RwLockHook,
    Env: ThreadEnv,
{
    inner: &'a BaseRwLockInner<Hook, Env>,
    // Use a raw pointer instead of a reference to prevent aliasing violations during `drop` when
    // the lock is released and then acquired by another thread before `drop` completes.
    data: *mut T,
}

impl<'a, T, Hook, Env> BaseRwLockWriteGuard<'a, T, Hook, Env>
where
    T: ?Sized,
    Hook: RwLockHook,
    Env: ThreadEnv,
{
    unsafe fn new(lock: &'a BaseRwLock<T, Hook, Env>) -> Self {
        Self {
            inner: &lock.inner,
            data: lock.data.get(),
        }
    }
}

impl<T, Hook, Env> Deref for BaseRwLockWriteGuard<'_, T, Hook, Env>
where
    T: ?Sized,
    Hook: RwLockHook,
    Env: ThreadEnv,
{
    type Target = T;
    fn deref(&self) -> &Self::Target {
        unsafe { &*self.data }
    }
}

impl<T, Hook, Env> DerefMut for BaseRwLockWriteGuard<'_, T, Hook, Env>
where
    T: ?Sized,
    Hook: RwLockHook,
    Env: ThreadEnv,
{
    fn deref_mut(&mut self) -> &mut Self::Target {
        unsafe { &mut *self.data }
    }
}

impl<T, Hook, Env> Drop for BaseRwLockWriteGuard<'_, T, Hook, Env>
where
    T: ?Sized,
    Hook: RwLockHook,
    Env: ThreadEnv,
{
    fn drop(&mut self) {
        unsafe { self.inner.unlock(Method::Write, Env::panicking()) };
        self.inner.hook.after_write();
    }
}

unsafe impl<T, Hook, Env> Send for BaseRwLockWriteGuard<'_, T, Hook, Env>
where
    T: ?Sized + Send,
    Hook: RwLockHook,
    Env: ThreadEnv,
{
}
unsafe impl<T, Hook, Env> Sync for BaseRwLockWriteGuard<'_, T, Hook, Env>
where
    T: ?Sized + Sync,
    Hook: RwLockHook,
    Env: ThreadEnv,
{
}

impl<'a, T, Hook, Env> RwLockWriteGuardApi<'a, T> for BaseRwLockWriteGuard<'a, T, Hook, Env>
where
    T: 'a + ?Sized,
    Hook: RwLockHook,
    Env: ThreadEnv,
{
}

pub type CoreRwLock<T> = BaseRwLock<T, (), CoreThreadEnv>;
pub type CoreRwLockReadGuard<'a, T> = BaseRwLockReadGuard<'a, T, (), CoreThreadEnv>;
pub type CoreRwLockWriteGuard<'a, T> = BaseRwLockWriteGuard<'a, T, (), CoreThreadEnv>;

#[cfg(feature = "std")]
mod std_types {
    use crate::primitives::StdThreadEnv;

    use super::{BaseRwLock, BaseRwLockReadGuard, BaseRwLockWriteGuard};

    pub type StdRwLock<T> = BaseRwLock<T, (), StdThreadEnv>;
    pub type StdRwLockReadGuard<'a, T> = BaseRwLockReadGuard<'a, T, (), StdThreadEnv>;
    pub type StdRwLockWriteGuard<'a, T> = BaseRwLockWriteGuard<'a, T, (), StdThreadEnv>;
}

#[cfg(feature = "std")]
pub use std_types::*;

#[cfg(not(feature = "std"))]
mod main_type {
    use super::{CoreRwLock, CoreRwLockReadGuard, CoreRwLockWriteGuard};

    pub type RwLock<T> = CoreRwLock<T>;
    pub type RwLockReadGuard<'a, T> = CoreRwLockReadGuard<'a, T>;
    pub type RwLockWriteGuard<'a, T> = CoreRwLockWriteGuard<'a, T>;
}
#[cfg(feature = "std")]
mod main_type {
    use super::{StdRwLock, StdRwLockReadGuard, StdRwLockWriteGuard};

    pub type RwLock<T> = StdRwLock<T>;
    pub type RwLockReadGuard<'a, T> = StdRwLockReadGuard<'a, T>;
    pub type RwLockWriteGuard<'a, T> = StdRwLockWriteGuard<'a, T>;
}

pub use main_type::*;
