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
            // The hook child aborts, which may dump core into its working
            // directory: keep that out of the checkout.
            .current_dir(std::env::temp_dir())
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

    /// Each binary's `main` must install the hook first, in release builds only.
    fn assert_main_installs_hook(file: &str, source: &str) {
        let mut mains = source.split("fn main() {");
        let body = mains.nth(1).unwrap_or_else(|| panic!("{file}: no `fn main() {{`"));
        assert!(mains.next().is_none(), "{file}: more than one `fn main() {{`");
        let first: Vec<&str> = body.split_whitespace().take(2).collect();
        assert_eq!(
            first,
            ["#[cfg(not(debug_assertions))]", "task_deck::panic_abort::abort_on_panic();"],
            "{file}: main() must start by installing the hook, in release builds only"
        );
    }

    /// The hook is named once in each binary's source and nowhere else in the
    /// files the crate compiles (its three roots and the `mod name;` files they
    /// declare), so no second, unguarded install hides behind the guarded one.
    /// Other files in src/ (an editor's lock file, a stray copy) are not built.
    fn assert_hook_installed_nowhere_else() {
        let src = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("src");
        let read = |file: &str| {
            std::fs::read_to_string(src.join(file)).unwrap_or_else(|e| panic!("src/{file}: {e}"))
        };
        let roots = ["lib.rs", "main.rs", "server_main.rs"];
        let mut files: Vec<String> = roots.map(String::from).to_vec();
        for root in roots {
            for line in read(root).lines() {
                let words: Vec<&str> = line.split_whitespace().collect();
                if let [vis @ .., "mod", name] = &words[..] {
                    if let Some(name) = name.strip_suffix(';') {
                        if vis.iter().all(|w| w.starts_with("pub")) {
                            files.push(format!("{name}.rs"));
                        }
                    }
                }
            }
        }
        assert!(files.iter().any(|f| f == "panic_abort.rs"), "found no `mod panic_abort;` in {files:?}");
        for file in files.iter().filter(|f| *f != "panic_abort.rs") {
            let count = read(file).matches("abort_on_panic").count();
            let expected = usize::from(file == "main.rs" || file == "server_main.rs");
            assert_eq!(count, expected, "src/{file} names abort_on_panic {count} times");
        }
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

        assert_main_installs_hook("src/main.rs", include_str!("main.rs"));
        assert_main_installs_hook("src/server_main.rs", include_str!("server_main.rs"));
        assert_hook_installed_nowhere_else();

        let (status, out, hook_err) = run_child("hook");
        assert!(aborted(status), "with the hook: {status}\nstdout:\n{out}\nstderr:\n{hook_err}");
        assert!(!out.contains(OUTLIVED), "with the hook, the child outlived the panic:\n{out}");

        let (status, out, err) = run_child("none");
        assert!(status.success(), "without the hook: {status}\nstdout:\n{out}\nstderr:\n{err}");
        let printed = err.matches(MESSAGE).count();
        assert_eq!(printed, 1, "without the hook, the panic message was printed {printed} times:\n{err}");
        assert!(out.contains(OUTLIVED), "without the hook, the child did not outlive the panic:\n{out}");

        // The hook must print the panic exactly as the default hook does. The
        // line names the thread by its id, which differs between the two runs.
        let digitless = |s: &str| s.replace(|c: char| c.is_ascii_digit(), "");
        assert_eq!(
            digitless(&hook_err),
            digitless(&err),
            "with the hook, stderr differs from the default hook's:\n{hook_err}\nwithout:\n{err}"
        );
    }
}
