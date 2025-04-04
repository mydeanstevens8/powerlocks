#![cfg(feature = "mutex")]

mod mutex_utils;
mod utils;

use std::{
    cell::UnsafeCell,
    panic::{RefUnwindSafe, UnwindSafe},
};

use powerlocks::mutex::{CoreMutex, CoreMutexGuard};

use mutex_utils::tests;

#[test]
fn assert_trait() {
    use utils::assert_is_trait;

    assert_is_trait!(CoreMutex<()>, Send, Sync, UnwindSafe, RefUnwindSafe, Unpin);
    assert_is_trait!(CoreMutex<i32>, Send, Sync, UnwindSafe, RefUnwindSafe, Unpin);
    assert_is_trait!(
        CoreMutex<bool>,
        Send,
        Sync,
        UnwindSafe,
        RefUnwindSafe,
        Unpin
    );
    assert_is_trait!(CoreMutex<u64>, Send, Sync, UnwindSafe, RefUnwindSafe, Unpin);
    assert_is_trait!(CoreMutex<i64>, Send, Sync, UnwindSafe, RefUnwindSafe, Unpin);

    assert_is_trait!(UnsafeCell<i32>, Send);
    assert_is_trait!(UnsafeCell<i32>, !Sync);
    assert_is_trait!(CoreMutex<UnsafeCell<i32>>, Send, Sync);
    assert_is_trait!(CoreMutex<UnsafeCell<i32>>, UnwindSafe, RefUnwindSafe, Unpin);

    assert_is_trait!(*const (), !Send, !Sync);
    assert_is_trait!(CoreMutex<*const ()>, !Send, !Sync);
    assert_is_trait!(CoreMutex<*const ()>, UnwindSafe, RefUnwindSafe, Unpin);

    assert_is_trait!(*mut (), !Send, !Sync);
    assert_is_trait!(CoreMutex<*mut ()>, !Send, !Sync);
    assert_is_trait!(CoreMutex<*mut ()>, UnwindSafe, RefUnwindSafe, Unpin);

    assert_is_trait!(CoreMutexGuard<'_, ()>, Send, Sync);
    assert_is_trait!(CoreMutexGuard<'_, i32>, Send, Sync);
    assert_is_trait!(CoreMutexGuard<'_, ()>, UnwindSafe, RefUnwindSafe, Unpin);
    assert_is_trait!(CoreMutexGuard<'_, i32>, UnwindSafe, RefUnwindSafe, Unpin);

    assert_is_trait!(CoreMutexGuard<'_, UnsafeCell<i32>>, Send);
    assert_is_trait!(CoreMutexGuard<'_, UnsafeCell<i32>>, !Sync);
    assert_is_trait!(CoreMutexGuard<'_, *const ()>, !Send, !Sync);
}

#[test]
fn lock() {
    tests::lock::<CoreMutex<_>, _>(&());
    tests::lock::<CoreMutex<_>, _>(&false);
    tests::lock::<CoreMutex<_>, _>(&0_u8);
    tests::lock::<CoreMutex<_>, _>(&0_u16);
    tests::lock::<CoreMutex<_>, _>(&0_u32);
    tests::lock::<CoreMutex<_>, _>(&0_u64);

    tests::lock_writing::<CoreMutex<_>, _>(&0_u8, 0xcb);
    tests::lock_writing::<CoreMutex<_>, _>(&0_u16, 0x47a2);
    tests::lock_writing::<CoreMutex<_>, _>(&0_u32, 0xac7e4d30);
    tests::lock_writing::<CoreMutex<_>, _>(&0_u64, 0xac7e4d30_951f268b);

    let array_i32 = [1, 2, 3, 4, 5];
    let unsized_lock: &mut CoreMutex<[i32]> = &mut CoreMutex::new(array_i32);
    tests::lock_unsized(unsized_lock, &array_i32);
}

#[test]
fn race_lock() {
    tests::race_lock::<CoreMutex<_>>();
}

#[test]
fn poison() {
    tests::poison::<CoreMutex<_>, _>(&(), false);
    tests::poison::<CoreMutex<_>, _>(&0_u64, false);
}

#[test]
fn try_lock() {
    tests::try_lock::<CoreMutex<_>, _>(&());
    tests::try_lock::<CoreMutex<_>, _>(&0_u64);
}

#[test]
fn load_test() {
    const THREADS: usize = if cfg!(miri) { 8 } else { 8 };
    const REPS: usize = if cfg!(miri) { 32 } else { 16384 };
    const CYCLES: usize = if cfg!(miri) { 8 } else { 64 };

    tests::do_load_test::<CoreMutex<_>>(THREADS, REPS, CYCLES, None);
}

#[test]
fn poisoning_load_test() {
    const THREADS: usize = if cfg!(miri) { 8 } else { 8 };
    const REPS: usize = if cfg!(miri) { 16 } else { 16384 };
    const CYCLES: usize = if cfg!(miri) { 8 } else { 64 };
    const POISONING_REPS: usize = if cfg!(miri) { 4 } else { 64 };
    mutex_utils::suppress_panic_message(|| {
        tests::do_load_test::<CoreMutex<_>>(THREADS, REPS, CYCLES, Some(POISONING_REPS))
    });
}

#[test]
#[ignore = "This is a benchmark test that takes a long time to run."]
fn extended_load_test() {
    const THREADS: usize = if cfg!(miri) { 8 } else { 8 };
    const REPS: usize = if cfg!(miri) { 32 } else { 262144 };
    const CYCLES: usize = if cfg!(miri) { 16 } else { 128 };

    tests::do_load_test::<CoreMutex<_>>(THREADS, REPS, CYCLES, None);
}
