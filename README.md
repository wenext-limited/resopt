# resopt

**Find smaller app resources, review the real candidates, and apply only what you approve — with every change reversible.**

resopt scans an Xcode, Swift package, Android or plain resource directory, measures what each image and animation could shrink to, and opens a local review page with previews, quality scores and one-click apply and restore.

Your files never leave your computer. Analysis never modifies your project.

## What you can do

- **Analyze a whole project.** Asset catalogs, loose resources, Android `res/` and `assets/`. Git ignore rules are respected by default, including nested rules, negations and force-tracked files.
- **See results while analysis runs.** Completed files appear immediately, largest first. Stop at any time; finished work is cached, so the next run continues where you left off.
- **Optimize PNG without changing a pixel.** Decoded samples (including color under transparent pixels) and metadata chunks are verified before a candidate is offered.
- **Compare JPEG, HEIC and WebP candidates** at the encoder quality levels you choose (75, 85 and 95 by default), including same-format recompression of existing JPEG and WebP files. Lossless WebP candidates are verified sample-for-sample.
- **Optimize SVGA animations losslessly.** Embedded images are recompressed; every other byte of the animation and every pixel is verified unchanged.
- **Judge quality with evidence.** Side-by-side previews, a full-size comparison slider, SSIMULACRA2 perceptual scores, and RGB and Alpha error for every candidate.
- **Decide on warnings yourself.** Candidates below the perceptual-score or Alpha thresholds are kept, clearly marked, and excluded from recommended totals. You can accept one after reviewing it; your approval is recorded. Corrupt files, changed dimensions, stale files and protected resources can never be approved through.
- **Apply safely, one file or many.** Preview exactly which files change, confirm, and restore any time — even after restarting resopt. Batch apply takes an explicit policy, reports each file's outcome, can be stopped midway, and "Restore all" undoes everything. Files you edited after analysis are never overwritten.
- **Keep references working.** Asset-catalog `Contents.json` entries and statically resolvable references in source, project and web files are migrated together with a format change, and restored together.
- **Find duplicate images.** Identical files, the same picture saved at different sizes, and near-duplicates are grouped by comparing decoded pixels. Intended variants (`@2x`/`@3x`, Android density folders) are not reported.
- **Inventory everything else.** Audio, video, fonts, archives, SVG, PDF and data files are listed with an explicit "no optimizer" status. With `ffprobe` installed, audio and video show codec, duration and bitrate.
- **Use it in scripts and CI.** JSON output, a self-contained HTML report, and command-line batch apply/restore.

## Install

