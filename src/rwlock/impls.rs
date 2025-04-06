use core::{
    cell::UnsafeCell,
    error::Error,
    fmt::{Debug, Display},
    hash::Hash,
    sync::atomic::{AtomicBool, Ordering},
};

extern crate alloc;
use alloc::{boxed::Box, collections::VecDeque, string::ToString, sync::Arc, vec::Vec};

use crate::{
    mutex::Mutex,
    primitives::{Handle, LockResult, PoisonError},
};

use super::{BaseRwLockReadGuard, BaseRwLockWriteGuard, Method, State, Strategy};

pub(super) enum LogicErrorHandlingMethod {
    Panic,
    BreakAndPanic,
}

macro_rules! error_type {
    ($vis:vis $name:ident { $($option:ident($message:literal, $handling:expr)),* $(,)? }) => {
        #[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
        $vis enum $name {
            $($option,)*
        }

        impl $name {
            fn handling_method(&self) -> LogicErrorHandlingMethod {
                match *self {
                    $(Self::$option => $handling,)*
                }
            }
        }

        impl Display for $name {
            #[allow(unused_variables)] // If we're generating an empty enum
            fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
                match *self {
                    $(Self::$option => write!(f, $message),)*
                }
            }
        }

        impl Error for $name {}
    };
}

error_type!(pub(super) StrategyLogicError {
    ConcurrentReadAndWrite(
        "The provided `Strategy` wanted to `State::Ok` a `Method::Write` and a \
        `Method::Read` together.",
        LogicErrorHandlingMethod::BreakAndPanic
    ),
    ConcurrentMultipleWrites(
        "The provided `Strategy` wanted to `State::Ok` two or more `Method::Write`s.",
        LogicErrorHandlingMethod::BreakAndPanic
    ),
    BlockedAfterOkState(
        "The provided `Strategy` wanted to re-block a `State::Ok`ed thread.",
        LogicErrorHandlingMethod::BreakAndPanic
    ),
    BrokenLock(
        "There is a logic error in the provided `Strategy`. Can't continue.",
        LogicErrorHandlingMethod::Panic
    )
});

#[cold]
#[inline(never)]
#[track_caller]
fn cold<F>(f: F) -> F {
    f
}

#[derive(Clone, Debug, PartialEq, Eq, Hash)]
struct LockEntry<H: Handle> {
    handle: Arc<H>,
    method: Method,
    state: State,
}

impl<H: Handle> LockEntry<H> {
    pub(super) fn new(handle: Arc<H>, method: Method, state: State) -> Self {
        Self {
            handle,
            method,
            state,
        }
    }

    pub(super) fn state(&self) -> State {
        self.state
    }
}

struct LockedQueue<H: Handle> {
    queue: VecDeque<LockEntry<H>>,
    strategy: Box<dyn Strategy>,
    broken: bool,
}

impl<H: Handle> Debug for LockedQueue<H> {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        f.debug_struct("LockedQueue").finish_non_exhaustive()
    }
}

// N.B. This object acts as a crticial section of which no other thread can access while it's
// locked. So this should only be held on for the shortest amount of time possible.
struct LockedQueueView<'a, H: Handle> {
    queue: &'a mut VecDeque<LockEntry<H>>,
    strategy: &'a mut dyn Strategy,
    broken: &'a mut bool,
}

impl<H: Handle> Debug for LockedQueueView<'_, H> {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        f.debug_struct("LockedQueueView").finish_non_exhaustive()
    }
}

impl<'a, H: Handle> LockedQueueView<'a, H> {
    fn new(queue: &'a mut LockedQueue<H>) -> Self {
        Self {
            queue: &mut queue.queue,
            strategy: &mut *queue.strategy,
            broken: &mut queue.broken,
        }
    }

    fn is_broken(&self) -> bool {
        *self.broken
    }

    fn assert_not_broken(&mut self) {
        if self.is_broken() {
            self.handle_logic_err(StrategyLogicError::BrokenLock)
        }
    }

    #[cold]
    #[inline(never)]
    fn handle_logic_err(&mut self, err: StrategyLogicError) -> ! {
        match err.handling_method() {
            LogicErrorHandlingMethod::BreakAndPanic => {
                *self.broken = true;
                panic!("{}", err.to_string())
            }
            LogicErrorHandlingMethod::Panic => {
                panic!("{}", err.to_string())
            }
        }
    }

