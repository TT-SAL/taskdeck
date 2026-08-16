//! Where TaskDeck keeps its files.
//!
//! Everything the app reads or writes at runtime lives under one **root**
//! folder, which holds `taskdeck_data/` (tasks, notes, colour schemes, config)
//! and `images/` (user background pictures). The root is resolved **once** at
//! startup by [`AppDirs::resolve`] and then passed around explicitly.
//!
//! This replaces the previous mix of working-directory-relative paths
//! (`images/`, `taskdeck_data/userconfig.toml`) and executable-relative ones
//! (everything else). The working directory is whatever the *launcher* happened
//! to set: it is the executable's folder when you double-click on Windows, but
//! `/` when you launch from Finder or the Dock on macOS, and the user's shell
//! directory when started from a terminal anywhere. Depending on it meant the
//! config and the background pictures could silently resolve somewhere else
//! than the tasks did — or nowhere at all.

use std::env;
use std::fs;
use std::path::{Path, PathBuf};

use tempfile::NamedTempFile;

/// Folder holding the user's tasks, notes, colour schemes and config.
pub const DATA_DIR_NAME: &str = "taskdeck_data";
/// The file whose lock says "a TaskDeck is using this folder".
const LOCK_FILE_NAME: &str = ".lock";
/// Folder holding user-supplied background images.
pub const IMAGES_DIR_NAME: &str = "images";

/* ────────────────────── One TaskDeck to a data folder ──────────────────────
 *
 * Every save in this app is atomic: serialize, write a temp file beside the
 * real one, fsync it, rename over the top. That makes a save **crash-safe** —
 * the file on disk is always either the whole previous version or the whole new
 * one, never half of either.
 *
 * It does nothing whatever about a *second copy of the program*, because that
 * is a different problem. Two instances each read the task list at startup,
 * each keep their own picture of it in memory, and each write **the whole
 * picture** on every change. Both writes are individually perfect; the second
 * one simply replaces the first, and everything the other instance did since it
 * started is gone. No amount of atomicity helps, because nothing was ever torn
 * — the loss is that two programs disagreed about what the truth was and the
 * later one won.
 *
 * (The archive is the one exception: `archived.jsonl` is appended to, and an
 * append is atomic, so two instances filing things is safe. Restoring or
 * forgetting rewrites the whole log, and that clobbers like everything else.)
 *
 * So: an OS lock on a file in the folder, held for as long as the process
 * lives. An OS lock rather than a PID written to a file, because the kernel
 * releases it when the process dies however it dies — there is no such thing as
 * a stale lock to reason about, and no liveness check to get wrong.
 *
 * A second instance is **warned, not stopped.** Refusing to start is the
 * stricter answer and it is the wrong one here: the failure mode of a false
 * positive is "cannot open my own calendar", which is worse than the thing it
 * prevents, and some filesystems (network shares especially) do not lock
 * faithfully. The warning goes through the same startup-error window that
 * reports a quarantined file, so it has to be read and dismissed.
 */

/// An exclusive claim on a data folder, held for as long as this value lives.
///
/// Nothing is ever read from it — the point is the `File` staying open, and the
/// kernel's lock going with it when the process ends.
#[derive(Debug)]
pub struct DataClaim {
    /// `None` when the filesystem would not lock at all, which is not an error
    /// worth telling anyone about: it means the guard is unavailable here, not
    /// that anything is wrong.
    _locked: Option<fs::File>,
    /// Whether another TaskDeck already had the folder.
    contended: bool,
}

impl DataClaim {
    /// True when another instance already owns this folder — the caller should
    /// tell the user, because whichever of the two saves last will replace the
    /// other's work.
    pub fn is_contended(&self) -> bool {
        self.contended
    }

    /// What to put in front of the user when it is.
    pub fn warning(&self) -> String {
        format!(
            "Another TaskDeck is already using this {DATA_DIR_NAME} folder.\n\n\
             Both copies keep their own picture of your tasks and write all of it \
             when anything changes, so whichever one saves last will replace what \
             the other did. Close one of them."
        )
    }
}

