//! `taskdeck-devicectl`: the headless CLI the end-to-end suite drives, one
//! facade call per request (device.md §19, DEV-CTL-001). It exists only with
//! the feature `test-trust`.
//!
//! Spec: `docs/sync/spec/device.md` §19. Owner: stage `device`. The protocol
//! lives in the library (`taskdeck_device::ctl`, behind `test-trust`) so
//! that it shares the device's strict JSON parser and is tested there.
#![deny(unsafe_code)]
#![deny(unsafe_op_in_unsafe_fn)]

/// Runs the protocol of device.md §19.2.
fn main() {
    std::process::exit(taskdeck_device::ctl::main());
}
