#![allow(dead_code, reason = "Some functions are not used by importing tests.")]

use super::{
    strategies, suppress_panic_message,
    try_strategy::{
        StrategyLogicError,
        TryStrategyAttempt::{Try, UnlockAll},
        try_strategy,
    },
};
use crate::utils::race_checker::{CheckerHandles, RaceChecker};
use powerlocks::{
    primitives::TryLockError,
    rwlock::{Method, RwLockApi, StrategiedRwLockApi},
};
use std::{fmt::Debug, hint::black_box, thread};

pub fn run_single_thread<A: RwLockApi<T>, T: Debug + Default + PartialEq>() {
    let locked_unit = A::new(T::default());
    let default_t = T::default();

    assert_eq!(*locked_unit.read().unwrap(), default_t);
    assert_eq!(*locked_unit.write().unwrap(), default_t);
    assert_eq!(*locked_unit.read().unwrap(), default_t);
    assert_eq!(*locked_unit.write().unwrap(), default_t);
}

pub fn race_reads<A: RwLockApi<RaceChecker> + Sync>(lock: &A) {
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

pub fn race_writes<A: RwLockApi<RaceChecker> + Sync>(lock: &A) {
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

pub fn race_fair_writes_and_reads<A: RwLockApi<RaceChecker> + Sync>(lock: &A) {
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

pub fn no_poison_on_read<A: RwLockApi<()> + Sync>(lock: &A) {
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

pub fn poison_on_write<A: RwLockApi<()> + Sync>(lock: &A) {
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

        assert_eq!(*lock.read().err().unwrap().into_inner(), ());
        assert_eq!(*lock.write().err().unwrap().into_inner(), ());
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

pub fn broken_strategy_one_read<A: StrategiedRwLockApi<T> + Sync, T: Default + Sync>() {
    try_strategy::<String, _>(
        &A::new_strategied(T::default(), Box::new(strategies::broken_always_allow)),
        &[
            Try(Method::Read, Ok(())),
            UnlockAll,
            Try(Method::Read, Ok(())),
            UnlockAll,
        ],
    );
}

pub fn broken_strategy_one_write<A: StrategiedRwLockApi<T> + Sync, T: Default + Sync>() {
    try_strategy::<String, _>(
        &A::new_strategied(T::default(), Box::new(strategies::broken_always_allow)),
        &[
            Try(Method::Write, Ok(())),
            UnlockAll,
            Try(Method::Write, Ok(())),
            UnlockAll,
        ],
    );
}

pub fn broken_strategy_sequential_read_then_write<
    A: StrategiedRwLockApi<T> + Sync,
    T: Default + Sync,
>() {
    try_strategy::<String, _>(
        &A::new_strategied(T::default(), Box::new(strategies::broken_always_allow)),
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

pub fn broken_strategy_multiple_reads<A: StrategiedRwLockApi<T> + Sync, T: Default + Sync>() {
    try_strategy::<String, _>(
        &A::new_strategied(T::default(), Box::new(strategies::broken_always_allow)),
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

pub fn broken_strategy_read_then_write<A: StrategiedRwLockApi<T> + Sync, T: Default + Sync>() {
    // Although the strategy here is to test `StdRwLock` as a black-box, this private
    // white-box type is only used to fetch the actual error message we expect. It does couple
    // the test to white-box details, but saves verbosity in writing out the error message, which
    // is not important to test.
    let expected_message = StrategyLogicError::ConcurrentReadAndWrite.to_string();

    try_strategy(
        &A::new_strategied(T::default(), Box::new(strategies::broken_always_allow)),
        &[
            Try(Method::Read, Ok(())),
            Try(Method::Write, Err(expected_message.clone())),
            UnlockAll,
        ],
    );

    try_strategy(
        &A::new_strategied(T::default(), Box::new(strategies::broken_always_allow)),
        &[
            Try(Method::Read, Ok(())),
            Try(Method::Read, Ok(())),
            Try(Method::Write, Err(expected_message.clone())),
            UnlockAll,
        ],
    );
}

pub fn broken_strategy_write_then_read<A: StrategiedRwLockApi<T> + Sync, T: Default + Sync>() {
    let expected_message = StrategyLogicError::ConcurrentReadAndWrite.to_string();

    try_strategy(
        &A::new_strategied(T::default(), Box::new(strategies::broken_always_allow)),
        &[
            Try(Method::Write, Ok(())),
            Try(Method::Read, Err(expected_message.clone())),
            UnlockAll,
        ],
    );
}

pub fn broken_strategy_two_writes<A: StrategiedRwLockApi<T> + Sync, T: Default + Sync>() {
    let expected_message = StrategyLogicError::ConcurrentMultipleWrites.to_string();

    try_strategy(
        &A::new_strategied(T::default(), Box::new(strategies::broken_always_allow)),
        &[
            Try(Method::Write, Ok(())),
            Try(Method::Write, Err(expected_message.clone())),
            UnlockAll,
        ],
    );
}

pub fn broken_strategy_ok_then_blocked<A: StrategiedRwLockApi<T> + Sync, T: Default + Sync>() {
    let expected_message = StrategyLogicError::BlockedAfterOkState.to_string();

    try_strategy(
        &A::new_strategied(T::default(), Box::new(strategies::broken_block_on_second)),
        &[
            Try(Method::Read, Ok(())),
            Try(Method::Read, Err(expected_message.clone())),
            UnlockAll,
        ],
    );

    try_strategy(
        &A::new_strategied(T::default(), Box::new(strategies::broken_block_on_second)),
        &[
            Try(Method::Read, Ok(())),
            Try(Method::Write, Err(expected_message.clone())),
            UnlockAll,
        ],
    );

    try_strategy(
        &A::new_strategied(T::default(), Box::new(strategies::broken_block_on_second)),
        &[
            Try(Method::Write, Ok(())),
            Try(Method::Read, Err(expected_message.clone())),
            UnlockAll,
        ],
    );

    try_strategy(
        &A::new_strategied(T::default(), Box::new(strategies::broken_block_on_second)),
        &[
            Try(Method::Write, Ok(())),
            Try(Method::Write, Err(expected_message.clone())),
            UnlockAll,
        ],
    );
}

pub fn broken_strategy_try_after_broken<A: StrategiedRwLockApi<T> + Sync, T: Default + Sync>() {
    let broken_message = StrategyLogicError::BrokenLock.to_string();

    // Try more after breakage
    try_strategy(
        &A::new_strategied(T::default(), Box::new(strategies::broken_always_allow)),
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
        &A::new_strategied(T::default(), Box::new(strategies::broken_always_allow)),
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
        &A::new_strategied(T::default(), Box::new(strategies::broken_always_allow)),
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
        &A::new_strategied(T::default(), Box::new(strategies::broken_always_allow)),
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
        &A::new_strategied(T::default(), Box::new(strategies::broken_block_on_second)),
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

pub fn load_test_with<A: RwLockApi<usize> + Sync>(
    mut lock: A,
    threads: usize,
    writes: usize,
    reads: usize,
) {
    *lock.get_mut().unwrap() = 0_usize;
    thread::scope(|scope| {
        for t in 0..threads {
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
                        actions.append(&mut vec![TestActions::Write; writes / 2]);
                        actions.append(&mut vec![TestActions::Read; reads / 2]);

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
