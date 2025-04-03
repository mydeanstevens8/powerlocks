#![cfg(all(feature = "rwlock", feature = "std"))]

use std::{
    cell::UnsafeCell,
    panic::{RefUnwindSafe, UnwindSafe},
};

use powerlocks::rwlock::{Method, StdRwLock, StdRwLockReadGuard, StdRwLockWriteGuard, strategies};

mod rwlock_utils;
use rwlock_utils::{
    StrategyLogicError,
    TryStrategyAttempt::{Try, UnlockAll},
    load_test_with, strategies as test_strategies, try_strategy,
};

mod utils;
use utils::{assert_is_trait, race_checker::RaceChecker};

#[test]
fn assert_trait() {
    assert_is_trait!(StdRwLock<()>, Send, Sync);
    assert_is_trait!(StdRwLock<bool>, Send, Sync);
    assert_is_trait!(StdRwLock<i32>, Send, Sync);
    assert_is_trait!(StdRwLock<usize>, Send, Sync);
    assert_is_trait!(StdRwLock<isize>, Send, Sync);

    assert_is_trait!(StdRwLock<()>, UnwindSafe, RefUnwindSafe, Unpin);
    assert_is_trait!(StdRwLock<i32>, UnwindSafe, RefUnwindSafe, Unpin);

    assert_is_trait!(UnsafeCell<i32>, Send);
    assert_is_trait!(UnsafeCell<i32>, !Sync);
    assert_is_trait!(StdRwLock<UnsafeCell<i32>>, Send);
    assert_is_trait!(StdRwLock<UnsafeCell<i32>>, !Sync);
    assert_is_trait!(StdRwLock<UnsafeCell<i32>>, UnwindSafe, RefUnwindSafe);
    assert_is_trait!(StdRwLock<UnsafeCell<i32>>, Unpin);

    assert_is_trait!(std::sync::RwLockReadGuard<'_, i32>, !Send);
    assert_is_trait!(std::sync::RwLockReadGuard<'_, i32>, Sync);
    assert_is_trait!(StdRwLock<std::sync::RwLockReadGuard<'_, i32>>, !Send, !Sync);
    assert_is_trait!(
        StdRwLock<std::sync::RwLockReadGuard<'_, i32>>,
        UnwindSafe,
        RefUnwindSafe,
        Unpin
    );

    assert_is_trait!(*const (), !Send, !Sync);
    assert_is_trait!(StdRwLock<*const ()>, !Send, !Sync);
    assert_is_trait!(StdRwLock<*const ()>, UnwindSafe, RefUnwindSafe, Unpin);

    assert_is_trait!(*mut (), !Send, !Sync);
    assert_is_trait!(StdRwLock<*mut ()>, !Send, !Sync);
    assert_is_trait!(StdRwLock<*mut ()>, UnwindSafe, RefUnwindSafe, Unpin);

    assert_is_trait!(StdRwLockReadGuard<'_, ()>, !Send);
    assert_is_trait!(StdRwLockReadGuard<'_, ()>, Sync);
    assert_is_trait!(StdRwLockReadGuard<'_, ()>, UnwindSafe, RefUnwindSafe, Unpin);

    assert_is_trait!(StdRwLockReadGuard<'_, i32>, !Send);
    assert_is_trait!(StdRwLockReadGuard<'_, i32>, Sync);
    assert_is_trait!(StdRwLockReadGuard<'_, i32>, UnwindSafe, RefUnwindSafe);
    assert_is_trait!(StdRwLockReadGuard<'_, i32>, Unpin);

    assert_is_trait!(StdRwLockReadGuard<'_, UnsafeCell<i32>>, !Send, !Sync);
    assert_is_trait!(StdRwLockReadGuard<'_, *const ()>, !Send, !Sync);
    assert_is_trait!(
        StdRwLockReadGuard<'_, std::sync::RwLockReadGuard<'_, i32>>,
        !Send
    );
    assert_is_trait!(
        StdRwLockReadGuard<'_, std::sync::RwLockReadGuard<'_, i32>>,
        Sync
    );

    assert_is_trait!(StdRwLockWriteGuard<'_, i32>, !Send);
    assert_is_trait!(StdRwLockWriteGuard<'_, i32>, Sync);
    assert_is_trait!(StdRwLockWriteGuard<'_, i32>, UnwindSafe, RefUnwindSafe);
    assert_is_trait!(StdRwLockWriteGuard<'_, i32>, Unpin);

    assert_is_trait!(StdRwLockWriteGuard<'_, UnsafeCell<i32>>, !Send, !Sync);
    assert_is_trait!(StdRwLockWriteGuard<'_, *const ()>, !Send, !Sync);
    assert_is_trait!(
        StdRwLockWriteGuard<'_, std::sync::RwLockReadGuard<'_, i32>>,
        !Send
    );
    assert_is_trait!(
        StdRwLockWriteGuard<'_, std::sync::RwLockReadGuard<'_, i32>>,
        Sync
    );
}

#[test]
fn run_single_thread() {
    rwlock_utils::run_single_thread::<StdRwLock<_>, ()>();
    rwlock_utils::run_single_thread::<StdRwLock<_>, bool>();
    rwlock_utils::run_single_thread::<StdRwLock<_>, i32>();
    rwlock_utils::run_single_thread::<StdRwLock<_>, usize>();
}

#[test]
fn run_single_thread_vec() {
    let locked_vec = StdRwLock::new(vec![1, 2, 3, 4, 5]);

    locked_vec.write().unwrap().push(6);
    assert_eq!(*locked_vec.read().unwrap(), [1, 2, 3, 4, 5, 6]);

    assert_eq!(locked_vec.write().unwrap().pop().unwrap(), 6);
    assert_eq!(*locked_vec.read().unwrap(), [1, 2, 3, 4, 5]);

    assert_eq!(locked_vec.write().unwrap().pop().unwrap(), 5);
    assert_eq!(*locked_vec.read().unwrap(), [1, 2, 3, 4]);
}

#[test]
fn race_reads() {
    rwlock_utils::race_reads(&StdRwLock::new_strategied(
        RaceChecker::new(),
        Box::new(strategies::fair),
    ));
}

#[test]
fn race_writes() {
    rwlock_utils::race_writes(&StdRwLock::new_strategied(
        RaceChecker::new(),
        Box::new(strategies::fair),
    ));
}

#[test]
fn race_fair_writes_and_reads() {
    rwlock_utils::race_fair_writes_and_reads(&StdRwLock::new_strategied(
        RaceChecker::new(),
        Box::new(strategies::fair),
    ));
}

#[test]
fn no_poison_on_read() {
    rwlock_utils::no_poison_on_read(&StdRwLock::new(()));
}

#[test]
fn poison_on_write() {
    rwlock_utils::poison_on_write(&StdRwLock::new(()));
}

#[test]
fn broken_strategy_one_read() {
    try_strategy::<String, _>(
        &StdRwLock::new_strategied((), Box::new(test_strategies::broken_always_allow)),
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
        &StdRwLock::new_strategied((), Box::new(test_strategies::broken_always_allow)),
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
        &StdRwLock::new_strategied((), Box::new(test_strategies::broken_always_allow)),
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
        &StdRwLock::new_strategied((), Box::new(test_strategies::broken_always_allow)),
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
    // Although the strategy here is to test `StdRwLock` as a black-box, this private
    // white-box type is only used to fetch the actual error message we expect. It does couple
    // the test to white-box details, but saves verbosity in writing out the error message, which
    // is not important to test.
    let expected_message = StrategyLogicError::ConcurrentReadAndWrite.to_string();

    try_strategy(
        &StdRwLock::new_strategied((), Box::new(test_strategies::broken_always_allow)),
        &[
            Try(Method::Read, Ok(())),
            Try(Method::Write, Err(expected_message.clone())),
            UnlockAll,
        ],
    );

    try_strategy(
        &StdRwLock::new_strategied((), Box::new(test_strategies::broken_always_allow)),
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
    let expected_message = StrategyLogicError::ConcurrentReadAndWrite.to_string();

    try_strategy(
        &StdRwLock::new_strategied((), Box::new(test_strategies::broken_always_allow)),
        &[
            Try(Method::Write, Ok(())),
            Try(Method::Read, Err(expected_message.clone())),
            UnlockAll,
        ],
    );
}

#[test]
fn broken_strategy_two_writes() {
    let expected_message = StrategyLogicError::ConcurrentMultipleWrites.to_string();

    try_strategy(
        &StdRwLock::new_strategied((), Box::new(test_strategies::broken_always_allow)),
        &[
            Try(Method::Write, Ok(())),
            Try(Method::Write, Err(expected_message.clone())),
            UnlockAll,
        ],
    );
}

#[test]
fn broken_strategy_ok_then_blocked() {
    let expected_message = StrategyLogicError::BlockedAfterOkState.to_string();

    try_strategy(
        &StdRwLock::new_strategied((), Box::new(test_strategies::broken_block_on_second)),
        &[
            Try(Method::Read, Ok(())),
            Try(Method::Read, Err(expected_message.clone())),
            UnlockAll,
        ],
    );

    try_strategy(
        &StdRwLock::new_strategied((), Box::new(test_strategies::broken_block_on_second)),
        &[
            Try(Method::Read, Ok(())),
            Try(Method::Write, Err(expected_message.clone())),
            UnlockAll,
        ],
    );

    try_strategy(
        &StdRwLock::new_strategied((), Box::new(test_strategies::broken_block_on_second)),
        &[
            Try(Method::Write, Ok(())),
            Try(Method::Read, Err(expected_message.clone())),
            UnlockAll,
        ],
    );

    try_strategy(
        &StdRwLock::new_strategied((), Box::new(test_strategies::broken_block_on_second)),
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
        &StdRwLock::new_strategied((), Box::new(test_strategies::broken_always_allow)),
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
        &StdRwLock::new_strategied((), Box::new(test_strategies::broken_always_allow)),
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
        &StdRwLock::new_strategied((), Box::new(test_strategies::broken_always_allow)),
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
        &StdRwLock::new_strategied((), Box::new(test_strategies::broken_always_allow)),
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
        &StdRwLock::new_strategied((), Box::new(test_strategies::broken_block_on_second)),
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

#[test]
fn load_test() {
    const THREADS: usize = if cfg!(miri) { 3 } else { 16 };
    const WRITES: usize = if cfg!(miri) { 3 } else { 256 };
    const READS: usize = if cfg!(miri) { 12 } else { 2048 };

    let num = StdRwLock::new_strategied(0usize, Box::new(strategies::fair));
    load_test_with(num, THREADS, WRITES, READS);
}
