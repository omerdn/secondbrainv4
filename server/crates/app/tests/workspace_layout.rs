//! Guards the Cargo workspace layout (T0.1.2, ARCHITECTURE §3, ADR-0007).
//!
//! - The set of crates under `server/crates` must match the module list in ARCHITECTURE §3.
//!   Adding or removing a module is an architectural change and must update this list.
//! - Every crate must inherit the shared lint and package settings, otherwise
//!   `cargo clippy -- -D warnings` would silently stop covering it.

// Test-only file: panicking on unexpected I/O failure is the desired behaviour, including in
// helper functions (clippy's `allow-*-in-tests` only covers `#[test]` bodies).
#![allow(clippy::expect_used, clippy::unwrap_used)]

use std::collections::BTreeSet;
use std::fs;
use std::path::{Path, PathBuf};

/// Modules listed in ARCHITECTURE §3 (`server/crates/*`).
const EXPECTED_CRATES: &[&str] = &[
    "access",
    "app",
    "db",
    "goals",
    "identity",
    "items",
    "jobs",
    "kernel",
    "notebooks",
    "notifications",
    "realtime",
    "recurrence",
    "routines",
    "search",
    "sessions",
    "social",
    "tasks",
    "teams",
    "tracking",
    "updates",
];

fn crates_dir() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .expect("app crate lives in server/crates")
        .to_path_buf()
}

fn crate_dirs() -> Vec<PathBuf> {
    let mut dirs: Vec<PathBuf> = fs::read_dir(crates_dir())
        .expect("read server/crates")
        .map(|entry| entry.expect("dir entry").path())
        .filter(|path| path.join("Cargo.toml").is_file())
        .collect();
    dirs.sort();
    dirs
}

fn manifest(dir: &Path) -> String {
    fs::read_to_string(dir.join("Cargo.toml")).expect("read Cargo.toml")
}

#[test]
fn crates_match_architecture_module_list() {
    let actual: BTreeSet<String> = crate_dirs()
        .iter()
        .map(|dir| dir.file_name().unwrap().to_string_lossy().into_owned())
        .collect();
    let expected: BTreeSet<String> = EXPECTED_CRATES.iter().map(|s| s.to_string()).collect();
    assert_eq!(
        actual, expected,
        "server/crates diverged from ARCHITECTURE §3"
    );
}

#[test]
fn every_crate_inherits_workspace_lints() {
    for dir in crate_dirs() {
        let manifest = manifest(&dir);
        assert!(
            manifest.contains("[lints]\nworkspace = true"),
            "{} must set `[lints] workspace = true`",
            dir.display()
        );
    }
}

#[test]
fn every_crate_inherits_workspace_package_settings() {
    for dir in crate_dirs() {
        let manifest = manifest(&dir);
        for key in ["edition", "rust-version", "publish"] {
            assert!(
                manifest.contains(&format!("{key}.workspace = true")),
                "{} must set `{key}.workspace = true`",
                dir.display()
            );
        }
    }
}

#[test]
fn package_name_matches_directory() {
    for dir in crate_dirs() {
        let name = dir.file_name().unwrap().to_string_lossy().into_owned();
        assert!(
            manifest(&dir).contains(&format!("name = \"{name}\"")),
            "{} package name must match its directory",
            dir.display()
        );
    }
}