/// Claim `data` for this process. Always succeeds — see the note above for why
/// contention is a warning rather than a refusal.
pub fn claim_data_dir(data: &Path) -> DataClaim {
    let _ = fs::create_dir_all(data);

    let file = match fs::OpenOptions::new()
        .create(true)
        .truncate(false)
        .write(true)
        .open(data.join(LOCK_FILE_NAME))
    {
        Ok(file) => file,
        // Nowhere to put the lock — a read-only folder, most likely, which the
        // first failed save will report far more usefully than this could.
        Err(_) => return DataClaim { _locked: None, contended: false },
    };

    match file.try_lock() {
        Ok(()) => DataClaim { _locked: Some(file), contended: false },
        Err(fs::TryLockError::WouldBlock) => DataClaim { _locked: None, contended: true },
        // The filesystem does not support locking. Carry on unguarded rather
        // than claiming a conflict that may not exist.
        Err(fs::TryLockError::Error(_)) => DataClaim { _locked: None, contended: false },
    }
}

/// The resolved locations TaskDeck reads and writes. Cheap to clone.
#[derive(Debug, Clone)]
pub struct AppDirs {
    /// Folder containing `taskdeck_data/` and `images/`.
    pub root: PathBuf,
    /// `<root>/taskdeck_data` — created on resolve if missing.
    pub data: PathBuf,
    /// `<root>/images` — created on resolve if missing.
    pub images: PathBuf,
}

impl AppDirs {
    /// Resolve the root and make sure both folders exist.
    ///
    /// Resolution order:
    /// 1. `TASKDECK_HOME`, if set — an escape hatch for putting the data
    ///    somewhere specific (also what the packaging scripts would use).
    /// 2. The project root, when running from `target/debug` or
    ///    `target/release` — so a `cargo run` build shares the repository's
    ///    data instead of scattering it through `target/`.
    /// 3. The executable's own folder, if it already contains `taskdeck_data/`
    ///    or `images/` — an existing portable install, wherever it sits.
    /// 4. The executable's own folder, if it is writable and not inside a macOS
    ///    `.app` bundle — the portable layout the README describes, created on
    ///    first run.
    /// 5. Otherwise the per-user data directory for the platform (see
    ///    [`user_data_dir`]), which is where an app installed read-only —
    ///    `/Applications`, `/usr/local/bin`, `C:\Program Files` — ends up.
    ///
    /// Creating the folders is best-effort: a failure here is not fatal, and
    /// surfaces later as a normal save/load error in the app's error window
    /// rather than as a failed startup.
    pub fn resolve() -> Self {
        let root = resolve_root();
        let data = root.join(DATA_DIR_NAME);
        let images = root.join(IMAGES_DIR_NAME);

        let _ = fs::create_dir_all(&data);
        let _ = fs::create_dir_all(&images);

        Self { root, data, images }
    }

    /// Path of the TOML settings file.
    pub fn config_file(&self) -> PathBuf {
        self.data.join("userconfig.toml")
    }

    /// Resolve a user-supplied image name to a path inside [`Self::images`],
    /// defending against path traversal. Only the final path component is kept,
    /// so `..`, absolute paths, drive prefixes, and embedded separators can't
    /// escape the folder. Returns `None` when `name` has no usable file-name
    /// component (e.g. `""`, `".."`, `"sub/"`).
    ///
    /// This is the single source of truth for that check — both the background
    /// loader and the colour-scheme generator go through it.
    pub fn image_path(&self, name: &str) -> Option<PathBuf> {
        let file_name = Path::new(name).file_name()?;
        Some(self.images.join(file_name))
    }

    /// Names of the pictures available as backgrounds. Unreadable directory →
    /// empty list, same as before; the Settings dropdown just has nothing to
    /// offer.
    pub fn background_options(&self) -> Vec<String> {
        match fs::read_dir(&self.images) {
            Ok(entries) => entries
                .filter_map(|entry| entry.ok())
                .filter_map(|entry| entry.file_name().into_string().ok())
                .collect(),
            Err(_) => Vec::new(),
        }
    }
}

