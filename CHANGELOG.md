# Changelog

## 0.8.0 · 2026-09-23

### Added
- **Similarity scores and grouped review.** Each similar-image group is one result row with thumbnails, total size and a score range. Every member is compared directly with the largest reference file; only identical file hashes score 100. Search and pagination preserve whole groups. Scores remain approximate fingerprint agreement, not proof of interchangeability.
- **ZIP resource browsing.** Search/filter package entries, inspect compressed and expanded sizes, and preview embedded images without extracting paths into the project. Inspection enforces input, entry-count, expansion, path and symlink limits.
- **Lossless PNG optimization inside ZIPs.** Eligible packages produce a smaller, same-format ZIP candidate. PNG pixels and metadata, filenames, atlas/config payloads and entry metadata are verified before apply. Apply/restore use the existing recovery journal. Recognized manifests and checksum maps require the package publisher and are not rewritten. A real Ludo ZIP copy saved 383.4 KiB and restored byte-for-byte.
- **VAP previews.** Detect VAP metadata inside MP4 files, reconstruct transparent playback, and scrub frames. Canvas, timing and dynamic-source counts are shown; app-injected overlays are explicitly outside the preview.
- **PAG/TCMP4 previews.** Detect PAG content regardless of the suffix and preview original templates with playback, frame scrubbing, and editable-text/image counts. The pinned offline libpag renderer runs in an opaque-origin sandbox without network or mutation API access. Runtime files and license notices are bundled; no CDN requests are needed.

### Fixed
- Similarity groups no longer chain indirectly matching images into misleading groups.
- Localized opacity changes cannot be hidden by the average alpha difference.
- Matching poster frames no longer imply matching animations.

### Release notes
- PAG/VAP remain inspection and playback features; no animation transcoding is offered.
- ZIP optimization is limited to verified static PNG recompression; nested archives are listed but not recursively rewritten.
- Existing reports remain readable. New similarity scores and resource inspections require re-analysis.
- CLI archives and source-built macOS apps include the PAG runtime's third-party license notices.

## 0.7.0 · 2026-09-19

### Added
- **Comparison modes.** Besides 2-up, the inspector and the full-size dialog can now compare a candidate by **swipe** (a split you drag across the picture), **onion skin** (candidate opacity over the original) and **difference** (a per-pixel difference, amplifiable ×1–×64, with the share of differing pixels and the largest difference). Colour hidden under full transparency does not count as a difference; alpha changes do. The chosen mode is remembered.
- **Near-lossless HEIC** (macOS, on by default, `--no-near-lossless-heic` to skip): HEIC is also tried at the encoder's highest quality, 100, and labelled "near-lossless". It is still a lossy candidate, scored and approved like the others — Apple's encoder has no lossless mode. On real artwork it saved a median 23% where it beat the source, at a median SSIMULACRA2 of 94.
- **Lossy PNG candidates** (on by default, `--no-lossy-png` to skip): PNG sources are also reduced to a 256/128/64-colour palette at qualities 95/85/75 by a built-in, deterministic quantizer (median cut with k-means refinement in an alpha-weighted colour space; no dithering). The file stays a PNG, so names, references, asset catalogs and Android resources are unaffected, and it works on every platform. They are judged like any lossy candidate (perceptual score, Alpha error, explicit approval) and never applied by the default lossless batch policy. Nine-patch, launcher-icon and `res/raw` files are never quantized. On large translucent artwork, expect them to appear as Alpha warnings: a 256-entry palette cannot keep every alpha level within the default 1/255 tolerance.
- **Multi-select and scoped batch actions.** Shift-click selects a range and Option/Alt-click toggles resources; with several selected, **Batch apply** and the new **Restore selected** act on that selection only, through the same policy → plan → confirm flow.
- **macOS app (build from source).** `macos/build-app.sh` builds a small SwiftUI shell that picks a project folder, runs the bundled `resopt web` and shows its loopback page; reports are kept under Application Support so changes stay restorable. Not part of the release downloads yet (distribution needs a Developer ID signature and notarization).
- **SVGA previews and playback.** SVGA files now render: every SVGA row gets a poster thumbnail plus its canvas size, frame rate and frame count, and a live session plays the animation frame by frame with a scrubber. Rendering follows the reference players (bitmap sprites, transforms, clip paths, matte layers, vector shapes and `keep` frames). A file that cannot be rendered is still optimized; the row says why there is no picture.

### Changed
- SVGA optimization now runs on the published [`svga`](https://crates.io/crates/svga) crate, extracted from resopt's own implementation. Results on real files are byte-for-byte unchanged. Embedded images are edited by position (svga 0.1.1), so an image stored under a key that is not valid UTF-8 is optimized too.
- WebP candidates are compared by default for loose files and Android resources; pass `--no-webp` to skip them. `--webp` is still accepted and has no effect.
- **JSON files are no longer listed as resources.** Catalog `Contents.json`, configuration and other JSON data buried real assets under thousands of rows. A Lottie animation (JSON with Lottie's version, frame-rate and frame-range keys up front) is still listed, as an animation.
- **Faster analysis.** Lossy qualities are tried in ascending order, and once one overshoots the original by a safe margin the higher — and most expensive — qualities of that format are not encoded. The report says so on those candidates. The margins (5%, and 25% before skipping HEIC quality 100) come from 10,634 real encodes, where size was non-monotonic five times and never by more than 13%; with them, a 2,730-resource project produced exactly the same usable candidates in 129 s instead of 206 s. Palette sizes for lossy PNG are exempt, because PNG size is not monotonic in palette size. The PNG quantizer decodes and builds its histogram once for all palette sizes. Per-format encode timings are reported by `--timings`.

### Fixed
- A project lock released a moment earlier could still read as busy while another thread was starting a helper process (git, aapt2), which could fail one file of a batch. Acquiring now waits briefly before reporting the project as busy.

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
