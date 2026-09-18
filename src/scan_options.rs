use anyhow::Result;
use ignore::WalkBuilder;
use serde::{Deserialize, Serialize};
use std::{
    collections::HashSet,
    path::{Path, PathBuf},
};

#[derive(Debug, Default, Clone, Copy, Serialize, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct ScanOptions {
    /// Include paths matched by Git ignore rules. Fixed build/VCS exclusions remain.
    pub include_ignored: bool,
}

pub(crate) struct ScanFilter {
    paths: HashSet<PathBuf>,
    pub diagnostics: Vec<String>,
}
impl ScanFilter {
    pub fn new(root: &Path, options: ScanOptions) -> Result<Self> {
        let mut paths = HashSet::new();
        let mut diagnostics = Vec::new();
        let mut builder = WalkBuilder::new(root);
        builder
            .hidden(false)
            .ignore(false)
            .follow_links(false)
            .git_ignore(!options.include_ignored)
            .git_exclude(!options.include_ignored)
            .git_global(!options.include_ignored)
            .require_git(false)
            .filter_entry(|e| {
                e.depth() == 0
                    || !e.file_type().is_some_and(|t| t.is_dir())
                    || !hard_excluded(e.file_name().to_str().unwrap_or(""))
            });
        for entry in builder.build() {
            match entry {
                Ok(entry) => {
                    paths.insert(entry.into_path());
                }
                Err(error) => diagnostics.push(error.to_string()),
            }
        }
        if !options.include_ignored {
            add_tracked_paths(root, &mut paths);
        }
        Ok(Self { paths, diagnostics })
    }
    pub fn allows(&self, path: &Path) -> bool {
        self.paths.contains(path)
    }
    pub fn paths(&self) -> impl Iterator<Item = &PathBuf> {
        self.paths.iter()
    }
}

pub(crate) fn hard_excluded(name: &str) -> bool {
    (crate::catalog::excluded(name) && !matches!(name, "Pods" | "Carthage" | "node_modules"))
        || matches!(
            name,
            ".github" | ".codex" | ".claude" | ".agents" | ".idea" | ".vscode"
        )
}

// Git ignore rules apply to untracked paths. Preserve force-added assets when
// an actual Git index is available; plain directories still use ignore files.
fn add_tracked_paths(root: &Path, paths: &mut HashSet<PathBuf>) {
    let run = |args: &[&str]| {
        std::process::Command::new("git")
            .arg("-c")
            .arg("core.fsmonitor=false")
            .arg("-C")
            .arg(root)
            .args(args)
            .env("GIT_OPTIONAL_LOCKS", "0")
            .output()
    };
    let Ok(prefix) = run(&["rev-parse", "--show-prefix"]) else {
        return;
    };
    if !prefix.status.success() {
        return;
    }
    let Ok(prefix) = String::from_utf8(prefix.stdout) else {
        return;
    };
    let prefix = prefix.trim_end_matches(['\r', '\n']);
    let Ok(files) = run(&[
        "ls-files",
        "--cached",
        "--recurse-submodules",
        "--full-name",
        "-z",
        "--",
        ".",
    ]) else {
        return;
    };
    if !files.status.success() {
        return;
    }
    for entry in files.stdout.split(|b| *b == 0).filter(|p| !p.is_empty()) {
        let Ok(entry) = std::str::from_utf8(entry) else {
            continue;
        };
        let Some(relative) = entry.strip_prefix(prefix) else {
            continue;
        };
        let relative = Path::new(relative);
        if relative
            .components()
            .any(|p| !matches!(p, std::path::Component::Normal(_)))
            || relative
                .parent()
                .into_iter()
                .flat_map(Path::components)
                .any(|p| hard_excluded(&p.as_os_str().to_string_lossy()))
        {
            continue;
        }
        let mut path = root.join(relative);
        while path.starts_with(root) {
            paths.insert(path.clone());
            if path == root || !path.pop() {
                break;
            }
        }
    }
}
