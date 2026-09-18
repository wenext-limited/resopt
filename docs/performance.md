# Analysis startup performance

Measured on the same local Apple Silicon machine with release builds on 2026-09-18. Inputs were disposable copies; neither source project was optimized. All runs retained the default quality levels (75, 85, 95), two workers, PNG effort 2, full-size scoring and Alpha checks. WebP was disabled for this like-for-like comparison because v0.5.0 did not offer it.

## Results

| Fixture | v0.5.0 | Updated analyzer | Reduction |
|---|---:|---:|---:|
| Eight large project PNGs (five distinct contents), 7.45 MB / 4.91 million pixels | 6.32 s | 4.20 s | 33% |
| Same eight files copied three times (24 files, five distinct contents) | 18.17 s | 4.36 s | 76% |

These are isolated end-to-end comparison runs, not a whole-project guarantee. The larger fixture intentionally stresses reuse. A read-only inventory of the iOS checkout contained 2,127 eligible images and 1,911 distinct format/content combinations (216 duplicate copies); actual speedup depends on image sizes and the eligible format policy.

Per-resource candidate format, quality, encoded byte count, validity and savings remained unchanged on the fixtures. The maximum SSIMULACRA2 score difference was zero.

## Changes and ablation

1. Identical decoded images return the exact score of 100 without constructing multiscale metric buffers.
2. For multiple quality/format trials, reuse SSIMULACRA2's precomputed source data. A three-run comparison with only this scoring change reduced the median from 6.37 s to 5.65 s. Reference caching is bounded by a two-million backdrop-pixel budget (about 96 MiB of retained reference pyramids per worker); larger images keep the existing strip path. This trades some memory for CPU time.
3. Group byte-identical eligible images during a run. Candidate policy (Apple catalogs vs loose files vs Android resources) is part of the grouping key. Recheck source hashes before sharing immutable artifacts, retain each resource's own path, and fall back to independent analysis if a source changes or the first analysis fails. Adding this reuse produced the final results above.
4. Show completed results during analysis: progress, number of opportunities, accumulated savings and the top 20 completed opportunities. The complete editable report opens only after analysis and verification finish. In browser verification, six of eight resources and 2.20 MiB of savings were visible before completion.

No reduced-quality scoring, fewer default quality levels, weaker verification, or persistent stale-result cache was used for these timings.
