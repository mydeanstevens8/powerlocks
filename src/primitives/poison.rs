// These types are mostly a copy of the respective types in `std::sync`, dated as of Rust 1.85.
// They exist to re-export the lock primitives without needing to manually import the Standard
// library. Portions of this code is copyright (C) 2025 Rust Contributors, licensed under the MIT
// or Apache 2.0 license, at your option.
//
// Modifications done to them include repurposing certain deprecated functions like
// `Error::description` as well as type name/inference changes.

use core::{
    error::Error,
    fmt::{self, Debug, Display, Formatter},
};

/// A type of error which can be returned whenever a lock is acquired.
///
/// See also: [`std::sync::PoisonError`].
pub struct PoisonError<T> {
    data: T,
}

impl<T> Debug for PoisonError<T> {
    fn fmt(&self, f: &mut Formatter<'_>) -> fmt::Result {
        f.debug_struct("PoisonError").finish_non_exhaustive()
    }
}

impl<T> Display for PoisonError<T> {
    fn fmt(&self, f: &mut Formatter<'_>) -> fmt::Result {
        Display::fmt("poisoned lock: another task failed inside", f)
    }
}

impl<T> Error for PoisonError<T> {}

impl<T> PoisonError<T> {
    /// Creates a `PoisonError`.
    ///
    /// See also: [`std::sync::PoisonError::new`].
    pub fn new(data: T) -> PoisonError<T> {
        if cfg!(panic = "unwind") {
            PoisonError { data }
        } else {
            panic!("`PoisonError` created in `primitives` built with panic=\"abort\"");
        }
    }

    /// Consumes this error indicating that a lock is poisoned, returning the
    /// associated data.
    ///
    /// See also: [`std::sync::PoisonError::into_inner`].
    pub fn into_inner(self) -> T {
        self.data
    }

    /// Reaches into this error indicating that a lock is poisoned, returning a
    /// reference to the associated data.
    ///
    /// See also: [`std::sync::PoisonError::get_ref`].
    pub fn get_ref(&self) -> &T {
        &self.data
    }

    /// Reaches into this error indicating that a lock is poisoned, returning a
    /// mutable reference to the associated data.
    ///
    /// See also: [`std::sync::PoisonError::get_mut`].
    pub fn get_mut(&mut self) -> &mut T {
        &mut self.data
    }
}

/// An enumeration of possible errors associated with a [`TryLockResult`] which
/// can occur while trying to acquire a lock.
///
/// See also: [`std::sync::TryLockError`].
pub enum TryLockError<T> {
    /// The lock could not be acquired because another thread failed while holding
    /// the lock.
    Poisoned(PoisonError<T>),
    /// The lock could not be acquired at this time because the operation would
    /// otherwise block.
    WouldBlock,
}

impl<T> From<PoisonError<T>> for TryLockError<T> {
    fn from(err: PoisonError<T>) -> TryLockError<T> {
        TryLockError::Poisoned(err)
    }
}

impl<T> Debug for TryLockError<T> {
    fn fmt(&self, f: &mut Formatter<'_>) -> fmt::Result {
        match *self {
            TryLockError::Poisoned(..) => Debug::fmt("Poisoned(..)", f),
            TryLockError::WouldBlock => Debug::fmt("WouldBlock", f),
        }
    }
}

impl<T> Display for TryLockError<T> {
    fn fmt(&self, f: &mut Formatter<'_>) -> fmt::Result {
        Display::fmt(
            match *self {
                TryLockError::Poisoned(..) => "poisoned lock: another task failed inside",
                TryLockError::WouldBlock => "try_lock failed because the operation would block",
            },
            f,
        )
    }
}

impl<T> Error for TryLockError<T> {}

/// A type alias for the result of a lock method which can be poisoned.
///
/// See also: [`std::sync::LockResult`].
pub type LockResult<T> = Result<T, PoisonError<T>>;

/// A type alias for the result of a nonblocking locking method.
///
/// See also: [`std::sync::TryLockResult`].
pub type TryLockResult<Guard> = Result<Guard, TryLockError<Guard>>;

#[cfg(feature = "std")]
pub mod conversions {
    #[cfg(feature = "std")]
    extern crate std;

    impl<T> From<std::sync::PoisonError<T>> for super::PoisonError<T> {
        fn from(value: std::sync::PoisonError<T>) -> Self {
            Self::new(value.into_inner())
        }
    }

    impl<T> From<super::PoisonError<T>> for std::sync::PoisonError<T> {
        fn from(value: super::PoisonError<T>) -> Self {
            Self::new(value.into_inner())
        }
    }

    impl<T> From<std::sync::TryLockError<T>> for super::TryLockError<T> {
        fn from(value: std::sync::TryLockError<T>) -> Self {
            match value {
                std::sync::TryLockError::Poisoned(guard) => Self::Poisoned(guard.into()),
                std::sync::TryLockError::WouldBlock => Self::WouldBlock,
            }
        }
    }

    impl<T> From<super::TryLockError<T>> for std::sync::TryLockError<T> {
        fn from(value: super::TryLockError<T>) -> Self {
            match value {
                super::TryLockError::Poisoned(guard) => Self::Poisoned(guard.into()),
                super::TryLockError::WouldBlock => Self::WouldBlock,
            }
        }
    }
}
