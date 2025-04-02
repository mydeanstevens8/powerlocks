use std::{
    iter,
    ops::Deref,
    panic::{AssertUnwindSafe, catch_unwind, resume_unwind},
    sync::{
        RwLock, TryLockError,
        atomic::{AtomicBool, Ordering},
    },
    thread::yield_now,
    time::{Duration, Instant},
};

pub const SUGGESTED_LOCK_WAIT: Duration = Duration::from_secs(10);
pub const SUGGESTED_NO_LOCK_WAIT: Duration = Duration::from_millis(50);

#[derive(Debug)]
pub struct CheckerHandle {
    locked: AtomicBool,
}

impl CheckerHandle {
    pub fn new() -> Self {
        Self {
            locked: AtomicBool::new(false),
        }
    }

    pub fn acquire(&self) {
        match self
            .locked
            .compare_exchange(false, true, Ordering::Acquire, Ordering::Acquire)
        {
            Ok(_) => {
                while self.is_locked() {
                    yield_now()
                }
            }
            Err(_) => panic!("`CheckerHandle` was already acquired."),
        }
    }

    pub fn is_locked(&self) -> bool {
        self.locked.load(Ordering::Acquire)
    }

    pub fn will_be_locked_by(&self, delay: Duration) -> bool {
        let start = Instant::now();
        loop {
            if self.is_locked() {
                break true;
            } else if Instant::now().saturating_duration_since(start) >= delay {
                break false;
            } else {
                yield_now();
            }
        }
    }

    pub fn will_be_locked(&self) -> bool {
        self.will_be_locked_by(SUGGESTED_LOCK_WAIT)
    }

    pub fn will_not_be_locked(&self) -> bool {
        !self.will_be_locked_by(SUGGESTED_NO_LOCK_WAIT)
    }

    pub fn release(&self) {
        self.locked.store(false, Ordering::Release);
    }
}

#[derive(Debug)]
pub struct CheckerHandles(Vec<CheckerHandle>);

impl CheckerHandles {
    pub fn new(count: usize) -> Self {
        Self(
            iter::from_fn(|| Some(CheckerHandle::new()))
                .take(count)
                .collect(),
        )
    }

    pub fn guard<R>(&self, f: impl FnOnce() -> R) -> R {
        match catch_unwind(AssertUnwindSafe(f)) {
            Ok(r) => r,
            Err(payload) => {
                for handle in &self.0 {
                    if handle.is_locked() {
                        handle.release();
                    }
                }
                resume_unwind(payload)
            }
        }
    }
}

impl Deref for CheckerHandles {
    type Target = [CheckerHandle];

    fn deref(&self) -> &Self::Target {
        &self.0
    }
}

pub struct RaceChecker {
    checker: RwLock<()>,
}

impl RaceChecker {
    pub fn new() -> Self {
        Self {
            checker: RwLock::new(()),
        }
    }

    // Used by `rwlock` tests, but not by `mutex` tests.
    #[allow(dead_code)]
    pub fn try_read(&self, handle: &CheckerHandle) -> Result<(), ()> {
        let guard = match self.checker.try_read() {
            Ok(guard) => Ok(guard),
            Err(TryLockError::Poisoned(guard)) => Ok(guard.into_inner()),
            Err(TryLockError::WouldBlock) => Err(()),
        }?;
        handle.acquire();
        Ok(drop(guard))
    }

    // Used by `rwlock` tests, but not by `mutex` tests.
    #[allow(dead_code)]
    pub fn read(&self, handle: &CheckerHandle) {
        self.try_read(handle).expect("read failed")
    }

    pub fn try_write(&self, handle: &CheckerHandle) -> Result<(), ()> {
        let guard = match self.checker.try_write() {
            Ok(guard) => Ok(guard),
            Err(TryLockError::Poisoned(guard)) => Ok(guard.into_inner()),
            Err(TryLockError::WouldBlock) => Err(()),
        }?;
        handle.acquire();
        Ok(drop(guard))
    }

    pub fn write(&self, handle: &CheckerHandle) {
        self.try_write(handle).expect("write failed")
    }
}