fn resolve_root() -> PathBuf {
    if let Some(explicit) = env::var_os("TASKDECK_HOME").filter(|value| !value.is_empty()) {
        return PathBuf::from(explicit);
    }

    let exe_dir = env::current_exe()
        .ok()
        .and_then(|exe| exe.parent().map(Path::to_path_buf));

    if let Some(exe_dir) = exe_dir {
        if let Some(project_root) = cargo_project_root(&exe_dir) {
            return project_root;
        }

        if exe_dir.join(DATA_DIR_NAME).is_dir() || exe_dir.join(IMAGES_DIR_NAME).is_dir() {
            return exe_dir;
        }

        if !is_inside_macos_bundle(&exe_dir) && is_writable(&exe_dir) {
            return exe_dir;
        }
    }

    user_data_dir()
}

/// `<project>/target/<profile>/TaskDeck` → `<project>`, and only when that
/// folder really is a Cargo project. Checking for the `Cargo.toml` (rather than
/// walking two levels up unconditionally, as the old `get_data_dir` did) keeps
/// an installed binary that happens to sit two levels deep from adopting an
/// unrelated folder as its data directory.
fn cargo_project_root(exe_dir: &Path) -> Option<PathBuf> {
    let target_dir = exe_dir.parent()?;
    if target_dir.file_name()? != "target" {
        return None;
    }
    let project_root = target_dir.parent()?;
    project_root
        .join("Cargo.toml")
        .is_file()
        .then(|| project_root.to_path_buf())
}

/// A macOS bundle keeps its executable in `TaskDeck.app/Contents/MacOS/`. That
/// tree is code-signed and may be read-only, so user data never belongs in it —
/// even when the running user happens to be able to write there.
fn is_inside_macos_bundle(exe_dir: &Path) -> bool {
    exe_dir.file_name() == Some("MacOS".as_ref())
        && exe_dir.parent().and_then(Path::file_name) == Some("Contents".as_ref())
}

/// Probe write access by actually creating a file — permission bits alone don't
/// account for read-only mounts, sandboxes, or ACLs. The temp file removes
/// itself when dropped, so the probe leaves nothing behind.
fn is_writable(dir: &Path) -> bool {
    NamedTempFile::new_in(dir).is_ok()
}

/// The per-user data directory, following each platform's own convention.
#[cfg(target_os = "macos")]
pub fn user_data_dir() -> PathBuf {
    home_dir().join("Library/Application Support/TaskDeck")
}

/// The per-user data directory, following each platform's own convention.
#[cfg(windows)]
pub fn user_data_dir() -> PathBuf {
    env::var_os("APPDATA")
        .filter(|value| !value.is_empty())
        .map(PathBuf::from)
        .unwrap_or_else(home_dir)
        .join("TaskDeck")
}

/// The per-user data directory, following each platform's own convention
/// (XDG on Linux and the other unixes).
#[cfg(all(unix, not(target_os = "macos")))]
pub fn user_data_dir() -> PathBuf {
    env::var_os("XDG_DATA_HOME")
        .map(PathBuf::from)
        .filter(|path| path.is_absolute())
        .unwrap_or_else(|| home_dir().join(".local/share"))
        .join("taskdeck")
}

