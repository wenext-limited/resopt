# Architecture

resopt is one Rust crate (`resopt-cli`, library name `resopt`) plus a thin
`wasm-bindgen` crate for the browser edition. The native binary embeds its web UI;
there is no runtime dependency on Node, Bun or a frontend build.

## Data flow

```
scan_options ─▶ resources (inventory) ─▶ analysis (orchestrator) ─▶ analysis.json + report.html
                    │                        │
                    ├─ catalog (xcassets)    ├─ analyze_image  PNG / JPEG / HEIC / WebP candidates
                    ├─ android (path model)  ├─ svga           SVGA 2.x container
                    └─ android_project       ├─ cache          persistent results
                                             └─ timings        per-phase timers
review ─▶ one journaled transaction per file ─▶ batch (policy, outcomes, restore-all)
server ─▶ loopback HTTP API over analysis + review + batch ─▶ ui/*.js
```

| Module | Responsibility |
|---|---|
| `scan_options` | Git-aware path filter: nested `.gitignore`, negation, `info/exclude`, tracked-but-ignored files, fixed build/VCS exclusions. |
| `resources` | Inventory with content sniffing, project kinds, per-resource `support`, `conversion_exclusion` and `format_lock`. |
| `catalog` | Asset-catalog renditions through the `xcassets` crate. No JSON editing lives here. |
| `android`, `android_project`, `android_refs`, `aapt` | Resource path semantics, `minSdk` detection, usage index, optional AAPT2 compilation. |
| `analysis`, `analyze_image` | Scheduling, duplicate sharing, cancellation, memory budget, candidate generation and verification. |
| `image_backend`, `webp_backend`, `optimizer`, `png_pixels`, `quality` | Codecs and metrics. ImageIO is macOS-only; oxipng and libwebp are bundled everywhere. |
| `localization`, `localization_checks` | Read-only localization review through `langcodec`: groups `.lproj` and Android `values-*` files into tables and reports coverage, empty values and placeholder mismatches. Runs on its own thread next to the image workers. |
| `svga` | SVGA optimization policy on top of the `svga` crate: refusals, lossless image replacement, `verify()` on final bytes. |
| `svga_render` | Self-contained SVGA frame renderer (`svga` + `tiny-skia` + `svgtypes`): poster thumbnails during analysis and on-demand frames for the live player. Semantics follow the Android/iOS reference players and are documented at the top of the module. Written to be liftable into its own crate. |
| `review` | Verifies and applies exactly one candidate as a recoverable transaction; restores it. |
| `references` | Static reference migration for path-addressed files (loose Apple resources, Android `assets/`). |
| `batch` | Chooses candidates under an explicit policy and runs them through `review`. |
| `server`, `web`, `report`, `ui/` | Loopback server, live results, static report, embedded UI. |
| `plan` | The original PNG-only `plan`/`apply`/`restore` workflow for asset catalogs. |
| `portable` + `crates/resopt-wasm` | Byte-oriented PNG API compiled to WebAssembly for the static site. |

## Invariants

**Analysis never writes to the project.** Output goes to a new directory outside
the scanned root. Sources are hashed before and after analysis; a file that moved
is reported as failed instead of producing stale numbers.

**"Lossless" is a verified claim.** A PNG candidate must decode to identical
samples (including RGB under transparent pixels) and keep every non-IDAT chunk;
with `--png-reductions` only IHDR/PLTE/tRNS may change and expanded RGBA16 samples
must match. A lossless WebP candidate must decode to the PNG's exact 8-bit RGBA;
sources with color-profile chunks or 16-bit samples are refused. An SVGA
candidate must keep every non-image protobuf field byte-identical and every
embedded PNG pixel-identical.

**Warnings are narrow.** Only three verdicts are approvable, each by name:
`alpha_error_exceeds_policy`, `transparency_presence_changed`,
`quality_below_policy`. Approval is checked against every threshold the candidate
misses and is written to the operation journal. Corrupt files, changed
dimensions, stale hashes, path escapes, exclusions and format locks are hard
failures in `review::prepare`, which every apply path goes through.

**Transactions are journal-first.** `operations/<index>/transaction.json` and the
content-addressed before/after blobs are durable before the first project file
changes. Each write re-checks that the file still holds the recorded before or
after content, so restore works after a crash or restart and never overwrites a
later edit. Files are created atomically (temporary sibling, then a no-clobber
link), so a crash never leaves a partial file that matches neither side; a
backup blob damaged by an older crash is rewritten, and a journal that no longer
loads is replaced only after `prepare` has proved the operation is not applied.
Project-wide mutual exclusion uses an operating-system file lock, which a crashed
process cannot leave behind (on Windows the empty lock file itself stays in
place and is reused).

**Reuse needs equal policy, not just equal bytes.** In-run duplicate sharing and
the persistent cache key on source SHA-256, the resource's policy class (catalog,
loose, Android area/type/nine-patch/minSdk, locks), every option that affects
candidate bytes or verdicts, the tool version and the codec backend (macOS build
for ImageIO). Cache entries are staged and renamed into place, hash-checked on
read, and discarded on any mismatch.

## Local server security model

`resopt web` and `resopt serve` bind `127.0.0.1` only.

- **Host** must equal the listening address on every request (DNS rebinding).
- **The page is served only to the launch URL**, which carries a per-process
  key (`/?k=…`, printed in the terminal and opened in the browser). The response
  sets an `HttpOnly; SameSite=Strict` cookie and the script removes the key from
  the address bar. A local process that only knows the port gets `403` for the
  page, for `analysis.json` and for every artifact, so it can neither read
  project data nor learn the session token.
- **Every `/api/*` route** requires the session token header; **every POST**
  also requires the page's own `Origin` and a JSON content type, so other sites
  cannot drive the API even from the same browser.
- The server accepts no uploads and no filesystem paths: resources are addressed
  by report index, artifacts only by generated names under `previews/`,
  `candidates/` and `originals/`.
- Responses carry a restrictive CSP, `nosniff`, `no-referrer` and
  `Cross-Origin-Resource-Policy: same-origin`.

Out of scope: a process running as the same user that can read the terminal
output or the browser's cookie store already has direct write access to the
project.

## Concurrency

Image workers are plain threads pulling indexes from a shared queue. They are
deliberately **not** a rayon pool: oxipng uses rayon internally, and a rayon
worker waiting on nested work executes other queued tasks on the same stack.
With a task already holding a pixel-budget lease or initializing a shared
duplicate slot, that re-entrancy deadlocked (found in review, reproduced, and
covered by `duplicates_under_a_tight_pixel_budget_never_deadlock`).

## Report compatibility

`analysis.json` schema 2 adds optional fields only; schema 1 reports from 0.5.x
still open in `serve`, `report`, `apply` and `restore`. Reports from before the
perceptual threshold are not re-judged against it when applied.

## UI

`src/ui/*.js` are plain scripts concatenated into one scope by `report.rs`.
`core.js` and `i18n.js` have no DOM access and are unit-tested with Node
(`tests/report-ui.cjs`), including translation completeness. The same page
serves a live session (rows streamed from `/api/results`) and the static
`report.html` (rows embedded, read-only).
