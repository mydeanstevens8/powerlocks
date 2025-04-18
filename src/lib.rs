#![no_std]

pub mod primitives;

#[cfg(feature = "mutex")]
pub mod mutex;

#[cfg(feature = "rwlock")]
pub mod strategied_rwlock;

#[cfg(feature = "rwlock")]
pub mod rwlock;
