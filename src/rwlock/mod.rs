mod api;
pub use api::*;

use core::{
    cell::UnsafeCell,
    marker::PhantomData,
    ops::{Deref, DerefMut},
    panic::{RefUnwindSafe, UnwindSafe},
    ptr::NonNull,
    sync::atomic::{AtomicBool, Ordering},
};

use crate::{
    primitives::{CoreHandle, Handle, LockResult, PoisonError, TryLockError, TryLockResult},
    strategied_rwlock::{RwLockApi, RwLockReadGuardApi, RwLockWriteGuardApi},
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
struct BaseRwLockInner<K: RwLockHook, H: Handle> {
    mutex: AtomicBool,
    state: UnsafeCell<State>,
    poison: AtomicBool,
    hook: K,
    handle_type: PhantomData<H>,
}

impl<H: Handle> BaseRwLockInner<(), H> {
    const fn new_unhooked() -> Self {
        Self {
            mutex: AtomicBool::new(false),
            state: UnsafeCell::new(State::new()),
            poison: AtomicBool::new(false),
            hook: (),
            handle_type: PhantomData,
        }
    }
}

impl<K: RwLockHook, H: Handle> BaseRwLockInner<K, H> {
    fn new() -> Self {
        Self {
            mutex: AtomicBool::new(false),
            state: UnsafeCell::new(State::new()),
            poison: AtomicBool::new(false),
            hook: K::new(),
            handle_type: PhantomData,
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
            H::dumb().yield_now();
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
unsafe impl<K: RwLockHook, H: Handle> Sync for BaseRwLockInner<K, H> {}

impl<K: RwLockHook, H: Handle> UnwindSafe for BaseRwLockInner<K, H> {}
impl<K: RwLockHook, H: Handle> RefUnwindSafe for BaseRwLockInner<K, H> {}

#[derive(Debug)]
pub struct BaseRwLock<T, K, H>
where
    T: ?Sized,
    K: RwLockHook,
    H: Handle,
{
    inner: BaseRwLockInner<K, H>,
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

impl<T, H> BaseRwLock<T, (), H>
where
    T: Sized,
    H: Handle,
{
    pub const fn new_unhooked(t: T) -> Self {
        Self {
            inner: BaseRwLockInner::new_unhooked(),
            data: UnsafeCell::new(t),
        }
    }
}

impl<T, K, H> BaseRwLock<T, K, H>
where
    T: ?Sized,
    K: RwLockHook,
    H: Handle,
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

    pub fn try_read(&self) -> TryLockResult<BaseRwLockReadGuard<'_, T, K, H>> {
        self.inner.hook.try_read().to_result()?;

        // SAFETY: The lock is acquired before guard creation by `try_lock`.
        map_ok_and_poisoned(self.inner.try_lock(Method::Read), |_| unsafe {
            BaseRwLockReadGuard::new(self)
        })
    }

    pub fn read(&self) -> LockResult<BaseRwLockReadGuard<'_, T, K, H>> {
        block_try_lock(|| self.try_read())
    }

    pub fn try_write(&self) -> TryLockResult<BaseRwLockWriteGuard<'_, T, K, H>> {
        self.inner.hook.try_write().to_result()?;

        // SAFETY: The lock is acquired before guard creation by `try_lock`.
        map_ok_and_poisoned(self.inner.try_lock(Method::Write), |_| unsafe {
            BaseRwLockWriteGuard::new(self)
        })
    }

    pub fn write(&self) -> LockResult<BaseRwLockWriteGuard<'_, T, K, H>> {
        block_try_lock(|| self.try_write())
    }
}

impl<T, K, H> RwLockApi<T> for BaseRwLock<T, K, H>
where
    T: ?Sized,
    K: RwLockHook,
    H: Handle,
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

unsafe impl<T, K, H> Send for BaseRwLock<T, K, H>
where
    T: ?Sized + Send,
    K: RwLockHook,
    H: Handle,
{
}
unsafe impl<T, K, H> Sync for BaseRwLock<T, K, H>
where
    T: ?Sized + Send + Sync,
    K: RwLockHook,
    H: Handle,
{
}

impl<T, K, H> UnwindSafe for BaseRwLock<T, K, H>
where
    T: ?Sized,
    K: RwLockHook,
    H: Handle,
{
}
impl<T, K, H> RefUnwindSafe for BaseRwLock<T, K, H>
where
    T: ?Sized,
    K: RwLockHook,
    H: Handle,
{
}

impl<T, K, H> Default for BaseRwLock<T, K, H>
where
    T: Default,
    K: RwLockHook,
    H: Handle,
{
    fn default() -> Self {
        Self::new(T::default())
    }
}

impl<T, K, H> From<T> for BaseRwLock<T, K, H>
where
    K: RwLockHook,
    H: Handle,
{
    fn from(value: T) -> Self {
        Self::new(value)
    }
}

#[derive(Debug)]
#[must_use = "if unused the read-write-lock will immediately unlock"]
pub struct BaseRwLockReadGuard<'a, T, K, H>
where
    T: 'a + ?Sized,
    K: RwLockHook,
    H: Handle,
{
    inner: &'a BaseRwLockInner<K, H>,
    // Use a raw pointer instead of a reference to prevent aliasing violations during `drop` when
    // the lock is released and then acquired by another thread before `drop` completes.
    data: NonNull<T>,
}