    fn set_and_enforce_preconditions(
        &mut self,
        current_handle: &H,
        new_states: &mut dyn Iterator<Item = State>,
    ) -> Result<(), StrategyLogicError> {
        #[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
        struct Violations {
            has_ok_read: bool,
            has_ok_write: bool,
            err_blocked_after_ok_state: bool,
            err_concurrent_read_and_write: bool,
            err_concurent_multiple_writes: bool,
        }

        let violations = self.queue.iter_mut().zip(new_states).fold(
            Violations {
                has_ok_read: false,
                has_ok_write: false,
                err_blocked_after_ok_state: false,
                err_concurrent_read_and_write: false,
                err_concurent_multiple_writes: false,
            },
            |mut violations, (entry, mut new_state)| {
                // The current handle's state may initially be set to `State::Ok` while the
                // `Strategy` mandates a `State::Blocked` state. Permit this since the
                // current thread is always attempting an acquire here, and can be blocked via the
                // results of this function. The current thread never appears here during a release
                // of the lock since it's removed from the queue before calling this function.
                if entry.handle.id() != current_handle.id()
                    && entry.state().is_ok()
                    && new_state.is_blocked()
                {
                    violations.err_blocked_after_ok_state = true;
                    new_state = State::Ok;
                }

                if new_state.is_ok() {
                    match entry.method {
                        Method::Read => {
                            violations.err_concurrent_read_and_write |= violations.has_ok_write;
                            violations.has_ok_read = true;
                        }
                        Method::Write => {
                            violations.err_concurrent_read_and_write |= violations.has_ok_read;
                            violations.err_concurent_multiple_writes |= violations.has_ok_write;
                            violations.has_ok_write = true;
                        }
                    }

                    if violations.err_concurrent_read_and_write
                        || violations.err_concurent_multiple_writes
                    {
                        new_state = State::Blocked;
                    }
                }

                entry.state = new_state;
                violations
            },
        );

        if violations.err_blocked_after_ok_state {
            cold(Err(StrategyLogicError::BlockedAfterOkState))
        } else if violations.err_concurrent_read_and_write {
            cold(Err(StrategyLogicError::ConcurrentReadAndWrite))
        } else if violations.err_concurent_multiple_writes {
            cold(Err(StrategyLogicError::ConcurrentMultipleWrites))
        } else {
            Ok(())
        }
    }

    fn run_queue_logic(&mut self, current_handle: &H) -> Result<(), StrategyLogicError> {
        // Run the strategy and enforce preconditions.
        let handles_and_methods = self
            .queue
            .iter()
            .map(|entry| (entry.handle.id(), entry.method))
            .collect::<Vec<_>>();

        let mut handles_and_methods_iter = handles_and_methods.iter();
        let mut raw_results = (self.strategy)(&mut handles_and_methods_iter);

        self.set_and_enforce_preconditions(current_handle, &mut raw_results)?;

        // Then unpark handles as needed
        self.queue.iter_mut().for_each(|entry| {
            if entry.handle.id() != current_handle.id() && entry.state().is_ok() {
                entry.handle.unpark();
            }
        });

        Ok(())
    }

    fn current_entry(&self, current_handle: &H) -> Option<&LockEntry<H>> {
        self.queue
            .iter()
            .find(|entry| entry.handle.id() == current_handle.id())
    }

    fn poll(&mut self, current_handle: &H) -> State {
        self.current_entry(current_handle)
            // The `None` case should never happen, as there's no way for us to remove a lock entry
            // without going through `try_acquire` or `release`
            .unwrap_or_else(|| unreachable!())
            .state()
    }

    fn do_acquire(&mut self, method: Method) -> (Arc<H>, State) {
        self.assert_not_broken();
        let current_handle = Arc::new(H::new());

        // Will be enforced by the `Strategy`
        self.queue.push_back(LockEntry::<H>::new(
            Arc::clone(&current_handle),
            method,
            State::Blocked,
        ));
        self.run_queue_logic(&current_handle)
            .unwrap_or_else(|err| self.handle_logic_err(err));
        let state = self.poll(&current_handle);

        (current_handle, state)
    }

    fn acquire(&mut self, method: Method) -> Arc<H> {
        self.do_acquire(method).0
    }

