use crate::primitives::ShouldBlock;

pub trait RwLockHook {
    fn new() -> Self
    where
        Self: Sized;

    fn try_read(&self) -> ShouldBlock {
        ShouldBlock::Ok
    }

    fn try_write(&self) -> ShouldBlock {
        ShouldBlock::Ok
    }

    fn after_read(&self) {}
    fn after_write(&self) {}
}

// `()` means a basic hook that does nothing.
impl RwLockHook for () {
    fn new() -> Self
    where
        Self: Sized,
    {
    }
}
