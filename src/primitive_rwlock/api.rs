pub trait RwLockHook {
    fn new() -> Self
    where
        Self: Sized;

    fn before_read(&self) {}
    fn before_write(&self) {}
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
