//! `taskdeck-devicectl`: the headless CLI the end-to-end suite drives, one
//! facade call per request (device.md §19, DEV-CTL-001). It exists only with
//! the feature `test-trust`.
//!
//! Spec: `docs/sync/spec/device.md` §19. Owner: stage `device`. Created as a
//! shell by stage `foundation`.
#![deny(unsafe_code)]
#![deny(unsafe_op_in_unsafe_fn)]

use taskdeck_device as _;

fn main() {
    eprintln!("taskdeck-devicectl: not built yet (stage device)");
    std::process::exit(2);
}
