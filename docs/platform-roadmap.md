# Platform and dependency assessment

This assessment distinguishes current behavior from proposed work. It does not imply build-target resolution or Android-safe cross-format application.

## Simplifying resopt with xcassets

The checked `xcassets` API is 0.2.0. It provides a typed catalog tree (`parse_catalog`), a lookup-name index (`index_asset_references`), diagnostics, raw JSON, and an optional parallel parser.

The existing reference index describes logical names, not individual image filenames. resopt therefore still recursively walks nodes, serializes typed contents back into JSON to detect special properties, rereads Contents.json for change detection, and implements filename replacement itself.

Two additions would have a clear boundary:

1. A **rendition iterator/index** over an already parsed catalog. Return the image-set path, Contents.json path, filename, idiom/scale/appearance, AppIcon classification, and resizing metadata. Keep unknown JSON fields accessible. This removes resopt's generic tree traversal and serde round trips without prescribing optimization policy.
2. A **pure filename replacement function** taking Contents.json bytes plus old/new basenames. Validate basenames and collisions, update every matching rendition, preserve unknown fields, and return changed bytes plus replacement count. Do not write files. This can replace the JSON-editing block in resopt's review preparation.

Leave hashing, stale-file detection, ignored paths, image encoding, source confinement, backups, and transactional application in resopt. Those are optimizer responsibilities, and moving them into an Apple catalog parser would make Android support harder.

A follow-up API should be additive and tested against duplicate renditions, RTL variants, appearance variants, resizing keys, AppIcons, opaque nodes, malformed JSON, and unknown properties. Integrate against a published compatible version; do not introduce a developer-machine path dependency. No xcassets source or package was changed during this assessment.

## Android evidence

Read-only inventory of the supplied Android checkout on 2026-09-18 found:

| Resource | Count | Source bytes |
|---|---:|---:|
| PNG | 2,003 | 28,319,244 |
| WebP | 1,555 | 24,686,816 |
| XML | 2,942 | — |
| Nine-patch (`.9.png`, included in PNG) | 423 | — |

All 3,558 inventoried PNG/WebP images were under `res/`. The version catalog declares `minSdk = 21`; the app uses shared `com.wenext.android.*` Gradle plugins, so these observations are a source audit, not a Gradle variant/minSdk resolution or build verification. No Android source files were changed and no Android build was run.

Current implemented boundaries:

- Inventory respects ignored/build directories and preserves resource paths.
- Normal PNG resources can receive same-format lossless candidates.
- `--webp` enables WebP encoding comparisons and same-format WebP recompression.
- Android resource paths do not receive JPEG/HEIC proposals.
- Nine-patch and mipmap assets are excluded from conversion.
- Cross-format application under Android `res/` is blocked. Candidates remain available for comparison/download.

Useful next steps:

1. Build an Android resource adapter that tracks source sets, qualifiers and resource IDs independently from Apple catalogs.
2. Validate PNG → WebP migration with resource basename preservation, duplicate-name checks, XML references, and file-path references in assets/raw. `R.drawable.name` is not a filename and should not be rewritten like a Swift string.
3. Gate target formats by supported Android API levels. WebP is already heavily used by this project; HEIC is not a suitable blanket replacement.
4. Handle Nine-patch in a separate path that preserves stretch/content markers and survives AAPT compilation. Do not send it through a normal bitmap conversion.
5. Validate representative debug/release variants, density/RTL selection, transparent edges and runtime rendering. Measure packaged APK/AAB differences separately from source-byte savings.

Primary references: [Android drawable resources](https://developer.android.com/guide/topics/resources/drawable-resource), [resource configuration and qualifiers](https://developer.android.com/topic/architecture/views/resources/providing-resources-views), and [supported media formats](https://developer.android.com/media/platform/supported-formats).

## WebP implementation

Native WebP encoding uses the bundled libwebp backend via the Rust `webp` crate. Candidates use the requested lossy quality settings and participate in size, dimension, orientation, Alpha and perceptual checks. They are not labelled as lossless.

On macOS, ImageIO decodes into sRGB before WebP encoding. The portable PNG/WebP path rejects unsupported profile/orientation metadata instead of silently discarding its interpretation. Animated images are not transcoded. The browser-only WASM build stays PNG-only.

## Alpha warnings

An Alpha mismatch is a reviewable visual tradeoff. The analyzer retains a smaller structurally valid candidate, its preview and the measured error, but excludes it from automatic recommendations and default savings totals. The report's warnings filter makes these candidates discoverable.

Application requires an explicit `approve_alpha_loss` flag after a warning dialog. That approval is recorded in the operation journal. Source/artifact hashes, dimensions, supported formats, project exclusions and reference safety remain enforced. This is not a general bypass of verification failures.
