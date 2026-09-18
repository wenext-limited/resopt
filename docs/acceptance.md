# Acceptance checklist

Status of the production-readiness requirements. `[x]` means implemented **and**
verified by the evidence named on the line; `[ ]` means open; `[~]` means shipped
with a stated limitation. Evidence lives in `docs/validation.md` and
`docs/performance.md`.

## 1. Local-first execution
- [ ] `resopt web <project>` covers analyze → review → apply → restore in one page.
- [ ] Loopback bind only; Host, Origin and session token enforced on every API route (tests/server.rs).
- [ ] No Bun/Node needed for the native CLI/browser UI (release binary smoke test).
- [ ] Static WASM site stays browser-only; no upload route exists anywhere.
- [ ] `/api/capabilities` reports runtime capabilities; UI explains platform limits.

## 2. Resource discovery
- [ ] Project kinds detected: Xcode, Swift package, Android, plain directory.
- [ ] .gitignore: nested rules, negation, tracked-but-ignored files, `--include-ignored` (tests/ignore.rs).
- [ ] Inventory covers PNG, JPEG, HEIC/HEIF, WebP, SVG, SVGA, audio, video, fonts, archives (content sniffing).
- [ ] Each resource reports one of: optimizable, excluded (reason), unsupported, failed.
- [ ] Reports state that disk inventory is not build-target membership.

## 3. Image optimization
- [ ] PNG lossless (pixel + ancillary-chunk verification).
- [ ] JPEG/HEIC candidates on macOS; WebP lossy and verified-lossless candidates on all native platforms.
- [ ] Existing WebP/JPEG inputs: same-format recompression; transparency handled.
- [ ] Quality presets 75/85/95, configurable; labelled as encoder parameters.
- [ ] Color profile / orientation preserved or normalized, never silently dropped; animated inputs never flattened.
- [ ] Candidates that are not smaller are not written; identical work is not repeated.

## 4. Verification and user choice
- [ ] Previews, exact savings, SSIMULACRA2, RGB and Alpha differences shown per candidate.
- [ ] Perceptual-score threshold and Alpha threshold produce *warning* candidates that are kept.
- [ ] Warning approval is explicit, per warning kind, and recorded in the transaction journal.
- [ ] Approval never bypasses: corrupt file, dimensions, source/artifact hash, confinement, exclusions, transaction integrity (tests).
- [ ] Warning candidates are filterable and excluded from recommended totals.

## 5. Apply and recovery
- [ ] Preview → confirm → apply → restore for single items, surviving a tool restart.
- [ ] Catalog metadata migration preserves unknown fields, duplicate renditions, appearance/RTL/scale entries.
- [ ] Journal-first multi-file transactions; interrupted apply is restorable; stale lock after a crash is recoverable.
- [ ] Edits made after analysis are never overwritten (apply and restore).
- [ ] Batch apply: explicit policy, warning handling, per-file outcomes, cancellation, partial completion, restore-all.

## 6. Android
- [ ] Adapter parses source set, resource type, qualifiers (density, RTL, API), resource name.
- [ ] minSdk detection with explicit override; WebP gated by API level (lossy 14+, lossless/alpha 18+).
- [ ] PNG→WebP in `res/` keeps the resource name; same-directory name collisions blocked; XML/`R.*` references indexed, `getIdentifier` flagged.
- [ ] `assets/` path references migrated; `res/raw` cross-format blocked with explanation.
- [ ] Nine-patch: lossless PNG only, border markers verified; launcher/adaptive icons: cross-format blocked.
- [ ] No HEIC/JPEG-from-PNG proposals for Android resources.
- [ ] AAPT2 validation of migrated resources where the SDK is present; Gradle/runtime validation recorded or limitation stated.
- [ ] Source savings reported separately from measured package savings (`resopt package-diff`).

## 7. Other formats
- [ ] SVGA: verified optimization path or documented blocker.
- [ ] SVG, audio, video: evaluated; optional-tool detection with install guidance; accurate capability status.
- [ ] Lossless and lossy operations are separated everywhere.

## 8. Performance
- [ ] Per-phase timings (scan, hash, decode, encode, score, preview, write, report).
- [ ] Bounded concurrency and memory; cancellation; resumable via validated persistent cache.
- [ ] Cache: versioned keys, integrity checks, atomic writes, invalidation tests.
- [ ] No redundant project scans or artifact re-hashing on open.
- [ ] Live results during analysis; large reports stay responsive.
- [ ] Real-project benchmark vs. v0.5.0 baseline with ablation (docs/performance.md).

## 9. UX and docs
- [ ] Filtering, sorting, original-size comparison, warnings, batch decisions, restoration in one workflow.
- [ ] Light/dark/system themes, keyboard operation, labels, responsive layout, reduced motion.
- [ ] KiB/MiB/GiB with exact bytes as secondary detail.
- [ ] Errors state the cause and a recovery action.
- [ ] README: English, user-focused; contributor/architecture docs separate; no stale claims.

## 10. xcassets integration
- [ ] Catalog parsing/editing needs live in xcassets; resopt uses published versions only.

## Release
- [ ] fmt, clippy, unit/integration, native codec, WASM and browser verification pass.
- [ ] CI green on macOS, Linux, Windows.
- [ ] GitHub Release + crates.io published; checksums, binaries, clean install and main workflows verified.