    fn try_acquire(&mut self, method: Method) -> Result<Arc<H>, ()> {
        let (handle, state) = self.do_acquire(method);

        if state.is_blocked() {
            // `do_acquire` always puts an entry into `queue` regardless. Since we're only
            // trying the lock, remove that last entry.
            let old_entry = self.queue.pop_back();

            // Do a sanity check here and make sure...
            if old_entry.is_none_or(|entry| entry.handle.id() != handle.id()) {
                // This is unreachable. We've just done a `push_back` of the exact same entry.
                unreachable!()
            }
        }

        state.is_ok().then_some(handle).ok_or(())
    }

    fn release(&mut self, current_handle: &H) {
        let result = self
            .queue
            .iter()
            .position(|entry| entry.handle.id() == current_handle.id())
            .and_then(|index| self.queue.remove(index));

        // Try not to panic if we are broken. We want threads releasing the `RwLockReadGuard` and
        // `RwLockWriteGuard` to work gracefully.
        if !self.is_broken() {
            result.unwrap();
            self.run_queue_logic(current_handle)
                .unwrap_or_else(|err| self.handle_logic_err(err));
        }
    }
}

#[derive(Debug)]
pub(super) struct Queue<H: Handle> {
    inner: Mutex<LockedQueue<H>>,
}

impl<H: Handle> Queue<H> {
    pub(super) const fn new(strategy: Box<dyn Strategy>) -> Self {
        Self {
            inner: Mutex::new_unhooked(LockedQueue {
                queue: VecDeque::new(),
                strategy,
                broken: false,
            }),
        }
    }

    fn lock<T>(&self, callback: impl for<'a> FnOnce(LockedQueueView<'a, H>) -> T) -> T {
        callback(LockedQueueView::new(
            &mut self.inner.lock().unwrap_or_else(PoisonError::into_inner),
        ))
    }

    pub(super) fn acquire(&self, method: Method) -> Arc<H> {
        let handle = self.lock(|mut queue| queue.acquire(method));
        while self.lock(|mut queue| queue.poll(&handle)).is_blocked() {
            handle.park();
        }

        handle
    }

    pub(super) fn try_acquire(&self, method: Method) -> Result<Arc<H>, ()> {
        self.lock(|mut queue| queue.try_acquire(method))
    }

    pub(super) fn release(&self, handle: &H) {
        self.lock(|mut queue| queue.release(handle));
    }
}

pub(super) fn wrap_if_poisoned<U>(poisoned: bool, data: U) -> LockResult<U> {
    match poisoned {
        true => Err(PoisonError::new(data)),
        false => Ok(data),
    }
}

#[derive(Debug)]
pub(super) struct RwLockInner<H: Handle> {
    queue: Queue<H>,
    poisoned: AtomicBool,
}

impl<H: Handle> RwLockInner<H> {
    pub(super) const fn new(strategy: Box<dyn Strategy>) -> Self {
        Self {
            queue: Queue::new(strategy),
            poisoned: AtomicBool::new(false),
        }
    }

    pub(super) fn queue(&self) -> &Queue<H> {
        &self.queue
    }

    pub(super) unsafe fn do_read<'a, T: ?Sized>(
        &'a self,
        handle: Arc<H>,
        data: &'a UnsafeCell<T>,
    ) -> LockResult<BaseRwLockReadGuard<'a, T, H>> {
        wrap_if_poisoned(self.is_poisoned(), unsafe {
            BaseRwLockReadGuard::new(data, handle, self)
        })
    }

    pub(super) unsafe fn do_write<'a, T: ?Sized>(
        &'a self,
        handle: Arc<H>,
        data: &'a UnsafeCell<T>,
    ) -> LockResult<BaseRwLockWriteGuard<'a, T, H>> {
        wrap_if_poisoned(self.is_poisoned(), unsafe {
            BaseRwLockWriteGuard::new(data, handle, self)
        })
    }

    pub(super) fn is_poisoned(&self) -> bool {
        self.poisoned.load(Ordering::Relaxed)
    }

    pub(super) fn clear_poison(&self) {
        self.poisoned.store(false, Ordering::Relaxed);
    }

    // `unsafe` enforces the locking invariant in the parent module.
    pub(super) unsafe fn finish_read(&self, handle: &H) {
        self.queue.release(handle);
        // The lock is not poisoned as the underlying `T` can't be mutated while `read`ing, which
        // could otherwise expose corrupt state. This is consistent with Rust's `RwLock`.
    }

    // `unsafe` enforces the locking invariant in the parent module.
    pub(super) unsafe fn finish_write(&self, handle: &H, poison: bool) {
        self.queue.release(handle);
        self.poisoned.fetch_or(poison, Ordering::AcqRel);
    }
}
