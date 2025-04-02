#![cfg(all(feature = "rwlock", feature = "std"))]

use std::{
    cell::UnsafeCell,
    hint::black_box,
    iter,
    panic::{RefUnwindSafe, UnwindSafe},
    thread,
};

use powerlocks::{
    primitives::TryLockError,
    rwlock::{
        Method, RwLock, RwLockReadGuard, RwLockWriteGuard, State, StrategyInput, StrategyResult,
        strategies,
    },
};

mod rwlock_utils;
use rwlock_utils::{
    StrategyLogicError,
    TryStrategyAttempt::{Try, UnlockAll},
    suppress_panic_message, try_strategy,
};

mod utils;
use utils::{
    assert_is_trait,
    race_checker::{CheckerHandles, RaceChecker},
};

macro_rules! assert_requires_unwind {
    () => {
        // Some of the tests here require child threads to panic, and parent threads to catch their
        // panics. This requires the "unwind" or a similar strategy, which only affects a single
        // thread, rather than "abort", which immediately kills the entire process on a panic and
        // fails the test. Note that `#[should_panic]`, which expects the parent (main) thread to
        // panic, and not the child threads, cannot be used in the tests that call this macro, since
        // the main threads do not panic.
        if cfg!(not(panic = "unwind")) {
            // Let builds succeed so we can properly track this failure in CI without blocking the
            // build of other test harnesses.
            panic!(
r#"This `RwLock` suite was built using `panic = "abort"`, rather than the expected `panic = "unwind"`.
    note: `panic = "unwind"` is required due to usage of `requires_child_thread_unwind` in this test."#
            )
        }
    }
}

