//! Any panic ends the process, as it did under `panic = "abort"`.
//!
//! The release profile unwinds (Cargo.toml says why), so without this a panic
//! on a background thread (a phone worker, the pulse, the weather, the sync
//! sender or listener) would end only that thread, and the process would run on
//! without it, out of reach of the service manager's restart. Both binaries call
//! [`abort_on_panic`] first thing in release builds; debug builds unwind as
//! they always did.

use std::panic;

/// Install a panic hook that prints the panic as before, then aborts.
pub fn abort_on_panic() {
    let previous = panic::take_hook();
    panic::set_hook(Box::new(move |info| {
        previous(info);
        std::process::abort();
    }));
}

#[cfg(test)]
mod tests {
    use std::{
        io::Read,
        process::{Command, ExitStatus, Stdio},
        thread,
        time::{Duration, Instant},
    };

    const CHILD: &str = "TASKDECK_PANIC_ABORT_CHILD";
    const MESSAGE: &str = "deliberate panic on a worker thread";
    const OUTLIVED: &str = "child outlived the worker's panic";

    /// Run this test again in a child process, as the child `mode` says.
    fn run_child(mode: &str) -> (ExitStatus, String, String) {
        let name = concat!(module_path!(), "::a_panic_on_any_thread_ends_the_process");
        let name = name.split_once("::").map_or(name, |(_, rest)| rest);
        let mut child = Command::new(std::env::current_exe().unwrap())
            .args([name, "--exact", "--nocapture", "--test-threads=1"])
            .env(CHILD, mode)
            .env("RUST_BACKTRACE", "0")
            .stdin(Stdio::null())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()
            .unwrap();
        let deadline = Instant::now() + Duration::from_secs(10);
        let status = loop {
            if let Some(status) = child.try_wait().unwrap() {
                break status;
            }
            if Instant::now() > deadline {
                let _ = child.kill();
                let _ = child.wait();
                panic!("child ({mode}) still running after 10 s");
            }
            thread::sleep(Duration::from_millis(20));
        };
        let (mut out, mut err) = (String::new(), String::new());
        child.stdout.take().unwrap().read_to_string(&mut out).unwrap();
        child.stderr.take().unwrap().read_to_string(&mut err).unwrap();
        (status, out, err)
    }

    #[cfg(unix)]
    fn aborted(status: ExitStatus) -> bool {
        use std::os::unix::process::ExitStatusExt;
        status.signal() == Some(6) // SIGABRT
    }

    #[cfg(not(unix))]
    fn aborted(status: ExitStatus) -> bool {
        // 101 is how a test binary exits when a test panics without aborting.
        !matches!(status.code(), Some(0) | Some(101))
    }

    #[test]
    fn a_panic_on_any_thread_ends_the_process() {
        if let Ok(mode) = std::env::var(CHILD) {
            if mode == "hook" {
                super::abort_on_panic();
            }
            let worker = thread::spawn(|| panic!("{MESSAGE}"));
            assert!(worker.join().is_err(), "the worker did not panic");
            println!("{OUTLIVED}");
            return;
        }

        let (status, out, err) = run_child("hook");
        assert!(aborted(status), "with the hook: {status}\nstdout:\n{out}\nstderr:\n{err}");
        assert!(err.contains(MESSAGE), "with the hook, the panic message was not printed:\n{err}");
        assert!(!out.contains(OUTLIVED), "with the hook, the child outlived the panic:\n{out}");

        let (status, out, err) = run_child("none");
        assert!(status.success(), "without the hook: {status}\nstdout:\n{out}\nstderr:\n{err}");
        assert!(err.contains(MESSAGE), "without the hook, the panic message was not printed:\n{err}");
        assert!(out.contains(OUTLIVED), "without the hook, the child did not outlive the panic:\n{out}");
    }
}
