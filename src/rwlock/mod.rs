pub mod strategies;

mod api;
pub use api::*;

mod impls;

use core::{
    cell::UnsafeCell,
    hash::Hash,
    marker::PhantomData,
    ops::{Deref, DerefMut},
    panic::{RefUnwindSafe, UnwindSafe},
    ptr::NonNull,
};

extern crate alloc;
use alloc::{boxed::Box, sync::Arc};

use crate::primitives::{CoreHandle, Handle, HandleId, LockResult, TryLockError, TryLockResult};

///
/// Denotes the type of operation that a Thread is performing on a [`RwLock`]. Used by
/// [`StrategyInput`] as part of the parameter to a [`Strategy`].
///
/// When operating with a `Strategy`, implementors must return a `StrategyResult`, which is a boxed
/// [`Iterator`] of `State`s. Each of the returned `State` objects corresponds to a particular
/// `Method` of a thread inside `StrategyInput`, which gets passed to `should_block`.
///
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Method {
    /// Denotes threads that are reading, or planning to read from the lock.
    Read,
    /// Denotes threads that are writing, or planning to write to the lock.
    Write,
}

impl Method {
    /// Returns `true` if this `Method` an instance of [`Method::Read`], and returns `false`
    /// otherwise.
    ///
    /// # Examples
    /// ```
    /// # use powerlocks::rwlock::Method;
    /// let method = Method::Read;
    /// assert!(method.is_read());
    ///
    /// let method = Method::Write;
    /// assert!(!method.is_read());
    /// ```
    ///
    pub fn is_read(&self) -> bool {
        *self == Method::Read
    }

    /// Returns `true` if this `Method` an instance of [`Method::Write`], and returns `false`
    /// otherwise.
    ///
    /// # Examples
    /// ```
    /// # use powerlocks::rwlock::Method;
    /// let method = Method::Write;
    /// assert!(method.is_write());
    ///
    /// let method = Method::Read;
    /// assert!(!method.is_write());
    /// ```
    ///
    pub fn is_write(&self) -> bool {
        *self == Method::Write
    }
}

///
/// Denotes whether a thread accessing a [`RwLock`] is allowed to proceed or is blocked. Used by
/// [`StrategyResult`] in the return of [`Strategy`].
///
/// When operating with a `Strategy`, implementors must return a `StrategyResult`, which is a boxed
/// [`Iterator`] of `State`s. Each of the returned `State` objects corresponds to a particular
/// `Method` of a thread inside `StrategyInput`, which gets passed to `should_block`. An
/// [`Ok`](State::Ok) value returned by `Strategy` indicates that the thread should be able to
/// access the data inside a `RwLock`, whereas a [`Blocked`](State::Blocked) value indicates that
/// the thread should remain blocked.
///
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum State {
    /// The state for a thread that is allowed to proceed.
    Ok,
    /// The state for a thread that is blocked.
    Blocked,
}

impl State {
    /// Returns `true` if this `State` an instance of [`State::Ok`], and returns `false`
    /// otherwise.
    ///
    /// # Examples
    /// ```
    /// # use powerlocks::rwlock::State;
    /// let state = State::Ok;
    /// assert!(state.is_ok());
    ///
    /// let state = State::Blocked;
    /// assert!(!state.is_ok());
    /// ```
    ///
    pub fn is_ok(&self) -> bool {
        *self == State::Ok
    }

    /// Returns `true` if this `State` an instance of [`State::Blocked`], and returns `false`
    /// otherwise.
    ///
    /// # Examples
    /// ```
    /// # use powerlocks::rwlock::State;
    /// let state = State::Blocked;
    /// assert!(state.is_blocked());
    ///
    /// let state = State::Ok;
    /// assert!(!state.is_blocked());
    /// ```
    ///
    pub fn is_blocked(&self) -> bool {
        *self == State::Blocked
    }
}

pub type StrategyInput<'i> = &'i mut dyn Iterator<Item = &'i (HandleId, Method)>;
pub type StrategyResult<'i> = Box<dyn Iterator<Item = State> + 'i>;

