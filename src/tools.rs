//! Optional external tools. resopt never installs anything; it reports what is
//! missing, what that tool would enable, and how to install it.
use serde::Serialize;
use std::{
    path::PathBuf,
    process::{Command, Stdio},
};

#[derive(Debug, Clone, Serialize)]
pub struct Tool {
    pub name: &'static str,
    pub available: bool,
    pub path: Option<PathBuf>,
    /// What resopt uses the tool for.
    pub purpose: &'static str,
    /// Installation guidance for this platform, shown only when missing.
    pub install: &'static str,
}

fn runs(program: &str, argument: &str) -> bool {
    Command::new(program)
        .arg(argument)
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .status()
        .is_ok_and(|status| status.success())
}

fn install_hint(macos: &'static str, linux: &'static str, windows: &'static str) -> &'static str {
    if cfg!(target_os = "macos") {
        macos
    } else if cfg!(windows) {
        windows
    } else {
        linux
    }
}

/// Locate `aapt2` on PATH or in the newest installed Android SDK build-tools.
pub(crate) fn find_aapt2() -> Option<PathBuf> {
    let executable = if cfg!(windows) { "aapt2.exe" } else { "aapt2" };
    if runs(executable, "version") {
        return Some(PathBuf::from(executable));
    }
    let mut roots: Vec<PathBuf> = ["ANDROID_HOME", "ANDROID_SDK_ROOT"]
        .iter()
        .filter_map(|name| std::env::var_os(name).filter(|v| !v.is_empty()))
        .map(PathBuf::from)
        .collect();
    if let Some(home) = std::env::var_os("HOME").filter(|v| !v.is_empty()) {
        roots.push(PathBuf::from(&home).join("Library/Android/sdk"));
        roots.push(PathBuf::from(&home).join("Android/Sdk"));
    }
    if let Some(local) = std::env::var_os("LOCALAPPDATA").filter(|v| !v.is_empty()) {
        roots.push(PathBuf::from(local).join("Android").join("Sdk"));
    }
    for root in roots {
        let Ok(entries) = std::fs::read_dir(root.join("build-tools")) else {
            continue;
        };
        let mut versions: Vec<PathBuf> = entries.filter_map(|e| Some(e.ok()?.path())).collect();
        versions.sort();
        for version in versions.into_iter().rev() {
            let candidate = version.join(executable);
            if candidate.is_file() && runs(&candidate.to_string_lossy(), "version") {
                return Some(candidate);
            }
        }
    }
    None
}

pub fn detect() -> Vec<Tool> {
    let aapt2 = find_aapt2();
    let simple = |name: &'static str, flag: &str, purpose, install| {
        let available = runs(name, flag);
        Tool {
            name,
            available,
            path: available.then(|| PathBuf::from(name)),
            purpose,
            install,
        }
    };
    vec![
        simple(
            "ffprobe",
            "-version",
            "Reports codec, bitrate and duration for audio and video resources.",
            install_hint(
                "brew install ffmpeg",
                "Install ffmpeg with your package manager, e.g. `sudo apt install ffmpeg`.",
                "winget install Gyan.FFmpeg",
            ),
        ),
        Tool {
            name: "aapt2",
            available: aapt2.is_some(),
            path: aapt2,
            purpose: "Compiles migrated Android resources to confirm they are accepted by the build tools.",
            install: "Install Android SDK Build-Tools (Android Studio → SDK Manager) and set ANDROID_HOME.",
        },
    ]
}
