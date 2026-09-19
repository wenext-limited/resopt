# Performance

Measured 2026-09-19 on one Apple Silicon Mac (macOS, release builds, otherwise
idle). Every run preserved the established quality levels (75/85/95), PNG effort
2, full-size SSIMULACRA2 scoring and Alpha checks. No run reduced quality,
skipped verification or lowered the number of candidates.

## Input

A disposable copy of a production iOS project (the original checkout was never
written to): 7,453 files, 115 MB, 2,505 raster images of which 1,941 PNG, 474
HEIC, 82 WebP and 8 JPEG, in 31 asset catalogs plus loose resources. SVGA files
were removed from the copy and `ffprobe` was taken off `PATH`, because v0.5.0
cannot process either; every binary therefore did the same image work. WebP
candidates, which v0.5.0 does not have, were off in every run; they have since
become the default (`--no-webp` restores the measured configuration), so a cold
default run now does additional WebP encoding on loose files.

## Results and ablation

Each row adds one change to the row above it.

| Run | Wall time | First result | Peak RSS | vs. v0.5.0 |
|---|---:|---:|---:|---:|
| A. v0.5.0, 2 workers | 378.9 s | 1.48 s | 1,433 MiB | — |
| B. main before this work (scorer reuse, in-run duplicate reuse), 2 workers | 346.6 s | 1.42 s | 1,352 MiB | −9% |
| C. new pipeline (largest-first order, duplicate sharing without a serial pre-hash pass, not-smaller candidates skipped before scoring), artifacts still fsynced, 2 workers | 270.1 s | 1.24 s | 1,054 MiB | −29% |
| D. + no fsync for regenerable report artifacts, 2 workers | 244.3 s | 1.23 s | 1,018 MiB | −36% |
| E. + automatic worker count (8) with the pixel budget — **the default** | 152.3 s | 1.35 s | 1,393 MiB | **−60%** |
| F. + persistent cache, cold (writes entries) | 150.1 s | 1.29 s | 1,377 MiB | −60% |
| G. + persistent cache, warm (unchanged project) | **2.4 s** | 1.22 s | 131 MiB | −99% |

The lossless PNG output of the new analyzer is byte-for-byte the same total as
v0.5.0 (18,225,980 bytes across all PNG resources), and it produced 3,248
candidate artifacts against 3,233 for the previous main.

On the 7,078-file Android project (2,003 PNG, 1,555 WebP, `--webp` enabled) the
fsync change alone took whole-project analysis from 86.0 s to 39.0 s, because
that project writes many more small previews.

## Where the time goes

Per-phase timers (`resopt analyze --timings`, summed over workers) for run D:

| Phase | Seconds | Share |
|---|---:|---:|
| Lossy encoding (ImageIO JPEG/HEIC, 3 qualities × 1–2 formats) | 364.9 | 76% |
| Scoring (SSIMULACRA2 + RGB/Alpha error) | 51.8 | 11% |
| Lossless PNG encoding (oxipng) | 28.0 | 6% |
| Decoding (originals and candidates) | 25.5 | 5% |
| Previews | 9.7 | 2% |
| Writing artifacts | 1.9 | <1% (58.1 s before the fsync change) |
| Hashing | 1.3 | <1% |
| Project scan (one pass, Git-aware) | 1.0 | <1% |
| Report generation | 0.04 | <1% |

Two findings shaped the work:

- **fsync was the largest avoidable cost.** Forcing every preview and candidate
  to disk cost 58 s here and 420 s (summed) on the Android project. Artifacts
  are regenerable, so only the journal, backups and `analysis.json` are synced.
  The same applies to cache entries, which are hash-verified when read.
- **HEIC encoding does not scale with workers.** Going from 2 to 8 workers
  nearly tripled the summed lossy-encoding time (365 s → 1,052 s) while wall
  time fell 38%: the system encoder serializes internally. This is the floor
  for cold macOS runs with HEIC enabled; `--qualities 85` cuts it by two thirds
  for a first pass.

Time to first result is dominated by the project scan (about 1 s) plus the
first image, and was already good; the live page now shows that first result
instead of waiting for the whole report. Browser rendering stays flat because
rows are streamed in pages of 500 and the list renders 50 rows at a time; the
static `report.html` for the Android project is 9 MB and opens directly.

## Bounded resources

Workers default to the CPU count, capped at 8. Decoded source pixels in flight
are limited to one maximum-size image (`--max-pixels`), so extra workers help
ordinary assets without multiplying worst-case memory. Peak RSS stayed below
v0.5.0's in every configuration.

## Reuse rules

Work is shared only when the source bytes, the resource's policy class
(catalog / loose / Android area, type, nine-patch, `minSdk`, locks), every
option that affects candidates, the tool version and the codec backend all
match. Tests cover content changes, option changes, corrupted and truncated
entries, interrupted writes, foreign entries and traversal attempts
(`src/cache.rs`, `tests/pipeline.rs`).
