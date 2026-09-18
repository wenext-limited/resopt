# Acceptance checklist

Status of the production-readiness requirements. `[x]` means implemented **and**
verified by the evidence named on the line; `[ ]` means open; `[~]` means shipped
with a stated limitation. Evidence lives in `docs/validation.md` and
`docs/performance.md`.

## 1. Local-first execution
- [x] `resopt web <project>` covers analyze → review → apply → restore in one page.
- [x] Loopback bind only; Host, Origin and session token enforced on every API route (tests/server.rs).
- [x] No Bun/Node needed for the native CLI/browser UI (release binary smoke test).
- [x] Static WASM site stays browser-only; no upload route exists anywhere.
- [x] `/api/capabilities` reports runtime capabilities; UI explains platform limits.

## 2. Resource discovery
- [x] Project kinds detected: Xcode, Swift package, Android, plain directory (reported in `inventory.project_kinds`; build-target membership is explicitly out of scope).
- [x] .gitignore: nested rules, negation, tracked-but-ignored files, `--include-ignored` (tests/ignore.rs).
- [x] Inventory covers PNG, JPEG, HEIC/HEIF, WebP, SVG, SVGA, audio, video, fonts, archives (content sniffing).
- [x] Each resource reports one of: optimizable, excluded (reason), unsupported, failed.
- [x] Reports state that disk inventory is not build-target membership.

## 3. Image optimization
- [x] PNG lossless (pixel + ancillary-chunk verification).
- [x] JPEG/HEIC candidates on macOS; WebP lossy and verified-lossless candidates on all native platforms.
- [x] Existing WebP/JPEG inputs: same-format recompression; transparency handled.
- [x] Quality presets 75/85/95, configurable; labelled as encoder parameters.
- [~] Color profile / orientation preserved (ImageIO) or normalized to sRGB (WebP); sources that cannot be represented are refused; metadata a WebP conversion drops is listed on the candidate; animated inputs never flattened. **Limitation:** WebP candidates do not embed ICC profiles.
- [x] Candidates that are not smaller are not written; identical work is not repeated.

## 4. Verification and user choice
- [x] Previews, exact savings, SSIMULACRA2, RGB and Alpha differences shown per candidate.
- [x] Perceptual-score threshold and Alpha threshold produce *warning* candidates that are kept.
- [x] Warning approval is explicit, per warning kind, and recorded in the transaction journal.
- [x] Approval never bypasses: corrupt file, dimensions, source/artifact hash, confinement, exclusions, transaction integrity (tests).
- [x] Warning candidates are filterable and excluded from recommended totals.

## 5. Apply and recovery
- [x] Preview → confirm → apply → restore for single items, surviving a tool restart.
- [x] Catalog metadata migration preserves unknown fields, duplicate renditions, appearance/RTL/scale entries.
- [x] Journal-first multi-file transactions; interrupted apply is restorable; stale lock after a crash is recoverable.
- [x] Edits made after analysis are never overwritten (apply and restore).
- [x] Batch apply: explicit policy, warning handling, per-file outcomes, cancellation, partial completion, restore-all.

## 6. Android
- [x] Adapter parses source set, resource type, qualifiers (density, RTL, API), resource name.
- [x] minSdk detection with explicit override; WebP gated by API level (lossy 14+, lossless/alpha 18+).
- [x] PNG→WebP in `res/` keeps the resource name; same-directory name collisions blocked; XML/`R.*` references indexed, `getIdentifier` flagged.
- [x] `assets/` path references migrated; `res/raw` cross-format blocked with explanation.
- [x] Nine-patch: lossless PNG only, border markers verified; launcher/adaptive icons: cross-format blocked.
- [x] No HEIC/JPEG-from-PNG proposals for Android resources.
- [~] AAPT2 compile + link validation on a copied real module (docs/validation.md). **Limitation:** no Gradle build of the real app (private Gradle plugins, no Gradle installed) and no on-device rendering.
- [x] Source savings reported separately from measured package savings (`resopt package-diff`).

## 7. Other formats
- [x] SVGA: verified optimization path or documented blocker.
- [~] SVG, audio, video: evaluated and **not optimized** (reasons in docs/validation.md); `ffprobe` inspection with install guidance; every such file carries an explicit "no optimizer" status.
- [x] Lossless and lossy operations are separated everywhere.

## 8. Performance
- [x] Per-phase timings (scan, hash, decode, encode, score, preview, write, report).
- [x] Bounded concurrency and memory; cancellation; resumable via validated persistent cache.
- [x] Cache: versioned keys, integrity checks, atomic writes, invalidation tests.
- [~] Artifact hashes are recorded at analysis time, so opening a report no longer re-hashes every artifact. **Limitation:** the inventory still walks the tree three times (ignore filter, catalogs, resources); measured at about 1 s of a 150 s run, so it was left as is.
- [x] Live results during analysis; large reports stay responsive.
- [x] Real-project benchmark vs. v0.5.0 baseline with ablation (docs/performance.md).

## 9. UX and docs
- [x] Filtering, sorting, original-size comparison, warnings, batch decisions, restoration in one workflow.
- [x] Light/dark/system themes, keyboard operation, labels, responsive layout, reduced motion.
- [x] KiB/MiB/GiB with exact bytes as secondary detail.
- [x] Errors state the cause and a recovery action.
- [x] README: English, user-focused; contributor/architecture docs separate; no stale claims.

## 10. xcassets integration
- [x] Catalog traversal and `Contents.json` editing stay in xcassets 0.3 (published). This release needed no new catalog API, so xcassets was not re-released.

## Beyond the original list
- [x] Duplicate / resized / near-duplicate image detection (requested during the work; `src/similarity.rs`).

## Release
- [x] fmt, clippy, unit/integration, native codec, WASM and browser verification pass.
- [x] CI green on macOS, Linux, Windows.
- [ ] GitHub Release + crates.io published; checksums, binaries, clean install and main workflows verified.
