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
/// Locate Git for Windows' Bash even when Git was installed outside PATH.
#[cfg(windows)]
fn git_bash() -> Option<PathBuf> {
    let mut candidates = Vec::new();
    if let Some(program_files) = std::env::var_os("ProgramFiles") {
        candidates.push(
            PathBuf::from(&program_files)
                .join("Git")
                .join("bin")
                .join("bash.exe"),
        );
        candidates.push(
            PathBuf::from(program_files)
                .join("Git")
                .join("usr")
                .join("bin")
                .join("bash.exe"),
        );
    }
    if let Some(local_app_data) = std::env::var_os("LOCALAPPDATA") {
        candidates.push(
            PathBuf::from(local_app_data)
                .join("Programs")
                .join("Git")
                .join("bin")
                .join("bash.exe"),
        );
    }
    candidates.into_iter().find(|path| path.is_file())
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
    #[cfg(windows)]
    if let Some(path) = git_bash() {
        let path = path.to_string_lossy().into_owned();
        if !found.contains(&path) {
            found.push(path);
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

/// Normalize a local terminal directory before it is persisted or passed to a Windows PTY.
/// Git Bash reports MSYS paths such as `/c/Users/name`; Windows-native shells do not.
pub fn normalize_local_directory(directory: &str, shell: &str) -> String {
    #[cfg(windows)]
    {
        let basename = Path::new(shell)
            .file_stem()
            .unwrap_or_default()
            .to_string_lossy()
            .to_ascii_lowercase();
        let bytes = directory.as_bytes();
        if (basename == "bash" || basename == "git-bash")
            && bytes.len() >= 3
            && bytes[0] == b'/'
            && bytes[2] == b'/'
            && bytes[1].is_ascii_alphabetic()
        {
            let drive = (bytes[1] as char).to_ascii_uppercase();
            return format!("{drive}:\\{}", &directory[3..].replace('/', "\\"));
        }
    }
    directory.to_string()
}
