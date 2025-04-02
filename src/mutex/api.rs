use core::ops::{Deref, DerefMut};

use crate::primitives::{LockResult, TryLockError, TryLockResult};

pub trait MutexGuardApi<'a, T: 'a + ?Sized>: Deref<Target = T> + DerefMut<Target = T> {}

pub trait MutexApi<T: ?Sized> {
    fn try_lock<'a>(&'a self) -> TryLockResult<impl MutexGuardApi<'a, T>>
    where
        T: 'a;

    fn lock<'a>(&'a self) -> LockResult<impl MutexGuardApi<'a, T>>
    where
        T: 'a,
    {
        loop {
            match self.try_lock() {
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
pub mod std_mutex_api {
    #[cfg(feature = "std")]
    extern crate std;

    use super::{MutexApi, MutexGuardApi};
    use crate::primitives::{LockResult, PoisonError, TryLockError, TryLockResult};

    impl<'a, T: 'a + ?Sized> MutexGuardApi<'a, T> for std::sync::MutexGuard<'a, T> {}

    impl<T: ?Sized> MutexApi<T> for std::sync::Mutex<T> {
        fn try_lock<'a>(&'a self) -> TryLockResult<impl MutexGuardApi<'a, T>>
        where
            T: 'a,
        {
            self.try_lock().map_err(TryLockError::from)
        }

        fn lock<'a>(&'a self) -> LockResult<impl MutexGuardApi<'a, T>>
        where
            T: 'a,
        {
            self.lock().map_err(PoisonError::from)
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
