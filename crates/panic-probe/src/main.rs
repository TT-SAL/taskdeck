//! Checks that the workspace's release profile unwinds on panic.
//!
//! `cargo test --release` cannot prove this: cargo builds test harnesses with
//! unwinding whatever the profile says. A plain binary built with
//! `cargo build --release` gets the profile's real `panic` setting, so this
//! probe panics inside `catch_unwind` and reports whether it survived.
//!
//! Exit 0: the panic was caught and execution continued.
//! Exit 2: `catch_unwind` returned without a panic (the probe is broken).
//! Under `panic = "abort"` the process dies with SIGABRT (or the platform's
//! abort code) before it can print the success line.

use std::panic;
use std::process::ExitCode;

fn main() -> ExitCode {
    // Silent hook: the deliberate panic should not print a scary backtrace.
    panic::set_hook(Box::new(|_| {}));

    let outcome = panic::catch_unwind(|| {
        // `black_box` keeps the optimiser from reasoning about the panic.
        if std::hint::black_box(true) {
            panic!("panic-probe: deliberate panic");
        }
    });

    // Put the default hook back so a real panic below would still be reported.
    let _ = panic::take_hook();

    match outcome {
        Err(_) => {
            println!("panic-probe: panic caught, execution continued");
            ExitCode::SUCCESS
        }
        Ok(()) => {
            eprintln!("panic-probe: closure returned without panicking");
            ExitCode::from(2)
        }
    }
}
