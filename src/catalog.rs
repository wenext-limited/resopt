use crate::filesystem::{contained_file, hash};
use anyhow::{Context, Result, ensure};
use serde::{Deserialize, Serialize};
use std::{
    collections::BTreeMap,
    fs,
    path::{Path, PathBuf},
};
use walkdir::WalkDir;
use xcassets::Node;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Asset {
    /// Relative to the inventory root. Only files referenced by catalog JSON.
    pub path: PathBuf,
    pub bytes: u64,
    pub eligible: bool,
    pub reason: Option<String>,
    pub contents_path: PathBuf,
    pub contents_sha256: String,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct Inventory {
    pub schema_version: u32,
    pub root: PathBuf,
    pub catalogs: usize,
    pub assets: Vec<Asset>,
    pub diagnostics: Vec<String>,
}

/// Discover catalogs recursively without following symlinks or build caches.
/// This inventories disk resources; it does not prove target membership.
pub fn scan(root: impl AsRef<Path>) -> Result<Inventory> {
    scan_with_options(root, crate::ScanOptions::default())
}

pub fn scan_with_options(root: impl AsRef<Path>, options: crate::ScanOptions) -> Result<Inventory> {
    let root = fs::canonicalize(root.as_ref()).context("resolving project root")?;
    let filter = crate::scan_options::ScanFilter::new(&root, options)?;
    scan_filtered(&root, &filter)
}

pub(crate) fn scan_filtered(
    root: &Path,
    filter: &crate::scan_options::ScanFilter,
) -> Result<Inventory> {
    let root = root.to_path_buf();
    ensure!(root.is_dir(), "scan root must be a directory");
    let mut inventory = Inventory {
        schema_version: 1,
        root: root.clone(),
        catalogs: 0,
        assets: vec![],
        diagnostics: filter.diagnostics.clone(),
    };
    let mut walk = WalkDir::new(&root).follow_links(false).into_iter();
    while let Some(entry) = walk.next() {
        let entry = match entry {
            Ok(entry) => entry,
            Err(error) => {
                inventory.diagnostics.push(error.to_string());
                continue;
            }
        };
        if !filter.allows(entry.path()) {
            walk.skip_current_dir();
            continue;
        }
        if !entry.file_type().is_dir() {
            continue;
        }
        if entry.depth() > 0 && excluded(entry.file_name().to_str().unwrap_or("")) {
            walk.skip_current_dir();
            continue;
        }
        if entry
            .path()
            .extension()
            .is_some_and(|ext| ext == "xcassets")
        {
            walk.skip_current_dir();
            inventory.catalogs += 1;
            // The catalog parser may traverse child directories; reject catalogs
            // containing symlinks before handing them to the parser.
            let unsafe_tree = WalkDir::new(entry.path())
                .follow_links(false)
                .into_iter()
                .any(|child| child.map_or(true, |child| child.file_type().is_symlink()));
            if unsafe_tree {
                inventory.diagnostics.push(format!(
                    "skipped catalog with symlinks or unreadable entries: {}",
                    entry.path().display()
                ));
                continue;
            }
            match xcassets::parse_catalog(entry.path()) {
                Ok(report) => {
                    for diagnostic in report.diagnostics {
                        inventory.diagnostics.push(format!(
                            "{}: {}",
                            diagnostic.path.display(),
                            diagnostic.message
                        ));
                    }
                    visit(&report.catalog.children, entry.path(), &mut inventory)?;
                }
                Err(error) => inventory.diagnostics.push(error.to_string()),
            }
        }
    }
    // One filename may serve several renditions. Any exclusion wins.
    let mut unique: BTreeMap<PathBuf, Asset> = BTreeMap::new();
    for asset in inventory.assets.drain(..) {
        if !filter.allows(&root.join(&asset.path))
            || !filter.allows(&root.join(&asset.contents_path))
        {
            continue;
        }
        match unique.get(&asset.path) {
            Some(previous) if !previous.eligible => {}
            _ => {
                unique.insert(asset.path.clone(), asset);
            }
        }
    }
    inventory.assets = unique.into_values().collect();
    inventory.diagnostics.sort();
    Ok(inventory)
}

pub(crate) fn excluded(name: &str) -> bool {
    matches!(
        name,
        ".git"
            | ".worktrees"
            | ".worktree"
            | ".build"
            | ".swiftpm"
            | ".resopt"
            | "target"
            | "build"
            | "DerivedData"
            | "Pods"
            | "Carthage"
            | "node_modules"
    )
}

fn visit(nodes: &[Node], catalog: &Path, inventory: &mut Inventory) -> Result<()> {
    for node in nodes {
        match node {
            Node::Group(group) => visit(&group.children, catalog, inventory)?,
            Node::ImageSet(set) => {
                visit_set(&set.contents, &set.relative_path, false, catalog, inventory)?
            }
            Node::AppIconSet(set) => {
                visit_set(&set.contents, &set.relative_path, true, catalog, inventory)?
            }
            Node::Opaque(node) => inventory.diagnostics.push(format!(
                "unsupported catalog node: {}",
                catalog.join(&node.relative_path).display()
            )),
            Node::ColorSet(_) => {}
        }
    }
    Ok(())
}

fn visit_set<T: Serialize + serde::de::DeserializeOwned + PartialEq>(
    contents: &Option<T>,
    relative: &Path,
    app_icon: bool,
    catalog: &Path,
    inventory: &mut Inventory,
) -> Result<()> {
    let Some(contents) = contents else {
        return Ok(());
    };
    let raw = serde_json::to_value(contents)?;
    let Some(images) = raw.get("images").and_then(|value| value.as_array()) else {
        return Ok(());
    };
    let directory = catalog.join(relative);
    let contents_path = directory.join("Contents.json");
    let relative_contents = contents_path.strip_prefix(&inventory.root)?.to_path_buf();
    let contents_bytes = fs::read(&contents_path)?;
    ensure!(
        serde_json::from_slice::<T>(&contents_bytes)? == *contents,
        "catalog changed while scanning: {}",
        contents_path.display()
    );
    let contents_hash = hash(&contents_bytes);
    let special = if app_icon {
        Some("app_icon")
    } else if has_key(&raw, "resizing") {
        Some("resizing")
    } else {
        None
    };
    for image in images {
        let Some(filename) = image.get("filename").and_then(|value| value.as_str()) else {
            continue;
        };
        // Catalog rendition filenames must be a single basename.
        let filename_path = Path::new(filename);
        if filename_path.components().count() != 1
            || !matches!(
                filename_path.components().next(),
                Some(std::path::Component::Normal(_))
            )
        {
            inventory.diagnostics.push(format!(
                "unsafe rendition filename in {}: {filename}",
                contents_path.display()
            ));
            continue;
        }
        let path = directory
            .join(filename)
            .strip_prefix(&inventory.root)?
            .to_path_buf();
        let source = match contained_file(&inventory.root, &path) {
            Ok(source) => source,
            Err(error) => {
                inventory.diagnostics.push(error.to_string());
                continue;
            }
        };
        let reason = special.or_else(|| {
            if filename_path
                .extension()
                .is_some_and(|ext| ext.eq_ignore_ascii_case("png"))
            {
                None
            } else {
                Some("unsupported_format")
            }
        });
        inventory.assets.push(Asset {
            path,
            bytes: fs::metadata(source)?.len(),
            eligible: reason.is_none(),
            reason: reason.map(str::to_string),
            contents_path: relative_contents.clone(),
            contents_sha256: contents_hash.clone(),
        });
    }
    Ok(())
}

fn has_key(value: &serde_json::Value, key: &str) -> bool {
    match value {
        serde_json::Value::Object(values) => {
            values.contains_key(key) || values.values().any(|value| has_key(value, key))
        }
        serde_json::Value::Array(values) => values.iter().any(|value| has_key(value, key)),
        _ => false,
    }
}
