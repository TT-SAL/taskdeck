//! The nine entry points and `JNI_OnLoad` (bridge.md §11.2, §21); the only
//! module where the android stages allow `unsafe` (BRIDGE-GEN-001).
//!
//! Declared and empty: exporting a symbol needs `#[unsafe(no_mangle)]`, and
//! the skeleton has no `unsafe` (task 4). Stage `android-shell` writes them.