Download a ready-to-run binary from [GitHub Releases](https://github.com/wenext-limited/resopt/releases/latest):

- macOS — Apple Silicon or Intel
- Linux — x86_64
- Windows — x86_64

Extract the archive and put `resopt` (or `resopt.exe`) on your `PATH`. Binaries need no Rust, Node.js, Bun, ffmpeg or Android SDK. Verify a download with the published `SHA256SUMS`.

With Rust installed:

```sh
cargo install resopt-cli --locked
```

The package is named **resopt-cli**; the command is **resopt**. Building from source needs Rust 1.89+ and a C compiler.

## Start with your project

### macOS app (build from source)

On a Mac with Xcode and Rust installed, run `macos/build-app.sh` to create
`dist/Resopt.app`, then open the app and choose a project directory. The native
SwiftUI shell bundles the same Rust engine and local review UI as the CLI. It
keeps reports and restore backups under
`~/Library/Application Support/resopt/Reports`.

Choose 2, 4, or 8 parallel image tasks before starting a scan. In the results,
Shift-click selects a range and Option/Alt-click toggles individual resources;
when several resources are selected, batch apply is scoped to that selection.
Applied images use a green background and an aligned **Applied** label. The
local build is ad-hoc signed for testing and is not notarized for distribution.

### Command line

```sh
resopt web /path/to/project
```

resopt opens your browser on a page served from `127.0.0.1` only. The printed URL contains a session key; the page and its data are not served without it. Results stream in as files finish. When analysis completes you can compare candidates, apply a change, restore it, or batch-apply under a policy you choose.

The terminal prints the report directory. It holds the report and the restore backups — keep it for as long as you may want to undo changes. To reopen it later (open the URL it prints):

```sh
resopt serve /path/to/report
```

### Useful options

```sh
resopt web . --webp                     # also compare WebP (loose files and Android resources)
resopt web . --png-reductions           # allow lossless PNG palette/bit-depth reductions
resopt web . --qualities 85             # one quality level for a faster first pass
resopt web . --min-score 90             # stricter perceptual threshold (default 80)
resopt web . --android-min-sdk 21       # when minSdk cannot be read from Gradle files
resopt web . --out ~/resopt-report --no-open
resopt web . --include-ignored          # also scan files matched by Git ignore rules
```

Quality values are **encoder parameters, not savings percentages**. `--min-score` is the lowest SSIMULACRA2 score a lossy candidate may have and still be recommended (100 is identical, 90+ is usually imperceptible).

Use `--jobs` to set parallel workers (default: CPU count, up to 8), `--max-pixels` for very large images, and `--no-cache` to skip the result cache. `resopt cache` prints the cache location; `resopt cache --clear` empties it.

## Reports, scripts and CI

```sh
resopt scan . --json                                   # inventory only, nothing is encoded
resopt analyze . --out /tmp/report --json --timings    # analysis.json, report.html, candidates
resopt apply /tmp/report --dry-run                     # what the default policy would apply
resopt apply /tmp/report                               # verified lossless, same-format only
resopt apply /tmp/report --lossy --min-score 92        # widen the policy explicitly
resopt restore /tmp/report                             # undo every applied change
```

`apply` on a report never applies lossy candidates, format changes or warning candidates unless you pass `--lossy`, `--cross-format` or `--accept-warning <kind>`. Each file is applied as its own recoverable operation and gets its own outcome line; the command exits non-zero if any file failed.

The original PNG-only plan workflow is still available: `resopt plan`, then `resopt apply <plan>` and `resopt restore <plan>`.

## Android projects

resopt treats Android resources as resources, not loose files:

- Files under `res/` are identified by source set, resource type, qualifiers (density, RTL, locale, API level) and resource name.
- **PNG → WebP** keeps the resource name, so `@drawable/name` and `R.drawable.name` keep working and no source file is rewritten. A second file with the same name in the same directory blocks the change. The preview shows how many XML and code references use the name and flags `getIdentifier()` lookups.
- **WebP is gated by `minSdk`**, read from Gradle files and version catalogs: lossy WebP needs API 14, lossless or transparent WebP needs API 18. If `minSdk` cannot be determined, WebP is not proposed until you pass `--android-min-sdk`.
- **Nine-patch (`.9.png`)** files stay PNG so AAPT can read their stretch and content markers; they still get pixel-identical PNG optimization. **Launcher icons** (`mipmap-*`) and **`res/raw`** files also keep their format. The reason is shown wherever a change is blocked.
- **JPEG and HEIC replacements are never proposed** for Android resources.
- Files under `assets/` are opened by path, so a format change migrates path references like a loose file — one file at a time, never in a batch, because asset paths are often built at runtime.
- With the Android SDK build-tools installed, every `res/` candidate is compiled with **AAPT2** before it is applied, and the preview shows the measured compiled size. This matters: AAPT2 re-compresses PNG files during the build, so source savings on PNG do not translate one-to-one into the APK.

To measure what actually changed in a build, compare two packages:

```sh
resopt package-diff before.apk after.apk      # also .aab, .ipa or any zip
```

## Platform support

| Capability | macOS | Linux / Windows |
|---|---|---|
| Project inventory, Git ignore rules, duplicate detection | Yes | Yes |
| PNG lossless optimization | Yes | Yes |
| SVGA lossless optimization | Yes | Yes |
| WebP candidates (`--webp`), lossy and lossless | Yes | Yes¹ |
| JPEG and HEIC candidates; decoding JPEG/HEIC/GIF/TIFF inputs | Yes (Apple ImageIO) | No |
| Local review page, apply, batch, restore | Yes | Yes |
| AAPT2 validation (optional Android SDK), `ffprobe` media details (optional) | Yes | Yes |

¹ Without ImageIO, WebP conversion accepts PNG and WebP inputs that carry no embedded color profile or EXIF orientation; other inputs are reported as unsupported rather than converted incorrectly.

`resopt doctor` shows what is available on your machine and how to install optional tools. resopt never installs anything itself.

A browser-only edition (static site, WebAssembly) optimizes individual PNG files entirely inside the browser. It has no server component and nothing is uploaded. Whole-project scanning, other formats and applying changes need the `resopt` command.

## Important limits

- Savings are **source-file bytes**. They are not IPA/APK size or store download size: Xcode compiles asset catalogs and AAPT2 re-compresses PNGs. Use `package-diff` on real builds to measure shipped size.
- The inventory lists files on disk. It does not know which files a particular build target, flavor or variant includes.
- A perceptual score helps you prioritize; it does not replace looking at the image, especially for UI art with fine edges.
- App icons, sliced (resizable) catalog images and animated images are inspected but never converted. Animated images are never flattened.
- WebP is not offered for asset-catalog renditions.
- Reference migration covers statically resolvable references. Names built at runtime, third-party decoders and references outside the scanned directory need your review; ambiguous references block the change instead of guessing.
- SVGA 1.x (zip) files, and SVGA files containing audio or unknown fields, are reported as unsupported rather than rewritten.
- SVG, PDF, audio, video, fonts and archives are inventoried but not optimized. No lossy audio/video transcoding is performed.
- HEIC candidates cannot be displayed by most browsers; the comparison uses a PNG preview and links the file so you can open it in Preview or Safari.

Run `resopt --help` or `resopt <command> --help` for every option.

[Changelog](CHANGELOG.md) · [Development guide](docs/development.md) · [Architecture](docs/architecture.md) · [Validation evidence](docs/validation.md) · [Performance](docs/performance.md) · [MIT license](LICENSE)
