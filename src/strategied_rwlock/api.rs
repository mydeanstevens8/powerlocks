use super::Strategy;

extern crate alloc;
use alloc::boxed::Box;

use crate::rwlock::RwLockApi;

pub trait StrategiedRwLockApi<T: ?Sized>: RwLockApi<T> {
    fn new_strategied(t: T, strategy: Box<dyn Strategy>) -> Self
    where
        Self: Sized,
        T: Sized;
}
