pub trait ThreadEnv {
    fn yield_now()
    where
        Self: Sized,
    {
    }

    fn panicking() -> bool
    where
        Self: Sized,
    {
        false
    }
}

#[derive(Debug, Clone, Copy)]
pub struct CoreThreadEnv;
impl ThreadEnv for CoreThreadEnv {
    fn yield_now()
    where
        Self: Sized,
    {
        core::hint::spin_loop();
    }

    fn panicking() -> bool
    where
        Self: Sized,
    {
        false
    }
}

#[cfg(feature = "std")]
mod std_thread_env {
    extern crate std;

    use super::ThreadEnv;
    use std::thread;

    #[derive(Debug, Clone, Copy)]
    pub struct StdThreadEnv;
    impl ThreadEnv for StdThreadEnv {
        fn yield_now() {
            thread::yield_now();
        }

        fn panicking() -> bool {
            thread::panicking()
        }
    }
}

#[cfg(feature = "std")]
pub use std_thread_env::*;
