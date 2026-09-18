# Validation evidence — 0.6.0 (2026-09-19)

All write tests ran on disposable fixtures or copies. The three real projects
used for validation were only ever read; none of their files were modified.
Earlier evidence is kept in [validation-2026-09-15.md](validation-2026-09-15.md)
and [validation-images.md](validation-images.md).

## Automated checks

| Check | Result |
|---|---|
| `cargo fmt --all --check`, `cargo clippy --workspace --all-targets -D warnings` | clean (Rust 1.97 locally, 1.98 in CI) |
| Rust unit + integration tests | 100+ tests: analysis, cache, similarity, SVGA, Android rules, review transactions, batch, server, ignore rules, plan workflow |
| UI unit tests (`node --test tests/report-ui.cjs`) | 11 tests, including translation completeness and approvable-warning policy |
| WASM build, typecheck and Bun tests | CI `browser` job |
| macOS native codec tests (ImageIO JPEG/HEIC) and Bun host integration | CI macOS job |
| CI matrix | macOS, Linux and Windows |

Failure paths covered by tests, not only successful conversions:

- **Stale or hostile input**: source edited after analysis; tampered artifact; forged cross-format candidate for a nine-patch; symlinked sources, backup directories and lock files; path traversal in artifact routes, cache templates and AAPT2 inputs; malformed SVGA (every prefix of a valid file), zlib bombs, zip and non-zip garbage for `package-diff`.
- **Approval boundaries**: a warning needs approval of every threshold it misses; approving another or an unknown kind fails; approval never bypasses hash, dimension, format-lock or exclusion checks; approvals are recorded in the journal.
- **Transactions**: interrupted apply (partial state) restores; restore after restart; shared `Contents.json` restores in dependency order; later user edits are never overwritten by apply, restore or restore-all; a second live process is excluded while a crashed one leaves no lock behind.
- **Batch**: invalid policies are refused before any write; a stale preview token is refused; a conflicting file fails alone while the rest apply; applied files are skipped by the next batch; asset renames are excluded.
- **Cancellation and concurrency**: cancelling analysis yields a consistent, reviewable report; changes are refused while analysis runs; the pixel budget serializes oversized work without deadlock.
- **Cache**: content and option changes miss; corrupted, truncated, interrupted and foreign entries are discarded and recomputed; pruning removes the oldest entries.
- **Server**: Host, session token and Origin enforced per route; the page, `analysis.json` and artifacts are refused without the launch key or its cookie; no upload route; only generated artifact names are served.
- **Independent review**: an adversarial review of the branch reproduced an analysis deadlock (nested rayon work under a held lease; 9 of 12 runs hung in its reproduction, 0 of 24 after the fix) and reported three medium issues (token readable by local non-browser clients, non-atomic creates blocking crash recovery, a Windows lock-file race). All four are fixed, with regression tests for the deadlock, the keyed page and cookie, atomic creates and damaged-backup repair. The review found no issue in approval boundaries, path confinement, journal ordering, cache trust, malformed-input handling or the UI's text handling.

## Real-browser verification

`tests/browser/e2e.mjs` drives the release binary's page in headless Chrome
against a fixture built from copied real assets (41 image sets, loose PNGs, 6
SVGA files, audio). 13 steps pass, three consecutive runs: live results, filters
and sorting, keyboard use, named controls and image alt text, light/dark themes
and language switching, full-size comparison, warning confirmation wording,
single apply/restore, batch apply and restore-all with a **byte-exact project
snapshot afterwards**, phone-width layout without overflow, reduced motion, and
no script errors.

The browser run found four defects that unit tests had not: a control-character
regex corrupted by HTML inlining, restore-all also starting a batch apply, a
stale dialog `close` event cancelling progress polling, and toolbar overflow.
All are fixed and the first has a regression test.

The static offline report for the 7,078-resource Android project opens in about
0.3 s and filters in about 20 ms.

## Real projects (read-only analysis)

| Project | Files inventoried | Notes |
|---|---:|---|
| iOS app A (git repository with 20 ignored worktrees) | 4,342 | 27 catalogs. Before the fix in this release, an ignored root file hid all of them (0 found). |
| iOS app B (linked git worktree) | 5,184 | 31 catalogs; 2,505 images decoded; 206 duplicate groups (108 identical, 42 resized, 56 similar; 1.62 MiB redundant). |
| Android app (44 Gradle modules) | 7,078 | `minSdk` 21 found in the version catalog although only a convention plugin applies it; 423 nine-patch and 20 launcher-icon files format-locked; 17 animated WebP inspected, never flattened; 3,153 resources with candidates. |

## Android migration validated with AAPT2

One module's drawable folders (55 files: PNG, WebP, nine-patch, locale and RTL
variants) were copied into a fixture with `minSdk` 21, analyzed with `--webp`,
and batch-applied with `--lossy --cross-format --min-score 90`:

- 42 files applied, 0 failed; every `res/` candidate was compiled by AAPT2 first.
- `aapt2 compile` + `aapt2 link` succeeded before and after; the linked resource
  table has the **same 51 resource names**.
- **Source savings: 99,516 bytes. Measured resource-APK savings: 89,030 bytes**
  (`resopt package-diff`). The two are reported separately because AAPT2
  re-compresses PNGs during the build.
- `resopt restore` brought the project back; the re-linked APK is
  **byte-identical** to the original.

Not validated: a Gradle build of the real app (it depends on private Gradle
plugins and no Gradle distribution is installed here) and on-device rendering
(no app build to install). AAPT2 copies WebP files without decoding them, so
WebP content validity rests on resopt's own decode-and-compare step.

## SVGA

233 unique SVGA files from the three projects: 231 SVGA 2.x files optimized and
verified (non-image protobuf fields byte-identical, image keys and order
identical, every embedded PNG pixel-identical), 2 SVGA 1.x zip files refused with
a reason. With lossless PNG reductions, which SVGA uses by default:
34,799,422 → 30,425,799 bytes (12.6% saved); no file grew. libsvga itself is a
parse-only Zig library that is not on crates.io, so it cannot be a dependency of
a published crate; its bundled `svga_probe` produced identical output for
original and optimized samples as an independent cross-check.

## Evaluated and not implemented

- **SVG**: minifiers change path data and numeric precision; a safe path needs
  render-and-compare verification that resopt does not have. Inventory only.
- **Audio/video**: every meaningful reduction is a lossy transcode whose
  acceptability depends on the playback path. resopt reports codec, duration and
  bitrate through optional `ffprobe` and performs no transcoding.
