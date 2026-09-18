# resopt

**Find smaller image assets, compare the results, and apply the changes you approve.**

resopt scans a local project for resource files and shows where you can save space. It combines lossless PNG optimization with JPEG and HEIC comparisons on macOS, then gives you a browser report with previews, quality scores, and reversible changes.

Your images stay on your computer. Analysis never modifies the project.

## What you can do

- **Analyze a whole project.** Scan asset catalogs and loose resources, respecting Git ignore rules by default.
- **Find savings as analysis runs.** See progress, the savings found so far, and the largest completed opportunities before the full report is ready.
- **Optimize PNGs without changing pixels.** Optionally allow lossless palette, color-type, and bit-depth reductions. Decoded pixels and retained metadata are verified.
- **Compare WebP candidates with `--webp`.** Encode loose images at the selected quality levels, including transparency, with the bundled native codec.
- **Compare JPEG and HEIC candidates on macOS.** Try quality levels 75, 85, and 95, with transparency checks and SSIMULACRA2 quality scores.
- **Review images visually.** Compare original and candidate previews, inspect full-size files, search and sort results, and use light or dark mode.
- **Decide whether to accept transparency differences.** Alpha warnings keep their previews and downloads. An additional warning confirmation lets you accept that change; source-file conflicts and structural failures still block application.
- **Apply only what you approve.** Preview reference changes, confirm an image replacement, and restore its original when needed. Supported Xcode catalog and static file references are updated together.
- **Use reports in scripts and CI.** Export JSON and a self-contained HTML report without applying changes.

## Install

Download a ready-to-run binary from [GitHub Releases](https://github.com/wenext-limited/resopt/releases/latest):

- macOS — Apple Silicon or Intel
- Linux — x86_64
- Windows — x86_64

Extract the archive and put `resopt` (or `resopt.exe`) on your `PATH`. Downloaded binaries do not require Rust, Bun, Node.js, or ffmpeg.

With Rust installed:

```sh
cargo install resopt-cli --locked
```

The package is named **resopt-cli**; the command is **resopt**. Building from source requires Rust 1.89+ and a C compiler.

## Start with your project

```sh
resopt web /path/to/project
```

Or run this inside your project:

```sh
resopt web .
```

resopt opens your browser and analyzes files directly from disk. The server listens only on `127.0.0.1`; no images are uploaded to a remote service. Completed opportunities appear while analysis continues. Once finished, the full report lets you compare candidates, apply an individual change, or restore an original.

The terminal prints the report directory. Keep it if you need the report or restoration backups later. To reopen it:

```sh
resopt serve /path/to/report
```

## Choose how to analyze

```sh
# Allow additional lossless PNG reductions
resopt web . --png-reductions

# Include WebP candidates for loose images
resopt web . --webp

# Compare one lossy quality level for a quicker initial pass
resopt web . --qualities 85

# Keep reports in a known location; open the printed URL yourself
resopt web . --out /tmp/my-resource-report --no-open
```

The output directory must be new and outside the project. Use `--jobs` to adjust parallel image processing and `--max-pixels` for unusually large images. Quality values are encoder settings, **not** a percentage of size reduction.

## Reports without applying changes

```sh
resopt scan . --json
resopt analyze . --out /tmp/resource-analysis --json
resopt report /tmp/resource-analysis
```

- `scan` inventories resources without encoding them.
- `analyze` creates `analysis.json`, `report.html`, previews, and candidate files.
- `report` refreshes the HTML from saved measurements without recompressing images.

For an explicit lossless PNG plan:

```sh
resopt plan . --out /tmp/png-plan
resopt apply /tmp/png-plan
resopt restore /tmp/png-plan
```

Review the plan before applying it. `apply` and `restore` check file contents and refuse conflicting changes rather than overwriting subsequent edits.

## Platform support

| Capability | macOS | Linux / Windows |
|---|---|---|
| Project inventory and Git ignore rules | Yes | Yes |
| PNG lossless analysis and optimization | Yes | Yes |
| WebP candidates (`--webp`) | Yes | Yes* |
| Local browser report, review, and restore | Yes | Yes |
| JPEG / HEIC encoding and image comparison | Apple ImageIO | Not available |

*On Linux/Windows, WebP conversion uses PNG/WebP inputs without unsupported color profiles or EXIF orientation. macOS handles image color conversion through ImageIO. WebP is not offered as an Xcode image-set rendition.

The standalone browser/WASM edition processes selected static PNGs entirely in the browser. For whole-project scanning, JPEG/HEIC, and project reference updates, use the local `resopt web` command.

## Important limits

- Savings describe **source asset sizes**, not guaranteed APK, IPA, or store download savings.
- A quality score helps prioritize review; it does not replace checking the image yourself.
- App icons, recognized stretchable images, and multi-frame images are excluded from automatic conversion.
- Resource inventory is broader than optimization support. Audio, video, SVGA, fonts, and other files may be listed without a compression backend.
- Automatic reference migration covers supported static patterns. Ambiguous or dynamic references can block a cross-format replacement.
- Android resource directories support inventory, PNG lossless analysis, and opt-in WebP comparisons. Nine-patch images and mipmap assets are excluded; JPEG/HEIC conversions are not proposed for Android resources. Cross-format application to Android `res/` remains blocked until resource-aware migration is available. Existing WebP files can be re-encoded in the same format with your approval.

Run `resopt --help` or `resopt <command> --help` for all options.

[Development guide](docs/development.md) · [Platform roadmap](docs/platform-roadmap.md) · [Changelog](CHANGELOG.md) · [MIT license](LICENSE)
