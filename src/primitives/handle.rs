use core::ops::Deref;

#[cfg(not(feature = "mutex"))]
compile_error!("Internal crate error: `handle.rs` requires the `mutex` feature.");

mod handle_type {
    pub(super) type HandleIdBase = u128;
    pub(super) type HandleIdAtomicBase = crate::mutex::CoreMutex<u128>;
}

use handle_type::{HandleIdAtomicBase, HandleIdBase};

static HANDLE_COUNTER: HandleIdAtomicBase = HandleIdAtomicBase::new(1);

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

pub unsafe trait Handle {
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
    fn yield_now(&self);

    fn panicking(&self) -> bool {
        false
    }
}

#[derive(Debug, Clone)]
pub struct CoreHandle(HandleId);

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

    fn yield_now(&self) {
        core::hint::spin_loop();
    }
}

#[cfg(feature = "std")]
mod std_handle {
    use super::{Handle, HandleId};

    #[cfg(feature = "std")]
    extern crate std;

    use std::thread::{self, Thread};

    #[derive(Debug, Clone)]
    pub struct StdHandle {
        id: HandleId,
        thread: Thread,
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

        fn yield_now(&self) {
            assert_eq!(thread::current().id(), self.thread.id());
            thread::yield_now();
        }

        fn panicking(&self) -> bool {
            assert_eq!(thread::current().id(), self.thread.id());
            thread::panicking()
        }
    }
}

#[cfg(feature = "std")]
pub use std_handle::*;