#[test]
fn assert_trait() {
    assert_is_trait!(RwLock<()>, Send, Sync);
    assert_is_trait!(RwLock<bool>, Send, Sync);
    assert_is_trait!(RwLock<i32>, Send, Sync);
    assert_is_trait!(RwLock<usize>, Send, Sync);
    assert_is_trait!(RwLock<isize>, Send, Sync);

    assert_is_trait!(RwLock<()>, UnwindSafe, RefUnwindSafe, Unpin);
    assert_is_trait!(RwLock<i32>, UnwindSafe, RefUnwindSafe, Unpin);

    assert_is_trait!(UnsafeCell<i32>, Send);
    assert_is_trait!(UnsafeCell<i32>, !Sync);
    assert_is_trait!(RwLock<UnsafeCell<i32>>, Send);
    assert_is_trait!(RwLock<UnsafeCell<i32>>, !Sync);
    assert_is_trait!(RwLock<UnsafeCell<i32>>, UnwindSafe, RefUnwindSafe);
    assert_is_trait!(RwLock<UnsafeCell<i32>>, Unpin);

    assert_is_trait!(std::sync::RwLockReadGuard<'_, i32>, !Send);
    assert_is_trait!(std::sync::RwLockReadGuard<'_, i32>, Sync);
    assert_is_trait!(RwLock<std::sync::RwLockReadGuard<'_, i32>>, !Send, !Sync);
    assert_is_trait!(
        RwLock<std::sync::RwLockReadGuard<'_, i32>>,
        UnwindSafe,
        RefUnwindSafe
    );
    assert_is_trait!(RwLock<std::sync::RwLockReadGuard<'_, i32>>, Unpin);

    assert_is_trait!(*const (), !Send, !Sync);
    assert_is_trait!(RwLock<*const ()>, !Send, !Sync);
    assert_is_trait!(RwLock<*const ()>, UnwindSafe, RefUnwindSafe, Unpin);

    assert_is_trait!(*mut (), !Send, !Sync);
    assert_is_trait!(RwLock<*mut ()>, !Send, !Sync);
    assert_is_trait!(RwLock<*mut ()>, UnwindSafe, RefUnwindSafe, Unpin);

    assert_is_trait!(RwLockReadGuard<'_, ()>, !Send);
    assert_is_trait!(RwLockReadGuard<'_, ()>, Sync);
    assert_is_trait!(RwLockReadGuard<'_, ()>, UnwindSafe, RefUnwindSafe, Unpin);

    assert_is_trait!(RwLockReadGuard<'_, i32>, !Send);
    assert_is_trait!(RwLockReadGuard<'_, i32>, Sync);
    assert_is_trait!(RwLockReadGuard<'_, i32>, UnwindSafe, RefUnwindSafe, Unpin);

    assert_is_trait!(RwLockReadGuard<'_, UnsafeCell<i32>>, !Send, !Sync);
    assert_is_trait!(RwLockReadGuard<'_, *const ()>, !Send, !Sync);
    assert_is_trait!(
        RwLockReadGuard<'_, std::sync::RwLockReadGuard<'_, i32>>,
        !Send
    );
    assert_is_trait!(
        RwLockReadGuard<'_, std::sync::RwLockReadGuard<'_, i32>>,
        Sync
    );

    assert_is_trait!(RwLockWriteGuard<'_, i32>, !Send);
    assert_is_trait!(RwLockWriteGuard<'_, i32>, Sync);
    assert_is_trait!(RwLockWriteGuard<'_, i32>, UnwindSafe, RefUnwindSafe, Unpin);

    assert_is_trait!(RwLockWriteGuard<'_, UnsafeCell<i32>>, !Send, !Sync);
    assert_is_trait!(RwLockWriteGuard<'_, *const ()>, !Send, !Sync);
    assert_is_trait!(
        RwLockWriteGuard<'_, std::sync::RwLockReadGuard<'_, i32>>,
        !Send
    );
    assert_is_trait!(
        RwLockWriteGuard<'_, std::sync::RwLockReadGuard<'_, i32>>,
        Sync
    );
}

#[test]
fn run_single_thread_unit() {
    let locked_unit = RwLock::new(());

    assert_eq!(*locked_unit.read().unwrap(), ());
    assert_eq!(*locked_unit.write().unwrap(), ());
    assert_eq!(*locked_unit.read().unwrap(), ());
    assert_eq!(*locked_unit.write().unwrap(), ());
}

#[test]
fn run_single_thread_vec() {
    let locked_vec = RwLock::new(vec![1, 2, 3, 4, 5]);

    locked_vec.write().unwrap().push(6);
    assert_eq!(*locked_vec.read().unwrap(), [1, 2, 3, 4, 5, 6]);

    assert_eq!(locked_vec.write().unwrap().pop().unwrap(), 6);
    assert_eq!(*locked_vec.read().unwrap(), [1, 2, 3, 4, 5]);

    assert_eq!(locked_vec.write().unwrap().pop().unwrap(), 5);
    assert_eq!(*locked_vec.read().unwrap(), [1, 2, 3, 4]);
}

#[test]
fn race_reads() {
    let lock = RwLock::new_strategied(RaceChecker::new(), Box::new(strategies::fair));
    let handles = CheckerHandles::new(4);

    thread::scope(|scope| {
        handles.guard(|| {
            scope.spawn(|| lock.read().unwrap().read(&handles[0]));
            assert!(handles[0].will_be_locked());
            handles[0].release();

            scope.spawn(|| lock.read().unwrap().read(&handles[1]));
            assert!(handles[1].will_be_locked());
            scope.spawn(|| lock.read().unwrap().read(&handles[2]));
            assert!(handles[1].is_locked());
            assert!(handles[2].will_be_locked());
            handles[1].release();
            scope.spawn(|| lock.read().unwrap().read(&handles[3]));
            assert!(handles[3].will_be_locked());
            handles[2].release();
            handles[3].release();
        });
    });
}

#[test]
fn race_writes() {
    let lock = RwLock::new_strategied(RaceChecker::new(), Box::new(strategies::fair));
    let handles = CheckerHandles::new(4);

    thread::scope(|scope| {
        handles.guard(|| {
            scope.spawn(|| lock.write().unwrap().write(&handles[0]));
            assert!(handles[0].will_be_locked());
            handles[0].release();

            scope.spawn(|| lock.write().unwrap().write(&handles[1]));
            assert!(handles[1].will_be_locked());
            scope.spawn(|| lock.write().unwrap().write(&handles[2]));
            assert!(handles[2].will_not_be_locked());
            handles[1].release();
            assert!(handles[2].will_be_locked());
            scope.spawn(|| lock.write().unwrap().write(&handles[3]));
            assert!(handles[3].will_not_be_locked());
            handles[2].release();
            assert!(handles[3].will_be_locked());
            handles[3].release();
        });
    });
}

#[test]
fn race_fair_write_then_read() {
    let lock = RwLock::new_strategied(RaceChecker::new(), Box::new(strategies::fair));
    let handles = CheckerHandles::new(6);

    thread::scope(|scope| {
        handles.guard(|| {
            scope.spawn(|| lock.read().unwrap().read(&handles[0]));
            assert!(handles[0].will_be_locked());
            scope.spawn(|| lock.write().unwrap().write(&handles[1]));
            assert!(handles[1].will_not_be_locked());
            handles[0].release();
            assert!(handles[1].will_be_locked());
            scope.spawn(|| lock.read().unwrap().read(&handles[2]));
            assert!(handles[2].will_not_be_locked());
            scope.spawn(|| lock.read().unwrap().read(&handles[3]));
            assert!(handles[3].will_not_be_locked());
            handles[1].release();
            assert!(handles[2].will_be_locked());
            assert!(handles[3].will_be_locked());
            scope.spawn(|| lock.write().unwrap().write(&handles[4]));
            assert!(handles[4].will_not_be_locked());
            scope.spawn(|| lock.write().unwrap().write(&handles[5]));
            assert!(handles[5].will_not_be_locked());
            handles[2].release();
            assert!(handles[4].will_not_be_locked());
            assert!(handles[5].will_not_be_locked());
            handles[3].release();
            assert!(handles[4].will_be_locked());
            assert!(handles[5].will_not_be_locked());
            handles[4].release();
            assert!(handles[5].will_be_locked());
            handles[5].release();
        });
    });
}

#[test]
fn poisoned_not_on_read() {
    assert_requires_unwind!();

    let lock = RwLock::new(());

    thread::scope(|scope| {
        suppress_panic_message(|| {
            thread::Builder::new()
                .name("Panicked reader".to_string())
                .spawn_scoped(scope, || {
                    let guard = lock.read().unwrap();
                    black_box(|| -> () { panic!() })();
                    drop(guard);
                })
                .unwrap()
                .join()
        })
        .expect_err("Spawned thread must panic");

        assert!(
            !lock.is_poisoned(),
            "Panicking during a `read` must not poison the `lock`."
        );

        assert_eq!(*lock.read().unwrap(), ());
        assert_eq!(*lock.write().unwrap(), ());

        assert_eq!(*lock.try_read().unwrap(), ());
        assert_eq!(*lock.try_write().unwrap(), ());
    });
}

#[test]
fn poisoned_on_write() {
    assert_requires_unwind!();

    let lock = RwLock::new(());

    thread::scope(|scope| {
        suppress_panic_message(|| {
            thread::Builder::new()
                .name("Panicked writer".to_string())
                .spawn_scoped(scope, || {
                    let guard = lock.write().unwrap();
                    black_box(|| -> () { panic!() })();
                    drop(guard);
                })
                .unwrap()
                .join()
        })
        .expect_err("Spawned thread must panic");

        assert!(
            lock.is_poisoned(),
            "Panicking during a `write` must poison the `lock`."
        );

        assert_eq!(*lock.read().unwrap_err().into_inner(), ());
        assert_eq!(*lock.write().unwrap_err().into_inner(), ());
        if let Err(TryLockError::Poisoned(poison)) = lock.try_read() {
            assert_eq!(*poison.into_inner(), ());
        } else {
            panic!("`lock` must be poisoned");
        }

        if let Err(TryLockError::Poisoned(poison)) = lock.try_write() {
            assert_eq!(*poison.into_inner(), ());
        } else {
            panic!("`lock` must be poisoned");
        }

        lock.clear_poison();
        assert!(!lock.is_poisoned());

        assert_eq!(*lock.read().unwrap(), ());
        assert_eq!(*lock.write().unwrap(), ());

        assert_eq!(*lock.try_read().unwrap(), ());
        assert_eq!(*lock.try_write().unwrap(), ());
    })
}

fn broken_always_allow(entries: StrategyInput) -> StrategyResult {
    Box::new(entries.map(|_| State::Ok))
}

fn broken_block_on_second(entries: StrategyInput) -> StrategyResult {
    let len = entries.count();
    let state = if len >= 2 { State::Blocked } else { State::Ok };
    Box::new(iter::repeat_n(state, len))
}

#[test]
fn broken_strategy_one_read() {
    let broken_lock = RwLock::new_strategied((), Box::new(broken_always_allow));
    try_strategy::<String, _>(
        &broken_lock,
        &[
            Try(Method::Read, Ok(())),
            UnlockAll,
            Try(Method::Read, Ok(())),
            UnlockAll,
        ],
    );
}

#[test]
fn broken_strategy_one_write() {
    try_strategy::<String, _>(
        &RwLock::new_strategied((), Box::new(broken_always_allow)),
        &[
            Try(Method::Write, Ok(())),
            UnlockAll,
            Try(Method::Write, Ok(())),
            UnlockAll,
        ],
    );
}

#[test]
fn broken_strategy_sequential_read_then_write() {
    try_strategy::<String, _>(
        &RwLock::new_strategied((), Box::new(broken_always_allow)),
        &[
            Try(Method::Read, Ok(())),
            UnlockAll,
            Try(Method::Write, Ok(())),
            UnlockAll,
            Try(Method::Read, Ok(())),
            Try(Method::Read, Ok(())),
            UnlockAll,
            Try(Method::Write, Ok(())),
            UnlockAll,
        ],
    );
}

#[test]
fn broken_strategy_multiple_reads() {
    try_strategy::<String, _>(
        &RwLock::new_strategied((), Box::new(broken_always_allow)),
        &[
            Try(Method::Read, Ok(())),
            UnlockAll,
            Try(Method::Read, Ok(())),
            Try(Method::Read, Ok(())),
            UnlockAll,
            Try(Method::Read, Ok(())),
            Try(Method::Read, Ok(())),
            Try(Method::Read, Ok(())),
            UnlockAll,
        ],
    );
}

#[test]
fn broken_strategy_read_then_write() {
    assert_requires_unwind!();

    // Although the strategy here is to test `RwLock` as a black-box, this private
    // white-box type is only used to fetch the actual error message we expect. It does couple
    // the test to white-box details, but saves verbosity in writing out the error message, which
    // is not important to test.
    let expected_message = StrategyLogicError::ConcurrentReadAndWrite.to_string();

    try_strategy(
        &RwLock::new_strategied((), Box::new(broken_always_allow)),
        &[
            Try(Method::Read, Ok(())),
            Try(Method::Write, Err(expected_message.clone())),
            UnlockAll,
        ],
    );

    try_strategy(
        &RwLock::new_strategied((), Box::new(broken_always_allow)),
        &[
            Try(Method::Read, Ok(())),
            Try(Method::Read, Ok(())),
            Try(Method::Write, Err(expected_message.clone())),
            UnlockAll,
        ],
    );
}

#[test]
fn broken_strategy_write_then_read() {
    assert_requires_unwind!();

    let expected_message = StrategyLogicError::ConcurrentReadAndWrite.to_string();

    try_strategy(
        &RwLock::new_strategied((), Box::new(broken_always_allow)),
        &[
            Try(Method::Write, Ok(())),
            Try(Method::Read, Err(expected_message.clone())),
            UnlockAll,
        ],
    );
}

#[test]
fn broken_strategy_two_writes() {
    assert_requires_unwind!();

    let expected_message = StrategyLogicError::ConcurrentMultipleWrites.to_string();

    try_strategy(
        &RwLock::new_strategied((), Box::new(broken_always_allow)),
        &[
            Try(Method::Write, Ok(())),
            Try(Method::Write, Err(expected_message.clone())),
            UnlockAll,
        ],
    );
}

#[test]
fn broken_strategy_ok_then_blocked() {
    assert_requires_unwind!();

    let expected_message = StrategyLogicError::BlockedAfterOkState.to_string();

    try_strategy(
        &RwLock::new_strategied((), Box::new(broken_block_on_second)),
        &[
            Try(Method::Read, Ok(())),
            Try(Method::Read, Err(expected_message.clone())),
            UnlockAll,
        ],
    );

    try_strategy(
        &RwLock::new_strategied((), Box::new(broken_block_on_second)),
        &[
            Try(Method::Read, Ok(())),
            Try(Method::Write, Err(expected_message.clone())),
            UnlockAll,
        ],
    );

    try_strategy(
        &RwLock::new_strategied((), Box::new(broken_block_on_second)),
        &[
            Try(Method::Write, Ok(())),
            Try(Method::Read, Err(expected_message.clone())),
            UnlockAll,
        ],
    );

    try_strategy(
        &RwLock::new_strategied((), Box::new(broken_block_on_second)),
        &[
            Try(Method::Write, Ok(())),
            Try(Method::Write, Err(expected_message.clone())),
            UnlockAll,
        ],
    );
}

#[test]
fn broken_strategy_try_after_broken() {
    let broken_message = StrategyLogicError::BrokenLock.to_string();

    // Try more after breakage
    try_strategy(
        &RwLock::new_strategied((), Box::new(broken_always_allow)),
        &[
            Try(Method::Read, Ok(())),
            Try(
                Method::Write,
                Err(StrategyLogicError::ConcurrentReadAndWrite.to_string()),
            ),
            Try(Method::Read, Err(broken_message.clone())),
            UnlockAll,
            Try(Method::Read, Err(broken_message.clone())),
            Try(Method::Read, Err(broken_message.clone())),
            UnlockAll,
        ],
    );

    try_strategy(
        &RwLock::new_strategied((), Box::new(broken_always_allow)),
        &[
            Try(Method::Read, Ok(())),
            Try(Method::Read, Ok(())),
            Try(
                Method::Write,
                Err(StrategyLogicError::ConcurrentReadAndWrite.to_string()),
            ),
            Try(Method::Write, Err(broken_message.clone())),
            UnlockAll,
            Try(Method::Write, Err(broken_message.clone())),
            Try(Method::Write, Err(broken_message.clone())),
            Try(Method::Write, Err(broken_message.clone())),
            UnlockAll,
        ],
    );

    try_strategy(
        &RwLock::new_strategied((), Box::new(broken_always_allow)),
        &[
            Try(Method::Write, Ok(())),
            Try(
                Method::Read,
                Err(StrategyLogicError::ConcurrentReadAndWrite.to_string()),
            ),
            Try(Method::Write, Err(broken_message.clone())),
            Try(Method::Read, Err(broken_message.clone())),
            UnlockAll,
            Try(Method::Read, Err(broken_message.clone())),
            Try(Method::Write, Err(broken_message.clone())),
            UnlockAll,
        ],
    );

    try_strategy(
        &RwLock::new_strategied((), Box::new(broken_always_allow)),
        &[
            Try(Method::Write, Ok(())),
            Try(
                Method::Write,
                Err(StrategyLogicError::ConcurrentMultipleWrites.to_string()),
            ),
            Try(Method::Read, Err(broken_message.clone())),
            Try(Method::Write, Err(broken_message.clone())),
            UnlockAll,
            Try(Method::Write, Err(broken_message.clone())),
            Try(Method::Read, Err(broken_message.clone())),
            UnlockAll,
        ],
    );

    try_strategy(
        &RwLock::new_strategied((), Box::new(broken_block_on_second)),
        &[
            Try(Method::Read, Ok(())),
            Try(
                Method::Read,
                Err(StrategyLogicError::BlockedAfterOkState.to_string()),
            ),
            Try(Method::Read, Err(broken_message.clone())),
            Try(Method::Write, Err(broken_message.clone())),
            Try(Method::Write, Err(broken_message.clone())),
            UnlockAll,
            Try(Method::Write, Err(broken_message.clone())),
            Try(Method::Read, Err(broken_message.clone())),
            Try(Method::Read, Err(broken_message.clone())),
            Try(Method::Write, Err(broken_message.clone())),
            Try(Method::Read, Err(broken_message.clone())),
            UnlockAll,
            Try(Method::Read, Err(broken_message.clone())),
            UnlockAll,
        ],
    );
}

fn load_test_with(mut lock: RwLock<usize>) {
    *lock.get_mut().unwrap() = 0usize;

    const THREADS: usize = if cfg!(miri) { 3 } else { 16 };
    const WRITES: usize = if cfg!(miri) { 3 } else { 256 };
    const READS: usize = if cfg!(miri) { 12 } else { 2048 };

    thread::scope(|scope| {
        for t in 0..THREADS {
            let lock_ref = &lock;
            thread::Builder::new()
                .name(format!("load thread number {}", t + 1))
                .spawn_scoped(scope, move || {
                    #[derive(Clone, Copy)]
                    enum TestActions {
                        Read,
                        Write,
                    }

                    let permute = || {
                        let mut rng = fastrand::Rng::with_seed(u64::try_from(t).unwrap());

                        let mut actions = vec![];
                        actions.append(&mut vec![TestActions::Write; WRITES / 2]);
                        actions.append(&mut vec![TestActions::Read; READS / 2]);

                        rng.shuffle(&mut *actions);

                        for action in actions {
                            match action {
                                TestActions::Read => {
                                    let guard = lock_ref.read().unwrap();
                                    black_box(*guard);
                                    drop(guard);
                                }
                                TestActions::Write => {
                                    let mut guard = lock_ref.write().unwrap();
                                    *guard ^= rng.usize(0..usize::MAX);
                                    drop(guard);
                                }
                            }
                        }
                    };

                    permute();
                    permute();
                })
                .unwrap();
        }
    });

    assert_eq!(*lock.read().unwrap(), 0);
    assert_eq!(*lock.write().unwrap(), 0);
    assert_eq!(*lock.get_mut().unwrap(), 0);
    assert_eq!(lock.into_inner().unwrap(), 0);
}

#[test]
fn load_test() {
    let num = RwLock::new_strategied(0usize, Box::new(strategies::fair));
    load_test_with(num);
}
