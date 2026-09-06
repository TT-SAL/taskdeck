pub mod archive;
pub mod tasks;
// The window and everything that only it needs. Behind `desk` so the
// headless server can be built without a graphics toolkit — see Cargo.toml.
#[cfg(feature = "desk")]
pub mod ui;
pub mod utilities;
#[cfg(feature = "desk")]
pub mod weather;
#[cfg(feature = "desk")]
pub mod calendarwidgets;
pub mod initialization;
pub mod color;
pub mod paths;
pub mod planner;
pub mod phone;
pub mod board;
pub mod ics;
pub mod subscriptions;
pub mod sync;