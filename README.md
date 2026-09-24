# resopt

**Find smaller app resources, review the real candidates, and apply only what you approve — with every change reversible.**

resopt scans Xcode, Swift package, Android and plain resource directories. It finds smaller image and ZIP candidates, groups similar images, and previews animations in a local review page with explicit apply and restore controls.

Your files never leave your computer. Analysis never modifies your project.

**New in [v0.8.0](https://github.com/wenext-limited/resopt/releases/tag/v0.8.0):** scored similarity groups, ZIP browsing and lossless embedded-PNG optimization, and VAP/PAG/TCMP4 playback. [Release notes](CHANGELOG.md#080--2026-09-23).

## Resource support

| Resource | Optimization | Review |
|---|---|---|
| PNG | Verified lossless recompression, optional lossy palette reduction | Previews, quality scores, pixel differences |
| JPEG / HEIC | Lossy candidates on macOS | PNG previews, quality scores, alpha checks |
| Static WebP | Same-format lossy recompression; PNG → WebP candidates | Lossless PNG → WebP verified sample-for-sample |
| SVGA 2.x | Lossless embedded-PNG recompression for supported files | Poster, timeline and live frame playback |
| ZIP resource packages | Lossless embedded-PNG recompression in eligible packages | Searchable contents, stored/expanded sizes, image previews |
| VAP inside MP4 | Preview only | Reconstructed transparency, playback and frame scrubbing |
| PAG / TCMP4 | Preview only; recognized by PAG content signature | Playback, frame scrubbing, editable-text/image and video counts |
| Localization: `.xcstrings`, `.strings`, `.stringsdict`, Android `strings.xml` | Inspection only | Coverage per language, empty values, placeholder mismatches with the source language, Xcode stale keys |
| Audio, ordinary video, fonts, SVG, PDF, recognized Lottie JSON | Inventory only | Format and size; optional audio/video metadata via `ffprobe` |

Optimization and preview support have different limits. See [platform support](#platform-support) and [important limits](#important-limits).

## Review before you apply

- **Scan a whole project.** Asset catalogs, loose resources, Android `res/` and `assets/`, with Git ignore rules respected. Results stream into the review page; analysis can be stopped. Eligible image and SVGA results are reused through a verified cache.
- **Compare image candidates.** Use 2-up, swipe, onion skin or an amplified pixel difference. Lossy image candidates show SSIMULACRA2 quality scores and RGB/alpha errors. Lossless candidates undergo pixel and metadata verification.
- **Review similar images as groups.** One row represents each group, with thumbnails, total size and a similarity range. Every member is scored against the largest reference file. Only identical file bytes score 100; other scores are approximate and capped at 99.9. Search keeps the entire group visible. Intended scale/density variants do not form findings on their own. Nothing is merged or deleted automatically. [Scoring details](docs/similarity.md).
- **Decide on warnings.** Candidates below quality or alpha thresholds remain reviewable but are excluded from recommended savings. Accepting a warning is explicit and recorded. Approval cannot bypass corrupt files, stale source hashes, changed dimensions or protected resources.
- **Apply and restore.** Apply one candidate or a reviewed batch. Changes are journaled and restorable across restarts; later edits are never overwritten. Asset-catalog entries and statically resolvable references are updated together when a format change requires it.

## Install

Download a ready-to-run binary from [GitHub Releases](https://github.com/wenext-limited/resopt/releases/latest):

- macOS — Apple Silicon or Intel
- Linux — x86_64
- Windows — x86_64

Extract the archive and put `resopt` (or `resopt.exe`) on your `PATH`. Binaries need no Rust, Node.js, Bun, ffmpeg or Android SDK. Verify a download with the published `SHA256SUMS`.

With Rust installed:

```sh
cargo install resopt-cli --locked
resopt --version
```

The package is named **resopt-cli**; the command is **resopt**. Building from source needs Rust 1.89+ and a C compiler.

## Start with your project

### Command line

```sh
resopt web /path/to/project
```

resopt opens your browser on a page served from `127.0.0.1` only. The printed URL contains a session key; the page and its data are not served without it. Results stream in as files finish. When analysis completes you can compare candidates, apply a change, restore it, or batch-apply under a policy you choose.

After analysis completes, live sessions check file presence every two seconds. Deleted resources disappear from the list, counts and similarity groups; files returned to their original paths reappear. New files and changed image contents still require re-analysis. Static HTML reports remain snapshots.

The terminal prints the report directory. It holds the report and the restore backups — keep it for as long as you may want to undo changes. To reopen it later (open the URL it prints):

```sh
resopt serve /path/to/report
```

### macOS app (build from source)

On a Mac with Xcode and Rust installed, run `macos/build-app.sh` to create
`dist/Resopt.app`, then open the app and choose a project directory. The native
SwiftUI shell bundles the same Rust engine and local review UI as the CLI. It
keeps reports and restore backups under
`~/Library/Application Support/resopt/Reports`.

Choose 2, 4, or 8 parallel image tasks before starting a scan. In the results,
Shift-click selects a range and Option/Alt-click toggles individual resources;
when several resources are selected, batch apply and restore selected are scoped
to that selection.
Applied images use a green background and an aligned **Applied** label. The
local build is ad-hoc signed for testing and is not notarized for distribution.

### ZIP packages and effect previews

Select a ZIP row to browse its contents, search paths, filter formats and compare stored versus expanded sizes. If a verified smaller ZIP is available, apply or restore it as one package. Filenames, pixels, atlas data and other payloads are preserved. Packages with recognized manifests or checksum maps require their publishing workflow. [ZIP support and limits](docs/archive-resources.md).

Select a VAP or PAG/TCMP4 row to play the original effect or scrub its frames. PAG playback uses a bundled renderer in an isolated frame with no CDN requests. These are **previews, not optimization candidates**; app-provided text/images can differ from the template shown. Open reports through `resopt serve` or a local HTTP server, since browser restrictions can block playback from `file://`. [Effect support and limits](docs/effect-resources.md).

To inspect resources without generating optimization candidates:

```sh
resopt analyze /path/to/project --probe-only --out /tmp/resopt-inspection
resopt serve /tmp/resopt-inspection
```

The output directory must be new and outside the project being scanned.

### Useful options

```sh
resopt web . --no-webp                  # skip WebP candidates (on by default for loose files and Android)
resopt web . --no-lossy-png             # skip palette-reduced PNG candidates (on by default)
resopt web . --no-near-lossless-heic    # skip the HEIC quality-100 candidate (macOS, on by default)
resopt web . --png-reductions           # allow lossless PNG palette/bit-depth reductions
resopt web . --qualities 85             # one quality level for a faster first pass
resopt web . --min-score 90             # stricter perceptual threshold (default 80)
resopt web . --android-min-sdk 21       # when minSdk cannot be read from Gradle files
resopt web . --out ~/resopt-report --no-open
resopt web . --include-ignored          # also scan files matched by Git ignore rules
```

Quality values are **encoder parameters, not savings percentages**. `--min-score` sets the minimum SSIMULACRA2 quality score for recommended lossy candidates. It does not control the separate 0–100 similarity scores used in image groups.

Use `--jobs` to set parallel workers (default: CPU count, up to 8), `--max-pixels` for very large images, and `--no-cache` to skip the result cache. `resopt cache` prints the cache location; `resopt cache --clear` empties it.

## Reports, scripts and CI

Keep the **entire report directory**, including `analysis.json`, previews, candidates, bundled playback files and restore backups. Copying only `report.html` loses those resources.

```sh
resopt scan . --json                                   # inventory only, nothing is encoded
resopt analyze . --out /tmp/report --json --timings    # analysis.json, report.html, candidates
resopt apply /tmp/report --dry-run                     # what the default policy would apply
resopt apply /tmp/report                               # verified lossless, same-format only
resopt apply /tmp/report --lossy --min-score 92        # widen the policy explicitly
resopt restore /tmp/report                             # undo every applied change
resopt report /tmp/report                              # refresh HTML without re-analysis
```

`apply` on a report never applies lossy candidates, format changes or warning candidates unless you pass `--lossy`, `--cross-format` or `--accept-warning <kind>`. Each file is applied as its own recoverable operation and gets its own outcome line; the command exits non-zero if any file failed.

Refreshing HTML does not create missing scores, previews or playback artifacts in an older report. Re-analyze the project to use newly added analysis features.

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
| Project inventory, Git ignore rules, scored similarity groups | Yes | Yes |
| PNG lossless optimization | Yes | Yes |
| SVGA lossless optimization | Yes | Yes |
| ZIP browsing and lossless embedded-PNG optimization | Yes | Yes |
| VAP / PAG / TCMP4 previews | Compatible browser required | Compatible browser required |
| Lossy PNG candidates (palette reduction; `--no-lossy-png` to skip) | Yes | Yes |
| WebP candidates, lossy and lossless (on by default; `--no-webp` to skip) | Yes | Yes¹ |
| JPEG and HEIC candidates; decoding JPEG/HEIC/GIF/TIFF inputs | Yes (Apple ImageIO) | No |
| Local review page, apply, batch, restore | Yes | Yes |
| AAPT2 validation (optional Android SDK), `ffprobe` media details (optional) | Yes | Yes |

¹ Without ImageIO, WebP conversion accepts PNG and WebP inputs that carry no embedded color profile or EXIF orientation; other inputs are reported as unsupported rather than converted incorrectly.

`resopt doctor` shows what is available on your machine and how to install optional tools. resopt never installs anything itself.

A browser-only edition (static site, WebAssembly) optimizes individual PNG files entirely inside the browser. It has no server component and nothing is uploaded. Whole-project scanning, other formats and applying changes need the `resopt` command.

## Important limits

- Savings are **source-file bytes**. They are not IPA/APK size or store download size: Xcode compiles asset catalogs and AAPT2 re-compresses PNGs. Use `package-diff` on real builds to measure shipped size.
- The inventory lists files on disk. It does not know which files a particular build target, flavor or variant includes.
- Quality and similarity scores help prioritize review; they do not replace looking at the image. Similarity uses 16×16 summaries and may miss tiny details or local color changes. Matching files are not necessarily interchangeable.
- Lossy PNG uses at most 256 palette colours without dithering. Images with soft transparency usually exceed the default Alpha tolerance and are offered as warnings rather than recommendations; raise `--max-alpha-error` if that trade-off is acceptable for your artwork.
- HEIC candidates are always lossy: Apple's encoder has no lossless mode (at quality 100, 2–17% of samples still change), so resopt does not offer a "lossless HEIC". It offers quality 100 as a clearly labelled *near-lossless* candidate instead.
- Existing HEIC files are not rewritten losslessly. Measured on 692 real app HEIC files, removable metadata (Exif, XMP) was 0.15% of their bytes and none contained a thumbnail, which does not justify rewriting the container.
- App icons, sliced (resizable) catalog images and animated images are inspected but never converted. Animated images are never flattened.
- WebP is not offered for asset-catalog renditions.
- Reference migration covers statically resolvable references. Names built at runtime, third-party decoders and references outside the scanned directory need your review; ambiguous references block the change instead of guessing.
- SVGA 1.x (zip) files, and SVGA files containing audio or unknown fields, are reported as unsupported rather than rewritten (they are still previewed). SVGA playback draws bitmaps, shapes, clip paths and mattes; dynamic text/images set by app code at runtime and JPEG-encoded embedded images are not drawn.
- ZIP inspection is bounded to 64 MiB input, 10,000 entries and 256 MiB expanded contents. Up to 32 large images receive previews. Nested archives are listed but not recursively rewritten; external signatures or hashes still require the package publisher.
- VAP/PAG playback depends on browser and decoder compatibility and previews original content only. It does not establish native-device pixel parity. No PAG/VAP transcoding, animated WebP optimization, audio transcoding or font subsetting is offered.
- Localization review compares languages within one table and never edits strings. Plural forms are not compared for arguments, and `.xcstrings` device variations count as missing. [Details](docs/localization.md).
- Archive, effect and localization inspection currently bypass the image-result cache.
- HEIC candidates cannot be displayed by most browsers; the comparison uses a PNG preview and links the file so you can open it in Preview or Safari.

Run `resopt --help` or `resopt <command> --help` for every option.

[Changelog](CHANGELOG.md) · [Development guide](docs/development.md) · [Architecture](docs/architecture.md) · [Validation evidence](docs/validation.md) · [Performance](docs/performance.md) · [MIT license](LICENSE) · [PAG runtime licenses](src/ui/vendor/libpag/LICENSE.txt)