impl<'a, T, K, H> BaseRwLockReadGuard<'a, T, K, H>
where
    T: ?Sized,
    K: RwLockHook,
    H: Handle,
{
    unsafe fn new(lock: &'a BaseRwLock<T, K, H>) -> Self {
        Self {
            inner: &lock.inner,
            // SAFETY: `UnsafeCell::get` never returns a null pointer.
            data: unsafe { NonNull::new_unchecked(lock.data.get()) },
        }
    }
}

impl<T, K, H> Deref for BaseRwLockReadGuard<'_, T, K, H>
where
    T: ?Sized,
    K: RwLockHook,
    H: Handle,
{
    type Target = T;
    fn deref(&self) -> &Self::Target {
        unsafe { self.data.as_ref() }
    }
}

impl<T, K, H> Drop for BaseRwLockReadGuard<'_, T, K, H>
where
    T: ?Sized,
    K: RwLockHook,
    H: Handle,
{
    fn drop(&mut self) {
        unsafe { self.inner.unlock(Method::Read, false) };
        self.inner.hook.after_read();
    }
}

unsafe impl<T, K, H> Send for BaseRwLockReadGuard<'_, T, K, H>
where
    T: ?Sized + Send,
    K: RwLockHook,
    H: Handle,
{
}
unsafe impl<T, K, H> Sync for BaseRwLockReadGuard<'_, T, K, H>
where
    T: ?Sized + Sync,
    K: RwLockHook,
    H: Handle,
{
}

impl<'a, T, K, H> RwLockReadGuardApi<'a, T> for BaseRwLockReadGuard<'a, T, K, H>
where
    T: 'a + ?Sized,
    K: RwLockHook,
    H: Handle,
{
}

#[derive(Debug)]
#[must_use = "if unused the read-write-lock will immediately unlock"]
pub struct BaseRwLockWriteGuard<'a, T, K, H>
where
    T: 'a + ?Sized,
    K: RwLockHook,
    H: Handle,
{
    inner: &'a BaseRwLockInner<K, H>,
    // Use a raw pointer instead of a reference to prevent aliasing violations during `drop` when
    // the lock is released and then acquired by another thread before `drop` completes.
    data: *mut T,
}

impl<'a, T, K, H> BaseRwLockWriteGuard<'a, T, K, H>
where
    T: ?Sized,
    K: RwLockHook,
    H: Handle,
{
    unsafe fn new(lock: &'a BaseRwLock<T, K, H>) -> Self {
        Self {
            inner: &lock.inner,
            data: lock.data.get(),
        }
    }
}

impl<T, K, H> Deref for BaseRwLockWriteGuard<'_, T, K, H>
where
    T: ?Sized,
    K: RwLockHook,
    H: Handle,
{
    type Target = T;
    fn deref(&self) -> &Self::Target {
        unsafe { &*self.data }
    }
}

impl<T, K, H> DerefMut for BaseRwLockWriteGuard<'_, T, K, H>
where
    T: ?Sized,
    K: RwLockHook,
    H: Handle,
{
    fn deref_mut(&mut self) -> &mut Self::Target {
        unsafe { &mut *self.data }
    }
}

impl<T, K, H> Drop for BaseRwLockWriteGuard<'_, T, K, H>
where
    T: ?Sized,
    K: RwLockHook,
    H: Handle,
{
    fn drop(&mut self) {
        unsafe { self.inner.unlock(Method::Write, H::dumb().panicking()) };
        self.inner.hook.after_write();
    }
}

unsafe impl<T, K, H> Send for BaseRwLockWriteGuard<'_, T, K, H>
where
    T: ?Sized + Send,
    K: RwLockHook,
    H: Handle,
{
}
unsafe impl<T, K, H> Sync for BaseRwLockWriteGuard<'_, T, K, H>
where
    T: ?Sized + Sync,
    K: RwLockHook,
    H: Handle,
{
}

impl<'a, T, K, H> RwLockWriteGuardApi<'a, T> for BaseRwLockWriteGuard<'a, T, K, H>
where
    T: 'a + ?Sized,
    K: RwLockHook,
    H: Handle,
{
}

pub type CoreRwLock<T> = BaseRwLock<T, (), CoreHandle>;
pub type CoreRwLockReadGuard<'a, T> = BaseRwLockReadGuard<'a, T, (), CoreHandle>;
pub type CoreRwLockWriteGuard<'a, T> = BaseRwLockWriteGuard<'a, T, (), CoreHandle>;

#[cfg(feature = "std")]
mod std_types {
    use crate::primitives::StdHandle;

    use super::{BaseRwLock, BaseRwLockReadGuard, BaseRwLockWriteGuard};

    pub type StdRwLock<T> = BaseRwLock<T, (), StdHandle>;
    pub type StdRwLockReadGuard<'a, T> = BaseRwLockReadGuard<'a, T, (), StdHandle>;
    pub type StdRwLockWriteGuard<'a, T> = BaseRwLockWriteGuard<'a, T, (), StdHandle>;
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
