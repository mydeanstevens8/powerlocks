#![cfg(feature = "mutex")]

use std::panic::{AssertUnwindSafe, catch_unwind, resume_unwind, set_hook, take_hook};

pub mod tests;

pub fn suppress_panic_message<T>(f: impl FnOnce() -> T) -> T {
    set_hook(Box::new(|_| {}));
    let result = catch_unwind(AssertUnwindSafe(f));
    let _ = take_hook();
    result.unwrap_or_else(|panic| resume_unwind(panic))
}
