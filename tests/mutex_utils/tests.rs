use powerlocks::{
    mutex::MutexApi,
    primitives::{PoisonError, TryLockError},
};

use std::{
    fmt::Debug,
    hint::black_box,
    ops::BitXorAssign,
    sync::atomic::{AtomicBool, Ordering},
    thread,
};

use crate::utils::race_checker::{CheckerHandles, RaceChecker};

pub trait Testable: Clone + PartialEq + Debug + Sync {}
impl<T: Clone + PartialEq + Debug + Sync> Testable for T {}

pub fn lock<A: MutexApi<T> + Sync, T: Testable>(value: &T) {
    let mut lock = A::new(value.clone());
    assert!(!lock.is_poisoned());

    thread::scope(|scope| {
        scope.spawn(|| {
            let guard = lock.lock().unwrap();
            black_box(&*guard);
            drop(guard);
        });

        scope.spawn(|| {
            let guard = lock.lock().unwrap();
            black_box(&*guard);
            drop(guard);
        });
    });

    assert_eq!(lock.get_mut().unwrap(), value);
    assert_eq!(lock.into_inner().unwrap(), *value);
}

pub fn lock_writing<A: MutexApi<T> + Sync, T: Testable + BitXorAssign + Send + Sync + Clone>(
    value: &T,
    mask: T,
) {
    let mut lock = A::new(value.clone());
    assert!(!lock.is_poisoned());

    thread::scope(|scope| {
        scope.spawn(|| {
            let mut guard = lock.lock().unwrap();
            *guard ^= mask.clone();
            drop(guard);
        });

        scope.spawn(|| {
            let mut guard = lock.lock().unwrap();
            *guard ^= mask.clone();
            drop(guard);
        });
    });

    assert_eq!(lock.get_mut().unwrap(), value);
    assert_eq!(lock.into_inner().unwrap(), *value);
}

pub fn lock_unsized<A: MutexApi<T> + ?Sized + Sync, T: ?Sized + Sync + PartialEq + Debug>(
    lock: &mut A,
    expected: &T,
) {
    assert!(!lock.is_poisoned());

    thread::scope(|scope| {
        scope.spawn(|| {
            let guard = lock.lock().unwrap();
            assert_eq!(&*guard, expected);
            drop(guard);
        });

        scope.spawn(|| {
            let guard = lock.lock().unwrap();
            assert_eq!(&*guard, expected);
            drop(guard);
        });
    });

    assert_eq!(lock.get_mut().unwrap(), expected);
}

pub fn race_lock<A: MutexApi<RaceChecker> + Sync>() {
    let lock = A::new(RaceChecker::new());
    let handles = CheckerHandles::new(4);

    thread::scope(|scope| {
        handles.guard(|| {
            scope.spawn(|| lock.lock().unwrap().write(&handles[0]));
            assert!(handles[0].will_be_locked());
            handles[0].release();

            scope.spawn(|| lock.lock().unwrap().write(&handles[1]));
            assert!(handles[1].will_be_locked());
            scope.spawn(|| lock.lock().unwrap().write(&handles[2]));
            assert!(handles[2].will_not_be_locked());

            handles[1].release();
            assert!(handles[2].will_be_locked());

            scope.spawn(|| lock.lock().unwrap().write(&handles[3]));
            assert!(handles[3].will_not_be_locked());
            handles[2].release();
            assert!(handles[3].will_be_locked());
            handles[3].release();
        })
    })
}

