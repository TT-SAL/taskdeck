//! `taskdeck-devicectl`: the headless CLI the end-to-end suite drives, one
//! facade call per request (device.md §19, DEV-CTL-001). It exists only with
//! the feature `test-trust`.
//!
//! Spec: `docs/sync/spec/device.md` §19. Owner: stage `device`. Created as a
//! shell by stage `foundation`. Device.md §25 names no public item of the
//! binary; its `main` is `unimplemented!()` until stage `device` writes the
//! protocol of §19.2.
#![deny(unsafe_code)]
#![deny(unsafe_op_in_unsafe_fn)]

use taskdeck_device as _;

/// Runs the protocol of device.md §19.2.
fn main() {
    unimplemented!()
}
