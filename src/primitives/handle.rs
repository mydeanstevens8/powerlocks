use core::ops::Deref;

#[cfg(not(feature = "mutex"))]
compile_error!("Internal crate error: `handle.rs` requires the `mutex` feature.");

mod handle_type {
    pub(super) type HandleIdBase = u128;
    pub(super) type HandleIdAtomicBase = crate::mutex::CoreMutex<u128>;
}

use handle_type::{HandleIdAtomicBase, HandleIdBase};

static HANDLE_COUNTER: HandleIdAtomicBase = HandleIdAtomicBase::new_unhooked(1);

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct HandleId(HandleIdBase);
impl HandleId {
    fn new() -> Self {
        if *HANDLE_COUNTER.lock().unwrap() == u128::MAX {
            panic!("Exhausted `HandleId::new()`.");
        }

        let val = {
            let mut guard = HANDLE_COUNTER.lock().unwrap();
            let val = *guard;
            *guard += 1;
            drop(guard);
            val
        };

        Self(val)
    }

    fn new_dumb() -> Self {
        Self(0)
    }
}

impl Deref for HandleId {
    type Target = HandleIdBase;
    fn deref(&self) -> &Self::Target {
        &self.0
    }
}

pub trait ThreadEnv {
    fn yield_now()
    where
        Self: Sized,
    {
    }

    fn panicking() -> bool
    where
        Self: Sized,
    {
        false
    }
}

/// The core primitive for interacting with a thread environment, independent of the OS.
///
/// # Safety
/// Libraries may assume that this `Handle` is correctly implemented. In particular, the following
/// properties must hold for each handle:
///  - [`new`](Handle::new) must always return a `Handle` with a unique [`HandleId`] (retrieved from
///    `id`) every time it is called. No two `HandleId`s can be the same using `new`.
///  - [`dumb`](Handle::dumb) must always return a `Handle` with the same `HandleId` (from `id`)
///    every time it is called. It cannot return different `HandleIds` on each invocation.
///
/// Failing to uphold these properties may lead to incorrect synchronization in crate libraries,
/// enabling data races and undefined behavior.
///
/// Unsafe code however must not assume that `park` and `unpark` are correctly implemented.
///
/// # Other functions
/// With [`park`](Handle::park) and [`unpark`](Handle::unpark), implementors should abide by the
/// following properties:
///  - `unpark` should not block the current thread, and it should release the `park` on the target
///    thread (if any are in progress).
///  - `park` ideally should block, but is not required to (due to implementations permitting
///    spurious wakeups).
///
/// It is a logic error for `unpark` to not satisfy the above properties.
///
pub unsafe trait Handle: ThreadEnv {
    fn new() -> Self
    where
        Self: Sized;

    fn dumb() -> Self
    where
        Self: Sized,
    {
        Self::new()
    }

    fn id(&self) -> HandleId;
    fn park(&self);
    fn unpark(&self);
}

#[derive(Debug, Clone, Copy)]
pub struct CoreThreadEnv;
impl ThreadEnv for CoreThreadEnv {
    fn yield_now()
    where
        Self: Sized,
    {
        core::hint::spin_loop();
    }

    fn panicking() -> bool
    where
        Self: Sized,
    {
        false
    }
}

#[derive(Debug, Clone)]
pub struct CoreHandle(HandleId);

impl ThreadEnv for CoreHandle {
    fn yield_now()
    where
        Self: Sized,
    {
        CoreThreadEnv::yield_now();
    }
}

unsafe impl Handle for CoreHandle {
    fn new() -> Self
    where
        Self: Sized,
    {
        Self(HandleId::new())
    }

    fn dumb() -> Self
    where
        Self: Sized,
    {
        Self(HandleId::new_dumb())
    }

    fn id(&self) -> HandleId {
        self.0
    }

    fn park(&self) {
        core::hint::spin_loop();
    }

    fn unpark(&self) {}
}

#[cfg(feature = "std")]
mod std_handle {
    use super::{Handle, HandleId, ThreadEnv};

    #[cfg(feature = "std")]
    extern crate std;

    use std::thread::{self, Thread};

    #[derive(Debug, Clone, Copy)]
    pub struct StdThreadEnv;
    impl ThreadEnv for StdThreadEnv {
        fn yield_now() {
            thread::yield_now();
        }

        fn panicking() -> bool {
            thread::panicking()
        }
    }

    #[derive(Debug, Clone)]
    pub struct StdHandle {
        id: HandleId,
        thread: Thread,
    }

    impl ThreadEnv for StdHandle {
        fn yield_now() {
            StdThreadEnv::yield_now();
        }

        fn panicking() -> bool {
            StdThreadEnv::panicking()
        }
    }

    unsafe impl Handle for StdHandle {
        fn new() -> Self
        where
            Self: Sized,
        {
            Self {
                id: HandleId::new(),
                thread: thread::current(),
            }
        }

        fn dumb() -> Self
        where
            Self: Sized,
        {
            Self {
                id: HandleId::new_dumb(),
                thread: thread::current(),
            }
        }

        fn id(&self) -> HandleId {
            self.id
        }

        fn park(&self) {
            assert_eq!(thread::current().id(), self.thread.id());
            thread::park();
        }

        fn unpark(&self) {
            self.thread.unpark();
        }
    }
}

#[cfg(feature = "std")]
pub use std_handle::*;
