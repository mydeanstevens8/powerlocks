# Powerlocks

A crate providing locks and synchronization primitives, written entirely in
Rust, with little to no dependencies on system libraries. This library does not
require the Rust Standard library, although integrations can be enabled with the
library using the `std` feature.

This crate has no external dependencies whatsoever. (The `fastrand` dependency
is used for the test harness.)

## Features

- `mutex` - A mutex that uses core atomic instructions for synchronization
  rather than system libraries.
- `rwlock` - A readers-writers lock that uses a configurable locking strategy at
  the back-end, allowing fine-grained control. Requires the `alloc` library.
- `std` - Enables various integrations with the Rust standard library, allowing
  `mutex`, `rwlock` etc. to use OS-level synchronization primitives where
  appropriate, which may help to improve performace. Adds lock poisoning
  support.

## Notes and caveats

- This crate is not yet stable. Breaking API changes may be introduced at short
  notice.
- The standard library is _not_ required, but the `alloc` library is required
  for certain features, such as `rwlock` (which uses a `Vec` to enqueue threads
  in the lock for the purposes of using strategies).
  - There are plans to introduce a more primitive `rwlock` that does not require
    the `alloc` library.
- These locks do not implement poisoning, except when the `std` feature is
  enabled and a supported lock is used.
- This crate uses lots of unsafe code internally and needs to be properly vetted
  for type and memory safety.
