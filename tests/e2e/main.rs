//! The end-to-end suite (device.md §20): devices driven through
//! `taskdeck-devicectl` against the standby relay on loopback. The harness is
//! in `harness/`; each scenario is a test in its own module, written with the
//! typed helpers of `harness::Ctl`. See `README.md` for how to write one.
//!
//! Spec: `docs/sync/spec/device.md` §19, §20. Built only with the feature
//! `test-trust` (`required-features` of the test target).
#![forbid(unsafe_code)]

mod harness;
mod smoke;