fn home_dir() -> PathBuf {
    #[cfg(windows)]
    let key = "USERPROFILE";
    #[cfg(not(windows))]
    let key = "HOME";

    env::var_os(key)
        .filter(|value| !value.is_empty())
        .map(PathBuf::from)
        // No home directory at all is pathological; the working directory at
        // least keeps the app running instead of writing to the filesystem root.
        .unwrap_or_else(|| PathBuf::from("."))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn dirs_at(root: &Path) -> AppDirs {
        AppDirs {
            root: root.to_path_buf(),
            data: root.join(DATA_DIR_NAME),
            images: root.join(IMAGES_DIR_NAME),
        }
    }

    #[test]
    fn image_path_confines_to_images_dir() {
        let root = PathBuf::from("/opt/taskdeck");
        let dirs = dirs_at(&root);
        let images = root.join(IMAGES_DIR_NAME);

        // Ordinary names resolve directly under images/.
        assert_eq!(dirs.image_path("pic.png"), Some(images.join("pic.png")));

        // Traversal and absolute paths are reduced to their final component, so
        // they can't escape images/.
        assert_eq!(dirs.image_path("../../etc/passwd"), Some(images.join("passwd")));
        assert_eq!(dirs.image_path("/etc/passwd"), Some(images.join("passwd")));
        assert_eq!(dirs.image_path("sub/dir/p.png"), Some(images.join("p.png")));
        // A trailing separator is ignored — the final named component is kept.
        assert_eq!(dirs.image_path("sub/"), Some(images.join("sub")));

        // Names with no usable file component are rejected.
        assert_eq!(dirs.image_path(""), None);
        assert_eq!(dirs.image_path(".."), None);
    }

    #[test]
    #[cfg(windows)]
    fn image_path_handles_windows_separators() {
        // `Path::file_name` only treats `\` as a separator on Windows, so this
        // is asserted where it actually holds.
        let dirs = dirs_at(Path::new("C:\\TaskDeck"));
        assert_eq!(
            dirs.image_path(r"..\..\win.png"),
            Some(dirs.images.join("win.png"))
        );
    }

    #[test]
    fn resolve_creates_both_folders_under_an_explicit_root() {
        let tmp = tempfile::tempdir().unwrap();
        let root = tmp.path().join("home");

        // SAFETY: single-threaded test process; no other thread reads the env.
        unsafe { env::set_var("TASKDECK_HOME", &root) };
        let dirs = AppDirs::resolve();
        unsafe { env::remove_var("TASKDECK_HOME") };

        assert_eq!(dirs.root, root);
        assert!(dirs.data.is_dir(), "data dir should be created");
        assert!(dirs.images.is_dir(), "images dir should be created");
        assert_eq!(dirs.config_file(), root.join(DATA_DIR_NAME).join("userconfig.toml"));
    }

    #[test]
    fn cargo_project_root_only_matches_a_real_target_dir() {
        let tmp = tempfile::tempdir().unwrap();
        let project = tmp.path();
        let exe_dir = project.join("target").join("debug");
        fs::create_dir_all(&exe_dir).unwrap();

        // No Cargo.toml yet: this is just a folder two levels down, not a
        // checkout, so it must not be adopted as the root.
        assert_eq!(cargo_project_root(&exe_dir), None);

        fs::write(project.join("Cargo.toml"), "[package]\n").unwrap();
        assert_eq!(cargo_project_root(&exe_dir).as_deref(), Some(project));

        // A folder that isn't under `target/` never matches.
        let elsewhere = project.join("bin").join("release");
        fs::create_dir_all(&elsewhere).unwrap();
        assert_eq!(cargo_project_root(&elsewhere), None);
    }

    #[test]
    fn macos_bundle_layout_is_recognised() {
        assert!(is_inside_macos_bundle(Path::new("/Applications/TaskDeck.app/Contents/MacOS")));
        assert!(!is_inside_macos_bundle(Path::new("/Applications/TaskDeck.app/Contents")));
        assert!(!is_inside_macos_bundle(Path::new("/usr/local/bin")));
    }

    #[test]
    fn user_data_dir_is_absolute_and_named() {
        let dir = user_data_dir();
        // Only meaningful when a home directory exists, which it does in CI and
        // on every developer machine; the "." fallback is the pathological case.
        if dir.starts_with(".") {
            return;
        }
        assert!(dir.is_absolute(), "{dir:?} should be absolute");
        let name = dir.file_name().and_then(|n| n.to_str()).unwrap_or_default();
        assert!(
            name.eq_ignore_ascii_case("taskdeck"),
            "{dir:?} should end in a TaskDeck-specific folder"
        );
    }
}
