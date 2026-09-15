# resopt

Reviewable resource optimization for Apple projects, as a Rust library and CLI.

`resopt` discovers referenced files in `.xcassets`, stages optimized candidates,
verifies their decoded pixels and metadata, and applies exactly the reviewed
bytes with recoverable originals. It uses `xcassets` for parsing and embeds
Oxipng for PNG optimization. Numi remains an independent code generator.

## First release scope

- Recursive catalog discovery, including catalogs inside Swift packages.
- Static PNG recompression, preserving file names, decoded samples, bit depth,
  color type, interlacing, palette, and non-IDAT chunks.
- Human-readable reports and JSON output.
- Explicit TOML policy for encoder effort and minimum worthwhile savings.
- Saved plans with source/catalog hashes, candidate artifacts, and originals.
- Apply/restore with complete batch preflight, per-file atomic replacement,
  locks, and a journal.

**Savings are source-file bytes.** This release does not measure compiled
`Assets.car`, IPA, App Store download size, decode speed, or memory use. Xcode
can recompress catalog inputs, so source savings may not become app savings.

## Install from source

```sh
cargo install --path . --locked
resopt doctor
```

Rust 1.88+ and a C compiler are required to build the embedded libdeflate
dependency. No separate Oxipng, Sips, or FFmpeg executable is required for the
implemented PNG workflow. `doctor` reports optional media tools for future
backends but does not install or use them for optimization.

## Workflow

```sh
# Read-only inventory. No encodes or project writes.
resopt scan /path/to/project
resopt scan /path/to/project --json > inventory.json

# Stage candidates; the output directory must not exist and its parent must exist.
resopt plan /path/to/project --out /tmp/resopt-review

# Inspect the ranked report, plan.json, originals/, and candidates/.
# Supplying apply is the explicit decision to write the reviewed changes.
resopt apply /tmp/resopt-review

# Repeat apply is a no-op for files already matching their candidates.
# Keep the review directory for as long as rollback is needed.
resopt restore /tmp/resopt-review
```

`--json` is available on all commands. Reports go to stdout; human diagnostics
and errors go to stderr. A JSON scan/plan includes diagnostics in the document.
Successful commands return 0, operation failures return 1, and invalid CLI usage
returns 2. A scan/plan can succeed with skipped files: inspect `diagnostics` and
`skipped`; successful execution does not prove every resource was handled.

### Policy

```sh
resopt plan /path/to/project --policy resopt.example.toml --out /tmp/resopt-review
```

```toml
png_level = 2
min_input_bytes = 51200
min_savings_bytes = 1024
min_savings_percent = 1.0
```

Defaults match this example. Policies are loaded only when `--policy` is supplied.
`png_level` controls optimization effort, not visual quality. Candidates must be
strictly smaller and meet both savings thresholds. Unknown policy fields are
errors, so a future lossy `quality = 75` cannot silently enable the wrong behavior.
Lossy compression is not implemented in this release.

### Composing with Numi

Run Numi from the project root if your workflow wants regeneration:

```sh
resopt apply /tmp/resopt-review && numi generate --workspace
```

This PNG backend retains filenames and asset names, so regenerated accessors
normally remain unchanged. There is no Numi dependency or implicit generator run.

## Safety contract

Planning performs optimization in memory and writes only into a new plan
directory. Every selected candidate must pass an independent PNG decode and
byte-for-byte decoded-sample comparison. All non-IDAT chunks, and their ordering
relative to the first IDAT, must match. This intentionally rejects some valid
optimizations that rewrite metadata. Fully transparent pixels retain their RGB
values; bit-depth, palette, and color-type reductions are disabled.

Applying checks all entries before changing any source. It verifies current
catalog eligibility, `Contents.json` hashes, source hashes, artifact hashes,
sizes, metadata, and decoded samples. It rechecks each entry just before writing.
The source can match either the original or the planned output, allowing repeat
application and recovery after partial progress. Restore uses the same rules and
refuses to overwrite subsequent user edits or changed catalog metadata.

Each replacement uses a temporary sibling file and atomic rename, preserving
ordinary file permissions. A multi-file batch is **not** one atomic transaction.
Originals are staged before application, and `journal.jsonl` records started and
completed replacements. If an IO error interrupts a batch, retain the review
directory and run `restore`. Restore can recognize partial progress from hashes
even if the last journal event was not written. Filesystem timestamps, extended
attributes, and hard-link relationships are not preserved.

Apply/restore create temporary `.lock` (plan) and `.resopt.lock` (project) files.
After a terminated process, remove these only after confirming it is no longer
running. Do not edit the source tree, change symlinks, or run overlapping-root
optimizations concurrently. This is a local developer tool, not a security
sandbox for concurrently hostile filesystem writers. Plans retain an absolute
project root; regenerate a plan after moving the project.

## Discovery and exclusions

- Only catalog-referenced rendition files are inventoried. Loose files are not
  yet scanned, and disk discovery does not prove build-target membership.
- `.git`, `.worktrees`, `.worktree`, `.build`, `.swiftpm`, `.resopt`, `target`, `build`, `DerivedData`,
  `Pods`, `Carthage`, and `node_modules` directories are skipped.
- AppIcons, `resizing` metadata, and non-PNG renditions are reported but skipped.
- APNGs, malformed PNGs, and candidates that fail verification are skipped with
  a reason. Planning supports at most 64 MiB input / 256 MiB decoded data per PNG.
- Catalogs containing symlinks or unreadable entries are skipped. Referenced
  filename traversal, artifact symlinks, and missing files are rejected.
- Unsupported catalog node types produce diagnostics; no unused-asset deletion,
  source-code rewrite, deployment-target inference, or extension conversion occurs.

## Library

```rust,no_run
use resopt::{Policy, create_plan, apply, restore};

let plan = create_plan("/path/to/project", "/tmp/resopt-review", Policy::default())?;
println!("{} candidates, {} source bytes", plan.candidates.len(), plan.savings_bytes());
// After reviewing the persisted plan:
let report = apply("/tmp/resopt-review")?;
restore("/tmp/resopt-review")?;
# Ok::<(), anyhow::Error>(())
```

The plan schema is versioned (`schema_version = 1`). `read_plan` validates its
structure; apply/restore additionally validate its artifacts and current project.
Treat the plan directory as one unit; do not copy `plan.json` alone.

## Development

```sh
cargo fmt --all --check
cargo clippy --all-targets -- -D warnings
cargo test --all-targets
cargo test --doc
```

Tests cover real encode/decode round trips, 16-bit samples, hidden RGB under
transparency, metadata, skip rules, Unicode paths, stale plans, corrupted and
rehashed unsafe candidates, symlinks, locks, partial application, rollback, and
CLI JSON. CI is configured for Linux, macOS, and Windows; local verification
does not establish that remote CI has passed.

## Next backends

JPEG optimization, approved lossy image candidates, HEIC conversion, ordinary
audio/video optimization, and target-aware discovery are future work. Specialized
effect containers (SVGA/VAP/etc.) require their own format contracts. Compiled
catalog measurements should be a separate opt-in validation stage.

## License

MIT. Dependencies retain their own licenses. No Imagequant/GPL dependency is
included in this initial release.
