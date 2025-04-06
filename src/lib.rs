#![no_std]

pub mod primitives;

#[cfg(feature = "mutex")]
pub mod mutex;

#[cfg(feature = "rwlock")]
pub mod rwlock;

#[cfg(feature = "rwlock")]
pub mod primitive_rwlock;
