#![cfg(all(feature = "mutex", feature = "std"))]

mod mutex_utils;
mod utils;

use std::{
    cell::UnsafeCell,
    panic::{RefUnwindSafe, UnwindSafe},
    sync::{Mutex, MutexGuard},
};

use mutex_utils::tests;

#[test]
fn assert_trait() {
    use utils::assert_is_trait;

    assert_is_trait!(Mutex<()>, Send, Sync, UnwindSafe, RefUnwindSafe, Unpin);
    assert_is_trait!(Mutex<i32>, Send, Sync, UnwindSafe, RefUnwindSafe, Unpin);
    assert_is_trait!(Mutex<bool>, Send, Sync, UnwindSafe, RefUnwindSafe, Unpin);
    assert_is_trait!(Mutex<u64>, Send, Sync, UnwindSafe, RefUnwindSafe, Unpin);
    assert_is_trait!(Mutex<i64>, Send, Sync, UnwindSafe, RefUnwindSafe, Unpin);

    assert_is_trait!(UnsafeCell<i32>, Send);
    assert_is_trait!(UnsafeCell<i32>, !Sync);
    assert_is_trait!(Mutex<UnsafeCell<i32>>, Send, Sync);
    assert_is_trait!(Mutex<UnsafeCell<i32>>, UnwindSafe, RefUnwindSafe, Unpin);

    assert_is_trait!(*const (), !Send, !Sync);
    assert_is_trait!(Mutex<*const ()>, !Send, !Sync);
    assert_is_trait!(Mutex<*const ()>, UnwindSafe, RefUnwindSafe, Unpin);

    assert_is_trait!(*mut (), !Send, !Sync);
    assert_is_trait!(Mutex<*mut ()>, !Send, !Sync);
    assert_is_trait!(Mutex<*mut ()>, UnwindSafe, RefUnwindSafe, Unpin);

    assert_is_trait!(MutexGuard<'_, ()>, !Send);
    assert_is_trait!(MutexGuard<'_, ()>, Sync);
    assert_is_trait!(MutexGuard<'_, i32>, !Send);
    assert_is_trait!(MutexGuard<'_, i32>, Sync);
    assert_is_trait!(MutexGuard<'_, ()>, UnwindSafe, RefUnwindSafe, Unpin);
    assert_is_trait!(MutexGuard<'_, i32>, UnwindSafe, RefUnwindSafe, Unpin);

    assert_is_trait!(MutexGuard<'_, UnsafeCell<i32>>, !Send, !Sync);
    assert_is_trait!(MutexGuard<'_, *const ()>, !Send, !Sync);
}

#[test]
fn lock() {
    tests::lock::<Mutex<_>, _>(&());
    tests::lock::<Mutex<_>, _>(&false);
    tests::lock::<Mutex<_>, _>(&0_u8);
    tests::lock::<Mutex<_>, _>(&0_u16);
    tests::lock::<Mutex<_>, _>(&0_u32);
    tests::lock::<Mutex<_>, _>(&0_u64);

    tests::lock_writing::<Mutex<_>, _>(&0_u8, 0xcb);
    tests::lock_writing::<Mutex<_>, _>(&0_u16, 0x47a2);
    tests::lock_writing::<Mutex<_>, _>(&0_u32, 0xac7e4d30);
    tests::lock_writing::<Mutex<_>, _>(&0_u64, 0xac7e4d30_951f268b);

    let array_i32 = [1, 2, 3, 4, 5];
    let unsized_lock: &mut Mutex<[i32]> = &mut Mutex::new(array_i32);
    tests::lock_unsized(unsized_lock, &array_i32);
}

#[test]
fn race_lock() {
    tests::race_lock::<Mutex<_>>();
}

#[test]
fn poison() {
    tests::poison::<Mutex<_>, _>(&(), true);
    tests::poison::<Mutex<_>, _>(&0_u64, true);
}

#[test]
fn try_lock() {
    tests::try_lock::<Mutex<_>, _>(&());
    tests::try_lock::<Mutex<_>, _>(&0_u64);
}

#[test]
fn load_test() {
    const THREADS: usize = if cfg!(miri) { 8 } else { 8 };
    const REPS: usize = if cfg!(miri) { 32 } else { 16384 };
    const CYCLES: usize = if cfg!(miri) { 8 } else { 64 };

    tests::do_load_test::<Mutex<_>>(THREADS, REPS, CYCLES, None);
}

#[test]
fn poisoning_load_test() {
    const THREADS: usize = if cfg!(miri) { 8 } else { 8 };
    const REPS: usize = if cfg!(miri) { 16 } else { 16384 };
    const CYCLES: usize = if cfg!(miri) { 8 } else { 64 };
    const POISONING_REPS: usize = if cfg!(miri) { 4 } else { 64 };
    mutex_utils::suppress_panic_message(|| {
        tests::do_load_test::<Mutex<_>>(THREADS, REPS, CYCLES, Some(POISONING_REPS))
    });
}

#[test]
#[ignore = "This is a benchmark test that takes a long time to run."]
fn extended_load_test() {
    const THREADS: usize = if cfg!(miri) { 8 } else { 8 };
    const REPS: usize = if cfg!(miri) { 32 } else { 262144 };
    const CYCLES: usize = if cfg!(miri) { 16 } else { 128 };

    tests::do_load_test::<Mutex<_>>(THREADS, REPS, CYCLES, None);
}
