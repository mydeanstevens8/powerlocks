use crate::primitives::{ShouldBlock, ThreadEnv};

pub mod fair;

// This is a redefinition of `RwLockHook`, but it's designed to be independent so the hooks here can
// be used both in a `Mutex` and a `RwLock` without leading to conflicting implementations across
// their respective hook traits.
pub trait MultiHook<Env: ThreadEnv> {
    fn new() -> Self
    where
        Self: Sized;

    fn try_read(&self) -> ShouldBlock {
        ShouldBlock::Ok
    }

    fn read(&self) {
        while let ShouldBlock::Block = self.try_read() {
            Env::yield_now();
        }
    }

    fn after_read(&self) {}

    fn try_write(&self) -> ShouldBlock {
        ShouldBlock::Ok
    }

    fn write(&self) {
        while let ShouldBlock::Block = self.try_write() {
            Env::yield_now();
        }
    }

    fn after_write(&self) {}
}

#[cfg(feature = "mutex")]
mod mutex_impl {
    use super::MultiHook;
    use crate::{mutex::MutexHook, primitives::ThreadEnv};

    impl<H: MultiHook<Env>, Env: ThreadEnv> MutexHook<Env> for H {
        fn new() -> Self
        where
            Self: Sized,
        {
            <H as MultiHook<Env>>::new()
        }

        fn try_lock(&self) -> crate::primitives::ShouldBlock {
            <H as MultiHook<Env>>::try_write(self)
        }

        fn lock(&self) {
            <H as MultiHook<Env>>::write(self);
        }

        fn after_lock(&self) {
            <H as MultiHook<Env>>::after_write(self);
        }
    }
}

#[cfg(feature = "rwlock")]
mod rwlock_impl {
    use super::MultiHook;
    use crate::{primitives::ThreadEnv, rwlock::RwLockHook};

    impl<H: MultiHook<Env>, Env: ThreadEnv> RwLockHook<Env> for H {
        fn new() -> Self
        where
            Self: Sized,
        {
            <H as MultiHook<Env>>::new()
        }

        fn try_read(&self) -> crate::primitives::ShouldBlock {
            <H as MultiHook<Env>>::try_read(self)
        }

        fn read(&self) {
            <H as MultiHook<Env>>::read(&self);
        }

        fn after_read(&self) {
            <H as MultiHook<Env>>::after_read(self);
        }

        fn try_write(&self) -> crate::primitives::ShouldBlock {
            <H as MultiHook<Env>>::try_write(self)
        }

        fn write(&self) {
            <H as MultiHook<Env>>::write(&self);
        }

        fn after_write(&self) {
            <H as MultiHook<Env>>::after_write(self);
        }
    }
}
