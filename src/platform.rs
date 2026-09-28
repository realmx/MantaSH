//! OS-specific paths and shell discovery.
use anyhow::{Context, Result};
use std::path::{Path, PathBuf};

/// Native interface family; missing fonts are handled by the platform fallback chain.
pub fn ui_font() -> &'static str {
    if cfg!(windows) {
        "Segoe UI"
    } else {
        ".SystemUIFont"
    }
}
/// Available system monospace default.
pub fn terminal_font() -> &'static str {
    if cfg!(windows) { "Consolas" } else { "Menlo" }
}
/// User's real home directory, used only as a local terminal starting path.
pub fn home_directory() -> String {
    directories::BaseDirs::new()
        .map(|d| d.home_dir().to_string_lossy().into_owned())
        .unwrap_or_else(|| ".".into())
}
/// Locate an executable in PATH without running a shell.
pub fn find_executable(name: &str) -> Option<PathBuf> {
    let path = Path::new(name);
    if path.is_absolute() && path.is_file() {
        return Some(path.into());
    }
    std::env::split_paths(&std::env::var_os("PATH").unwrap_or_default())
        .flat_map(|dir| {
            let mut paths = vec![dir.join(name)];
            if cfg!(windows) && !name.ends_with(".exe") {
                paths.push(dir.join(format!("{name}.exe")));
            }
            paths
        })
        .find(|p| p.is_file())
}
/// Prefer the user's configured Unix shell or available Windows PowerShell.
pub fn default_shell() -> String {
    if cfg!(windows) {
        find_executable("pwsh")
            .or_else(|| find_executable("powershell"))
            .map(|p| p.to_string_lossy().into_owned())
            .unwrap_or_else(|| "cmd.exe".into())
    } else {
        std::env::var("SHELL")
            .ok()
            .filter(|s| Path::new(s).is_file())
            .unwrap_or_else(|| "/bin/sh".into())
    }
}
/// Explicit shell choices shown in settings; an unavailable custom path remains editable.
pub fn shells() -> Vec<String> {
    let mut found = vec![default_shell()];
    let names: &[&str] = if cfg!(windows) {
        &["pwsh", "powershell", "cmd", "bash"]
    } else {
        &["zsh", "bash", "sh", "fish"]
    };
    for name in names {
        if let Some(p) = find_executable(name) {
            let s = p.to_string_lossy().into_owned();
            if !found.contains(&s) {
                found.push(s);
            }
        }
    }
    found
}
/// Independently namespaced data directory, overrideable only for explicit testing.
pub fn data_directory() -> Result<PathBuf> {
    if let Some(path) = data_override() {
        return Ok(PathBuf::from(path));
    }
    directories::ProjectDirs::from("app", "MantaSH", "MantaSH")
        .map(|d| d.data_local_dir().to_owned())
        .context("Cannot locate the MantaSH application data directory")
}
/// Explicit data-directory override, used only for isolated testing and acceptance runs.
pub fn data_override() -> Option<std::ffi::OsString> {
    std::env::var_os("MANTASH_DATA_DIR")
}
