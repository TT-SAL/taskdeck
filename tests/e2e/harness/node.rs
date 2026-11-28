//! Finding Node 22 or later, as `relay/ci.sh` does: `TD_NODE`, then the
//! `node` of the `PATH`, then the runner's tool cache. The standby needs
//! `node:sqlite`, which Node 22 has.

use std::path::{Path, PathBuf};
use std::process::Command;

const MIN_MAJOR: u32 = 22;

fn major_of(node: &Path) -> Option<u32> {
    let out = Command::new(node).arg("--version").output().ok()?;
    let text = String::from_utf8(out.stdout).ok()?;
    text.trim().strip_prefix('v')?.split('.').next()?.parse().ok()
}

fn path_candidates() -> Vec<PathBuf> {
    let path = std::env::var_os("PATH").unwrap_or_default();
    std::env::split_paths(&path)
        .map(|d| d.join("node"))
        .filter(|p| p.is_file())
        .collect()
}

fn tool_cache_candidates() -> Vec<PathBuf> {
    let cache = std::env::var_os("RUNNER_TOOL_CACHE").map_or_else(|| PathBuf::from("/opt/hostedtoolcache"), PathBuf::from);
    let Ok(dir) = std::fs::read_dir(cache.join("node")) else {
        return Vec::new();
    };
    let mut found: Vec<(u32, PathBuf)> = dir
        .filter_map(Result::ok)
        .filter_map(|e| {
            let major: u32 = e.file_name().to_str()?.split('.').next()?.parse().ok()?;
            let node = e.path().join("x64/bin/node");
            (major >= MIN_MAJOR && node.is_file()).then_some((major, node))
        })
        .collect();
    found.sort();
    found.into_iter().rev().map(|(_, p)| p).collect()
}

/// The path of a `node` of version 22 or later; panics with what was tried.
pub fn find_node() -> PathBuf {
    if let Some(given) = std::env::var_os("TD_NODE") {
        let given = PathBuf::from(given);
        match major_of(&given) {
            Some(m) if m >= MIN_MAJOR => return given,
            other => panic!("TD_NODE={} is not Node {MIN_MAJOR} or later (major {other:?})", given.display()),
        }
    }
    let mut tried = Vec::new();
    for node in path_candidates().into_iter().chain(tool_cache_candidates()) {
        match major_of(&node) {
            Some(m) if m >= MIN_MAJOR => return node,
            other => tried.push(format!("{} (major {other:?})", node.display())),
        }
    }
    panic!("the standby needs Node {MIN_MAJOR} or later (node:sqlite); set TD_NODE. Tried: {tried:?}");
}