///
/// A `Strategy` is a [`Fn`] or function pointer that returns a [`StrategyResult`] consisting of
/// [`State`]s that determine if each [`HandleId`] with a given [`Method`] should block or not
/// block.
///
/// The items in [`StrategyInput`] are guaranteed to be in the order which the locks were requested,
/// with the oldest entries being the first in `StrategyInput`'s `Iterator`. Each item in the
/// `Iterator<Item = State>` associated with `StrategyResult` must also match with each item in the
/// `Iterator` passed in from `StrategyInput`.
///
/// This means is that it is a logic error to return a `StrategyResult` that has an `Iterator` that
/// is not the same size as the passed in `Iterator` in `StrategyInput`. It is also a logic error to
/// return a `StrategyResult` that contains `State` entries that, in combination with the provided
/// `Method`s, would lead to violation of Rust's referencing and aliasing rules.
///
/// In particular, it is a logic error to:
///
///  - Return a `StrategyResult` having an `Iterator` with a different number of elements to the
///    passed in `StrategyInput`'s `Iterator`.
///  - Return [`State::Ok`]s for a [`Method::Read`] and a [`Method::Write`] at the same time.
///  - Return `State::Ok`s for two or more `Method::Write`s at the same time.
///  - Return a [`State::Blocked`] after returning a `State::Ok` for a particular `Method` in
///    a given position. This is becuase returning `State::Ok` indicated to a thread locking a
///    resource that it was allowed to proceed, and that requesting it to wait again may not be
///    possible.
///
/// The behaviour of a logic error is currently unspecified, but may lead to [`panic`]s and
/// [`abort`](std::process::abort)s.
///
/// # Examples
///
/// In crate [`strategies`]:
/// - [`strategies::fair`] - a fair strategy that holds Threads in a FIFO queue.
///
pub trait Strategy: Fn(StrategyInput) -> StrategyResult {}
impl<F> Strategy for F where F: ?Sized + Fn(StrategyInput) -> StrategyResult {}

#[derive(Debug)]
#[must_use = "if unused the `RwLock` will immediately unlock"]
pub struct BaseRwLockReadGuard<'a, T: 'a + ?Sized, H: Handle> {
    // It may seem as if we could get away with `&`, but no! While we are `drop`ping this guard,
    // `data` may still be live and some other thread could immediately lock the mutex while we are
    // dropping this guard (since we are releasing the lock during `drop`) and then create some
    // `&mut` along with a `&`, which is undefined behavior due to it being a `noalias` violation.
    // So use a raw pointer to prevent references etc. living during the `drop` call after release.
    //
    // `NonNull<T>` is also covariant over `T`, which is the desired property of `RwLockReadGuard`,
    // and enables niche optimization over the idiomatic `*const T`.
    // See [`std::sync::RwLockReadGuard`] for more info.
    data: NonNull<T>,
    handle: Arc<H>,
    lock: &'a impls::RwLockInner<H>,
}

impl<'a, T: 'a + ?Sized, H: Handle> BaseRwLockReadGuard<'a, T, H> {
    unsafe fn new(
        data: &'a UnsafeCell<T>,
        handle: Arc<H>,
        lock: &'a impls::RwLockInner<H>,
    ) -> Self {
        Self {
            // SAFETY: `data.get()` always returns a non-null pointer.
            data: unsafe { NonNull::new_unchecked(data.get()) },
            handle,
            lock,
        }
    }
}

// SAFETY: Unlike `RwLockReadGuard`, we are `Send` for similar reasons as why `BaseMutexGuard` is
// `Send` - we are `Handle`-based and we don't need to release the lock on the same thread, unlike
// what `pthread_mutex_unlock` requires. The `Handle` structure we have will never `park` after the
// lock is acquired, and `release` only works with the handle ID, which prevents any threading
// unsafety or conflicts that arise from `Send`ing this guard.
unsafe impl<'a, T: 'a + ?Sized + Send, H: Handle> Send for BaseRwLockReadGuard<'a, T, H> {}
unsafe impl<'a, T: 'a + ?Sized + Sync, H: Handle> Sync for BaseRwLockReadGuard<'a, T, H> {}

impl<'a, T: 'a + ?Sized, H: Handle> UnwindSafe for BaseRwLockReadGuard<'a, T, H> {}
impl<'a, T: 'a + ?Sized, H: Handle> RefUnwindSafe for BaseRwLockReadGuard<'a, T, H> {}

impl<'a, T: 'a + ?Sized, H: Handle> Deref for BaseRwLockReadGuard<'a, T, H> {
    type Target = T;
    fn deref(&self) -> &Self::Target {
        unsafe { self.data.as_ref() }
    }
}

