use std::{
    any::Any,
    error::Error,
    fmt::{Debug, Display},
    panic::{AssertUnwindSafe, catch_unwind, resume_unwind},
    thread::{self, Builder, Scope},
};

use powerlocks::strategied_rwlock::{Method, RwLockApi};

macro_rules! error_type {
        ($vis:vis $name:ident { $($option:ident($message:literal)),* $(,)? }) => {
            #[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
            $vis enum $name {
                $($option,)*
            }

            impl Display for $name {
                #[allow(unused_variables)] // If we're generating an empty enum
                fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
                    match *self {
                        $(Self::$option => write!(f, $message),)*
                    }
                }
            }

            impl Error for $name {}
        };
    }

error_type!(pub StrategyLogicError {
    ConcurrentReadAndWrite(
        "The provided `Strategy` wanted to `State::Ok` a `Method::Write` and a \
        `Method::Read` together."
    ),
    ConcurrentMultipleWrites(
        "The provided `Strategy` wanted to `State::Ok` two or more `Method::Write`s."
    ),
    BlockedAfterOkState(
        "The provided `Strategy` wanted to re-block a `State::Ok`ed thread."
    ),
    BrokenLock(
        "There is a logic error in the provided `Strategy`. Can't continue."
    ),
});

#[derive(Debug, Clone, PartialEq)]
pub enum TryStrategyAttempt<E>
where
    E: Any + Debug + PartialEq + Send + Sync,
{
    Try(Method, Result<(), E>),
    UnlockAll,
}

pub fn try_strategy<E, T>(lock: &(impl RwLockApi<T> + Sync), attempts: &[TryStrategyAttempt<E>])
where
    E: Any + Debug + PartialEq + Send + Sync,
    T: ?Sized + Sync,
{
    fn try_strategy_inner<'a, E, T, L>(
        lock: &'a L,
        scope: &'a Scope<'a, '_>,
        iteration: usize,
        attempts: &'a [TryStrategyAttempt<E>],
    ) where
        E: Any + Debug + PartialEq + Send + Sync,
        T: ?Sized + Sync,
        L: RwLockApi<T> + Sync,
    {
        match attempts.split_first() {
            None => (),
            Some((next, rest)) => {
                let TryStrategyAttempt::Try(method, expectation) = next else {
                    panic!("Expected a `Try` variant, got `UnlockAll`.")
                };

                let panic_message = expectation.as_ref().err();
                let should_panic = panic_message.is_some();

                let thread_name = format!(
                    "`try_strategy` {} #{} - {}",
                    match method {
                        Method::Read => "reader",
                        Method::Write => "writer",
                    },
                    iteration,
                    if should_panic { "should panic" } else { "" }
                );

                let result = Builder::new()
                    .name(thread_name)
                    .spawn_scoped(scope, move || {
                        let execution = || match *method {
                            Method::Read => {
                                let guard = catch_unwind(AssertUnwindSafe(|| lock.read().unwrap()));
                                // Always try to spawn threads, even when unwinding.
                                try_strategy_inner(lock, scope, iteration + 1, rest);
                                drop(guard.map_err(resume_unwind).unwrap());
                            }
                            Method::Write => {
                                let guard =
                                    catch_unwind(AssertUnwindSafe(|| lock.write().unwrap()));
                                try_strategy_inner(lock, scope, iteration + 1, rest);
                                drop(guard.map_err(resume_unwind).unwrap());
                            }
                        };

                        if should_panic {
                            super::suppress_panic_message(execution);
                        } else {
                            execution();
                        }
                    })
                    .unwrap()
                    .join();

                if should_panic {
                    result
                        .expect_err("This attempt must panic")
                        .downcast::<E>()
                        .map(|err| assert_eq!(&*err, panic_message.unwrap()))
                        .expect("Error must be of type `E`")
                } else {
                    result.expect("This attempt must not panic")
                }
            }
        }
    }

    thread::scope(|scope| {
        attempts
            .split(|attempt| match attempt {
                TryStrategyAttempt::UnlockAll => true,
                _ => false,
            })
            .for_each(|attempt_set| try_strategy_inner(lock, scope, 0, attempt_set));
    });
}
