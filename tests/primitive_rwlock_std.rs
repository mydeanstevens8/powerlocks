#![cfg(all(feature = "rwlock", feature = "std"))]

use std::{
    cell::UnsafeCell,
    panic::{RefUnwindSafe, UnwindSafe},
};

use powerlocks::primitive_rwlock::{StdRwLock, StdRwLockReadGuard, StdRwLockWriteGuard};

mod rwlock_utils;
use rwlock_utils::tests;

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

    assert_is_trait!(StdRwLockReadGuard<'_, ()>, Send, Sync);
    assert_is_trait!(StdRwLockReadGuard<'_, ()>, UnwindSafe, RefUnwindSafe, Unpin);

    assert_is_trait!(StdRwLockReadGuard<'_, i32>, Send, Sync);
    assert_is_trait!(StdRwLockReadGuard<'_, i32>, UnwindSafe, RefUnwindSafe);
    assert_is_trait!(StdRwLockReadGuard<'_, i32>, Unpin);

    assert_is_trait!(StdRwLockReadGuard<'_, UnsafeCell<i32>>, Send);
    assert_is_trait!(StdRwLockReadGuard<'_, UnsafeCell<i32>>, !Sync);
    assert_is_trait!(StdRwLockReadGuard<'_, *const ()>, !Send, !Sync);
    assert_is_trait!(
        StdRwLockReadGuard<'_, std::sync::RwLockReadGuard<'_, i32>>,
        !Send
    );
    assert_is_trait!(
        StdRwLockReadGuard<'_, std::sync::RwLockReadGuard<'_, i32>>,
        Sync
    );

    assert_is_trait!(StdRwLockWriteGuard<'_, i32>, Send, Sync);
    assert_is_trait!(StdRwLockWriteGuard<'_, i32>, UnwindSafe, RefUnwindSafe);
    assert_is_trait!(StdRwLockWriteGuard<'_, i32>, Unpin);

    assert_is_trait!(StdRwLockWriteGuard<'_, UnsafeCell<i32>>, Send);
    assert_is_trait!(StdRwLockWriteGuard<'_, UnsafeCell<i32>>, !Sync);
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
    tests::run_single_thread::<StdRwLock<_>, ()>();
    tests::run_single_thread::<StdRwLock<_>, bool>();
    tests::run_single_thread::<StdRwLock<_>, i32>();
    tests::run_single_thread::<StdRwLock<_>, usize>();
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
    tests::race_reads(&StdRwLock::new(RaceChecker::new()));
}

#[test]
fn race_writes() {
    tests::race_writes(&StdRwLock::new(RaceChecker::new()));
}

#[test]
fn no_poison_on_read() {
    tests::no_poison_on_read(&StdRwLock::new(()));
}

#[test]
fn poison_on_write() {
    tests::poison_on_write(&StdRwLock::new(()));
}

#[test]
fn load_test() {
    const THREADS: usize = if cfg!(miri) { 3 } else { 16 };
    const WRITES: usize = if cfg!(miri) { 3 } else { 256 };
    const READS: usize = if cfg!(miri) { 12 } else { 2048 };

    let num = StdRwLock::new(0usize);
    tests::load_test_with(num, THREADS, WRITES, READS);
}
