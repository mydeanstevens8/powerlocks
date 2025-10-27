use core::{
    marker::PhantomData,
    sync::atomic::{AtomicBool, AtomicUsize, Ordering},
};

use super::MultiHook;
use crate::primitives::{CoreThreadEnv, ShouldBlock, ThreadEnv};

pub struct FairHook<Env: ThreadEnv = CoreThreadEnv> {
    current_ticket: AtomicUsize,
    next_read_ticket: AtomicUsize,
    next_write_ticket: AtomicUsize,
    locked: AtomicBool,
    thread_env: PhantomData<Env>,
}

impl<Env: ThreadEnv> FairHook<Env> {
    fn critical<T>(&self, f: impl FnOnce() -> T) -> T {
        while self
            .locked
            .compare_exchange(false, true, Ordering::Acquire, Ordering::Acquire)
            .is_err()
        {
            Env::yield_now();
        }

        let result = f();

        self.locked.store(false, Ordering::Release);

        result
    }
}

impl<Env: ThreadEnv> MultiHook<Env> for FairHook<Env> {
    fn new() -> Self
    where
        Self: Sized,
    {
        Self {
            current_ticket: AtomicUsize::new(0),
            next_read_ticket: AtomicUsize::new(0),
            next_write_ticket: AtomicUsize::new(0),
            locked: AtomicBool::new(false),
            thread_env: PhantomData,
        }
    }

    fn try_read(&self) -> ShouldBlock {
        self.critical(|| {
            if self.current_ticket.load(Ordering::Relaxed)
                == self.next_read_ticket.load(Ordering::Relaxed)
            {
                self.next_write_ticket.fetch_add(1, Ordering::Relaxed);
                ShouldBlock::Ok
            } else {
                ShouldBlock::Block
            }
        })
    }

    fn read(&self) {}

    fn after_read(&self) {}

    fn try_write(&self) -> ShouldBlock {
        self.critical(|| {
            if self.current_ticket.load(Ordering::Relaxed)
                == self.next_write_ticket.load(Ordering::Relaxed)
            {
                self.next_read_ticket.fetch_add(1, Ordering::Relaxed);
                self.next_write_ticket.fetch_add(1, Ordering::Relaxed);
                ShouldBlock::Ok
            } else {
                ShouldBlock::Block
            }
        })
    }

    fn write(&self) {}

    fn after_write(&self) {}
}