pub fn poison<A: MutexApi<T> + Sync, T: Testable>(value: &T, expect_poisoned: bool) {
    let mut lock = A::new(value.clone());

    drop(lock.lock().unwrap());

    let panic_message = "Poisoning it";
    thread::scope(|scope| {
        scope
            .spawn(|| {
                let guard = lock.lock().unwrap();
                black_box(|g| {
                    black_box(g);
                    panic!("{}", panic_message);
                })(&guard);
                drop(guard);
            })
            .join()
            .expect_err("Should have panicked")
            .downcast::<String>()
            .map(|err| assert_eq!(*err, panic_message.to_string()))
            .expect("Error must be a `String`");
    });

    if expect_poisoned {
        assert!(lock.is_poisoned(), "`A` must be poisoned.");
        let Err(error) = lock.lock() else {
            panic!("Expected `Err`, got `Ok`");
        };
        let guard = error.into_inner();
        assert_eq!(*guard, *value);
        drop(guard);
        assert!(lock.is_poisoned());
    } else {
        assert!(!lock.is_poisoned(), "`A` cannot ever be poisoned.");
        let guard = lock.lock().unwrap();
        assert_eq!(*guard, *value);
        drop(guard);
        assert!(!lock.is_poisoned());
    }

    lock.clear_poison();
    assert!(!lock.is_poisoned());
    let guard = lock.lock().unwrap();
    assert_eq!(*guard, *value);
    drop(guard);

    assert_eq!(lock.get_mut().unwrap(), value);
    assert_eq!(lock.into_inner().unwrap(), *value);
}

pub fn try_lock<A: MutexApi<T> + Sync, T: Testable>(value: &T) {
    let mut lock = A::new(value.clone());
    let lock_active = AtomicBool::new(false);

    thread::scope(|scope| {
        // Miri always wants to spuriously fail with `compare_exchange_weak` here...
        let guard = lock.try_lock().unwrap();
        black_box(&*guard);
        drop(guard);

        let handle = scope.spawn(|| {
            let guard = lock.try_lock().unwrap();
            assert_eq!(*guard, *value);
            lock_active.store(true, Ordering::Relaxed);
            while lock_active.load(Ordering::Relaxed) {
                black_box(&*guard);
            }
            drop(guard);
        });

        while !lock_active.load(Ordering::Relaxed) {}

        match lock.try_lock() {
            Ok(_) => panic!("Expected `Err(TryLockError::WouldBlock)`, got `Ok`."),
            Err(TryLockError::Poisoned(_)) => {
                panic!(
                    "Expected `Err(TryLockError::WouldBlock)`, got `Err(TryLockError::Poisoned)`."
                )
            }
            Err(TryLockError::WouldBlock) => (),
        };

        lock_active.store(false, Ordering::Relaxed);
        handle.join().unwrap();

        let guard = lock.try_lock().unwrap();
        assert_eq!(*guard, *value);
        drop(guard);
    });

    assert_eq!(lock.get_mut().unwrap(), value);
    assert_eq!(lock.into_inner().unwrap(), *value);
}

pub fn do_load_test<A: MutexApi<u64> + Sync>(
    threads: usize,
    reps: usize,
    cycles: usize,
    poisoning_reps: Option<usize>,
) {
    let value = 0_u64;
    let mut lock = A::new(value);

    thread::scope(|scope| {
        for thread in 0..threads {
            let lock_ref = &lock;
            scope.spawn(move || {
                let permute = || {
                    let mut rng = fastrand::Rng::with_seed(u64::try_from(thread).unwrap());
                    for rep in 0..reps {
                        let poison = || {
                            scope
                                .spawn(|| {
                                    let guard =
                                        lock_ref.lock().unwrap_or_else(PoisonError::into_inner);
                                    black_box(|value| (panic!("Poisoning: {}", value)))(*guard);
                                    drop(guard);
                                })
                                .join()
                                .unwrap_err()
                                .downcast::<String>()
                                .expect("Error must be a `String`");
                        };

                        let mut normal = || {
                            let mut guard = lock_ref.lock().unwrap_or_else(PoisonError::into_inner);
                            for _ in 0..cycles {
                                *guard ^= rng.u64(0..u64::MAX);
                            }
                            drop(guard);
                        };

                        normal();
                        poisoning_reps.map(|poisoning_reps| {
                            match (rep + thread) % (poisoning_reps) {
                                0 => {
                                    poison();
                                }
                                i if poisoning_reps / 2 == i => {
                                    lock_ref.clear_poison();
                                }
                                _ => (),
                            }
                        });
                    }
                };

                // These two calls must balance each other's XOR.
                permute();
                permute();
            });
        }
    });

    assert_eq!(
        *lock.get_mut().unwrap_or_else(PoisonError::into_inner),
        0_u64
    );
    assert_eq!(
        lock.into_inner().unwrap_or_else(PoisonError::into_inner),
        0_u64
    );
}
