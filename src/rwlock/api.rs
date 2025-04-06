use core::ops::{Deref, DerefMut};

use crate::primitives::{LockResult, ShouldBlock, TryLockError, TryLockResult};

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

pub trait RwLockReadGuardApi<'a, T: 'a + ?Sized>: Deref<Target = T> {}
pub trait RwLockWriteGuardApi<'a, T: 'a + ?Sized>:
    Deref<Target = T> + DerefMut<Target = T>
{
}

pub trait RwLockApi<T: ?Sized> {
    fn try_read<'a>(&'a self) -> TryLockResult<impl RwLockReadGuardApi<'a, T>>
    where
        T: 'a;

    fn read<'a>(&'a self) -> LockResult<impl RwLockReadGuardApi<'a, T>>
    where
        T: 'a,
    {
        loop {
            match self.try_read() {
                Ok(guard) => break Ok(guard),
                Err(TryLockError::Poisoned(poison)) => break Err(poison),
                Err(TryLockError::WouldBlock) => continue,
            };
        }
    }

    fn try_write<'a>(&'a self) -> TryLockResult<impl RwLockWriteGuardApi<'a, T>>
    where
        T: 'a;

    fn write<'a>(&'a self) -> LockResult<impl RwLockWriteGuardApi<'a, T>>
    where
        T: 'a,
    {
        loop {
            match self.try_write() {
                Ok(guard) => break Ok(guard),
                Err(TryLockError::Poisoned(poison)) => break Err(poison),
                Err(TryLockError::WouldBlock) => continue,
            };
        }
    }

    fn get_mut(&mut self) -> LockResult<&mut T>;

    fn new(t: T) -> Self
    where
        Self: Sized,
        T: Sized;

    fn into_inner(self) -> LockResult<T>
    where
        Self: Sized,
        T: Sized;

    fn is_poisoned(&self) -> bool {
        false
    }

    fn clear_poison(&self) {}
}

#[cfg(feature = "std")]
pub mod std_rwlock_api {
    #[cfg(feature = "std")]
    extern crate std;

    use super::{RwLockApi, RwLockReadGuardApi, RwLockWriteGuardApi};
    use crate::primitives::{LockResult, PoisonError, TryLockError, TryLockResult};

    impl<'a, T: 'a + ?Sized> RwLockReadGuardApi<'a, T> for std::sync::RwLockReadGuard<'a, T> {}
    impl<'a, T: 'a + ?Sized> RwLockWriteGuardApi<'a, T> for std::sync::RwLockWriteGuard<'a, T> {}

    impl<T: ?Sized> RwLockApi<T> for std::sync::RwLock<T> {
        fn try_read<'a>(&'a self) -> TryLockResult<impl RwLockReadGuardApi<'a, T>>
        where
            T: 'a,
        {
            self.try_read().map_err(TryLockError::from)
        }

        fn read<'a>(&'a self) -> LockResult<impl RwLockReadGuardApi<'a, T>>
        where
            T: 'a,
        {
            self.read().map_err(PoisonError::from)
        }

        fn try_write<'a>(&'a self) -> TryLockResult<impl RwLockWriteGuardApi<'a, T>>
        where
            T: 'a,
        {
            self.try_write().map_err(TryLockError::from)
        }

        fn write<'a>(&'a self) -> LockResult<impl RwLockWriteGuardApi<'a, T>>
        where
            T: 'a,
        {
            self.write().map_err(PoisonError::from)
        }

        fn is_poisoned(&self) -> bool {
            self.is_poisoned()
        }

        fn clear_poison(&self) {
            self.clear_poison();
        }

        fn get_mut(&mut self) -> LockResult<&mut T> {
            self.get_mut().map_err(PoisonError::from)
        }

        fn new(t: T) -> Self
        where
            Self: Sized,
            T: Sized,
        {
            Self::new(t)
        }

        fn into_inner(self) -> LockResult<T>
        where
            Self: Sized,
            T: Sized,
        {
            self.into_inner().map_err(PoisonError::from)
        }
    }
}
