pub mod archive;
pub mod tasks;
// The window and everything that only it needs. Behind `desk` so the
// headless server can be built without a graphics toolkit — see Cargo.toml.
#[cfg(feature = "desk")]
pub mod ui;
pub mod utilities;
// The forecast. Not behind `desk`: the fetch, the report and the symbol table
// are plain data the headless server serves to the phone too, and only the
// toolkit's own `ImageSource` inside it is feature-gated.
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