impl<'a, T: 'a + ?Sized, H: Handle> Drop for BaseRwLockReadGuard<'a, T, H> {
    fn drop(&mut self) {
        // SAFETY: `Queue` ensures that there are no writers currently operating.
        unsafe { self.lock.finish_read(&self.handle) }
    }
}

#[derive(Debug)]
#[must_use = "if unused the `RwLock` will immediately unlock"]
pub struct BaseRwLockWriteGuard<'a, T: 'a + ?Sized, H: Handle> {
    data: NonNull<T>,
    handle: Arc<H>,
    lock: &'a impls::RwLockInner<H>,
    // Enforce invariance over `T` because `NonNull` is covariant.
    invariant_t: PhantomData<&'a mut T>,
}

impl<'a, T: 'a + ?Sized, H: Handle> BaseRwLockWriteGuard<'a, T, H> {
    unsafe fn new(
        data: &'a UnsafeCell<T>,
        handle: Arc<H>,
        lock: &'a impls::RwLockInner<H>,
    ) -> Self {
        Self {
            // SAFETY: `data.get()` always returns a non-null pointer.
            data: unsafe { NonNull::new_unchecked(data.get()) },
            handle,
            lock,
            invariant_t: PhantomData,
        }
    }
}

// SAFETY: `BaseRwLockWriteGuard` is send for the same reason as `BaseRwLockReadGuard`.
unsafe impl<'a, T: 'a + ?Sized + Send, H: Handle> Send for BaseRwLockWriteGuard<'a, T, H> {}
unsafe impl<'a, T: 'a + ?Sized + Sync, H: Handle> Sync for BaseRwLockWriteGuard<'a, T, H> {}

impl<'a, T: 'a + ?Sized, H: Handle> UnwindSafe for BaseRwLockWriteGuard<'a, T, H> {}
impl<'a, T: 'a + ?Sized, H: Handle> RefUnwindSafe for BaseRwLockWriteGuard<'a, T, H> {}

impl<'a, T: 'a + ?Sized, H: Handle> Deref for BaseRwLockWriteGuard<'a, T, H> {
    type Target = T;
    fn deref(&self) -> &Self::Target {
        unsafe { self.data.as_ref() }
    }
}

impl<'a, T: 'a + ?Sized, H: Handle> DerefMut for BaseRwLockWriteGuard<'a, T, H> {
    fn deref_mut(&mut self) -> &mut Self::Target {
        unsafe { self.data.as_mut() }
    }
}

impl<'a, T: 'a + ?Sized, H: Handle> Drop for BaseRwLockWriteGuard<'a, T, H> {
    fn drop(&mut self) {
        // SAFETY: `Queue` ensures that we have the only access as required here.
        unsafe {
            self.lock
                .finish_write(&self.handle, self.handle.panicking())
        }
    }
}

#[derive(Debug)]
pub struct BaseRwLock<T: ?Sized, H: Handle> {
    inner: impls::RwLockInner<H>,
    data: UnsafeCell<T>,
}

impl<T: Sized, H: Handle> BaseRwLock<T, H> {
    pub const fn new_strategied(t: T, strategy: Box<dyn Strategy>) -> Self {
        Self {
            inner: impls::RwLockInner::new(strategy),
            data: UnsafeCell::new(t),
        }
    }

    pub fn new(t: T) -> Self {
        BaseRwLock::new_strategied(t, Box::new(strategies::fair))
    }

    pub fn into_inner(self) -> LockResult<T> {
        impls::wrap_if_poisoned(self.is_poisoned(), self.data.into_inner())
    }
}

impl<T: ?Sized, H: Handle> BaseRwLock<T, H> {
    pub fn read(&self) -> LockResult<BaseRwLockReadGuard<T, H>> {
        let handle = self.inner.queue().acquire(Method::Read);
        // SAFETY: `acquire` ensures that no write operations are happening.
        unsafe { self.inner.do_read(handle, &self.data) }
    }

    pub fn try_read(&self) -> TryLockResult<BaseRwLockReadGuard<T, H>> {
        if let Ok(handle) = self.inner.queue().try_acquire(Method::Read) {
            // SAFETY: `try_acquire` returning `Ok` ensures that no write operations are happening.
            unsafe { self.inner.do_read(handle, &self.data) }.map_err(TryLockError::Poisoned)
        } else {
            Err(TryLockError::WouldBlock)
        }
    }

