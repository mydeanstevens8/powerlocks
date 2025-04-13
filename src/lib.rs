#![no_std]

pub mod mutex;
pub mod primitives;
pub mod rwlock;

#[cfg(feature = "rwlock")]
pub mod strategied_rwlock;
