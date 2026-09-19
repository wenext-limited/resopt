# Changelog

## 0.6.1 · 2026-09-19

### Fixed
- Images without a smaller candidate (already-optimal PNGs, and WebP files when `--webp` is off) had no thumbnail or preview. Every decoded image now gets a preview, and a live session can open the project's own file at full size.
- Such images now say why nothing was proposed ("WebP candidates are off for this run…") and are labelled "No smaller candidate" instead of "Already optimal".

### Changed
- Interface language follows the browser's preference order, with an explicit "Auto" choice in the selector.

## 0.6.0 · 2026-09-19

### Added
- **One live review page.** `resopt web` streams results while analysis runs, largest files first, and the same page handles review, apply, batch and restore. Analysis can be stopped; unfinished files are marked "Not analyzed".
- **Batch apply and restore-all**, in the page and as `resopt apply <report>` / `resopt restore <report>`: explicit policy (lossless by default; lossy, format changes and each warning kind are opt-in), a preview that must be confirmed, per-file outcomes, cancellation, and recoverable partial completion.
- **Perceptual-score threshold** (`--min-score`, default 80). Candidates below it, like candidates above the Alpha threshold, are kept as warnings: reviewable, excluded from recommended totals, and applicable only after explicit approval of every threshold they miss. Approvals are recorded in the operation journal.
- **Android-aware resources**: source set, type, qualifiers and resource name; `minSdk` detection from Gradle files and version catalogs (`--android-min-sdk` to override); WebP gated by API level; PNG→WebP in `res/` keeps the resource name and blocks same-name collisions; nine-patch, launcher icons and `res/raw` keep their format with pixel-identical PNG optimization; `assets/` path references are migrated; optional AAPT2 compilation of every `res/` candidate with measured compiled sizes; no JPEG/HEIC proposals.
- **Verified lossless WebP** candidates (exact 8-bit RGBA, including color under transparent pixels).
- **SVGA 2.x lossless optimization**: embedded PNGs are recompressed; all other bytes and every pixel are verified unchanged. SVGA 1.x, audio and unknown fields are refused with a reason.
- **Duplicate detection**: identical files, the same picture at different sizes, and near-duplicates, excluding intended scale and density variants.
- **Persistent result cache** with versioned keys, integrity checks and atomic writes (`--no-cache`, `--cache-dir`, `resopt cache [--clear]`); per-phase timings (`analyze --timings`).
- `resopt package-diff` measures the entry-by-entry difference between two APK/AAB/IPA builds.
- Audio and video show codec, duration and bitrate when `ffprobe` is installed. `resopt doctor` reports capabilities and installation guidance for optional tools.
- English interface with Simplified Chinese translation; full-size comparison slider; filters for warnings, duplicates, applied, unsupported and failed resources; sort by lowest score.

### Changed
- The default worker count follows the CPU count (up to 8); decoded pixels in flight are bounded so more workers do not multiply peak memory.
- Report artifacts are no longer fsynced one by one (they are regenerable); on a 7,000-file project this alone reduced analysis from 86 s to 39 s.
- Project locking uses an operating-system file lock. A crashed process can no longer leave a project or plan locked.
- Local server hardening: the page is served only to the launch URL printed in the terminal (it carries a session key and sets an HttpOnly, SameSite=Strict cookie); artifacts and `analysis.json` require that cookie; every API route requires the session token, and state-changing routes also the page's Origin. Artifacts are served only by generated name.
- Files are created atomically during apply and restore, so a crash cannot leave a partial file that blocks recovery.
- Candidates that are not smaller are no longer scored, previewed or written.
- `analysis.json` is schema 2 (additive). Reports from 0.5.x still open.

### Fixed
- One ignored file (for example `.DS_Store`) could hide every asset catalog that sorted after it in the same directory.
- `plan`/`apply` kept refusing to run after an interrupted run left a lock file behind.

## 0.5.0 · 2026-09-18

- `resopt web <project>`: local scanning, progress and review page without uploads or Bun; JPEG/HEIC on macOS, lossless PNG analysis on Linux and Windows.
- The public site became browser-only (WebAssembly); the native debugging host is loopback-only and refuses remote encoding configuration.
- Browser edition: lossless PNG optimization, pixel verification and SSIMULACRA2 in WebAssembly, with themes, pause/resume and ZIP download.
- SSIMULACRA2 score for every candidate (lowest of black, white and gray backdrops); strip-wise scoring for large images.
- Default decoded-pixel limit raised to 16,777,216 with `--max-pixels`; `--png-level`; optional lossless PNG reductions (`--png-reductions`) verified on expanded RGBA16 samples.
- Opt-in WebP candidates, Alpha-warning review with explicit approval, in-run reuse of identical images, and asset-catalog edits through xcassets 0.3.
- Minimum Rust version 1.89.

## 0.2.0 – 0.4.0

- Package renamed to `resopt-cli` (command and library stay `resopt`).
- Whole-project inventory with content sniffing; `analyze` with ImageIO decoding, real transparency detection and JPEG/HEIC comparison at 75/85/95; JSON and HTML reports with previews and RGB/Alpha error.
- Interactive per-image apply and restore, loose-image reference migration, Git ignore support.

## 0.1.0

- Pixel-verified lossless PNG `plan` / `apply` / `restore` for asset catalogs.