    pub fn write(&self) -> LockResult<BaseRwLockWriteGuard<T, H>> {
        let handle = self.inner.queue().acquire(Method::Write);
        // SAFETY: `acquire` ensures that this thread has exclusive access.
        unsafe { self.inner.do_write(handle, &self.data) }
    }

    pub fn try_write(&self) -> TryLockResult<BaseRwLockWriteGuard<T, H>> {
        if let Ok(handle) = self.inner.queue().try_acquire(Method::Write) {
            // SAFETY: `try_acquire` returning `Ok` ensures that this thread has exclusive access.
            unsafe { self.inner.do_write(handle, &self.data) }.map_err(TryLockError::Poisoned)
        } else {
            Err(TryLockError::WouldBlock)
        }
    }

    pub fn is_poisoned(&self) -> bool {
        self.inner.is_poisoned()
    }

    pub fn clear_poison(&self) {
        self.inner.clear_poison();
    }

    pub fn get_mut(&mut self) -> LockResult<&mut T> {
        impls::wrap_if_poisoned(self.is_poisoned(), self.data.get_mut())
    }
}

impl<T: Sized, H: Handle> From<T> for BaseRwLock<T, H> {
    fn from(value: T) -> Self {
        Self::new(value)
    }
}

impl<T: Default, H: Handle> Default for BaseRwLock<T, H> {
    fn default() -> Self {
        Self::new(T::default())
    }
}

// SAFETY: `UnsafeCell` should be safe to send to another thread when `T` is `Send`. Hence we should
// be `Send`.
unsafe impl<T: ?Sized + Send, H: Handle> Send for BaseRwLock<T, H> {}

// SAFETY: `RwLock` promises to protect against `UnsafeCell`'s main barrier to being `Sync` by
// locking each thread's access to the `get` method.
//
// As for the `T` parameter, it must be `Sync` or else multiple threads could share read-references
// to `T` at the same time. It must also be `Send` since a reference to `RwLock` allows other
// threads to access, write to, and therefore `Send` `&mut T` and hence `T` across thread
// boundaries.
unsafe impl<T: ?Sized + Send + Sync, H: Handle> Sync for BaseRwLock<T, H> {}

impl<T: ?Sized, H: Handle> UnwindSafe for BaseRwLock<T, H> {}
impl<T: ?Sized, H: Handle> RefUnwindSafe for BaseRwLock<T, H> {}

impl<'a, T: ?Sized, H: Handle> RwLockReadGuardApi<'a, T> for BaseRwLockReadGuard<'a, T, H> {}
impl<'a, T: ?Sized, H: Handle> RwLockWriteGuardApi<'a, T> for BaseRwLockWriteGuard<'a, T, H> {}

impl<T: ?Sized, H: Handle> RwLockApi<T> for BaseRwLock<T, H> {
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

pub type CoreRwLockReadGuard<'a, T> = BaseRwLockReadGuard<'a, T, CoreHandle>;
pub type CoreRwLockWriteGuard<'a, T> = BaseRwLockWriteGuard<'a, T, CoreHandle>;
pub type CoreRwLock<T> = BaseRwLock<T, CoreHandle>;

#[cfg(not(feature = "std"))]
mod types {
    use super::{BaseRwLock, BaseRwLockReadGuard, BaseRwLockWriteGuard};
    use crate::primitives::CoreHandle;

    pub type RwLockReadGuard<'a, T> = BaseRwLockReadGuard<'a, T, CoreHandle>;
    pub type RwLockWriteGuard<'a, T> = BaseRwLockWriteGuard<'a, T, CoreHandle>;
    pub type RwLock<T> = BaseRwLock<T, CoreHandle>;
}

#[cfg(feature = "std")]
mod types {
    use super::{BaseRwLock, BaseRwLockReadGuard, BaseRwLockWriteGuard};
    use crate::primitives::StdHandle;

    pub type StdRwLockReadGuard<'a, T> = BaseRwLockReadGuard<'a, T, StdHandle>;
    pub type StdRwLockWriteGuard<'a, T> = BaseRwLockWriteGuard<'a, T, StdHandle>;
    pub type StdRwLock<T> = BaseRwLock<T, StdHandle>;

    pub type RwLockReadGuard<'a, T> = BaseRwLockReadGuard<'a, T, StdHandle>;
    pub type RwLockWriteGuard<'a, T> = BaseRwLockWriteGuard<'a, T, StdHandle>;
    pub type RwLock<T> = BaseRwLock<T, StdHandle>;
}

pub use types::*;
