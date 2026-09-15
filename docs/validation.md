# Initial validation — 2026-09-15

Environment: macOS, Rust 1.97.1. Embedded backend: Oxipng 10.2.1 with strict
PNG verification. This is local evidence; remote CI and the declared minimum
Rust version have not been executed here.

## Automated checks

- Formatting and Clippy with warnings denied.
- 19 passing integration tests, including CLI scan/plan/apply/restore.
- One compiled public API documentation example.
- Release build.

Regression coverage includes nonzero RGB under zero alpha, 16-bit samples,
non-IDAT metadata preservation, stale files/catalogs, unsafe rehashed candidates,
duplicate renditions, exclusions, symlinks, locks, repeat application, partial
batch recovery, and refusal to overwrite subsequent user edits.

## Real-resource sample

A read-only GameParty scan found 27 catalogs, 1,592 referenced files, and 1,408
PNG files eligible for analysis, with no diagnostics. Nested `.worktrees` were
excluded. These counts describe catalog files on disk, not target membership.

Six static PNGs were selected at evenly spaced indices from the size-sorted
eligible AppUI/Modules assets between 50 KiB and 1 MiB. The files were copied to
a temporary project; its catalog entries referenced only the selected renditions.
No live project assets were modified.

Default policy: effort 2; input >= 51,200 bytes; save >= 1,024 bytes and >= 1%.

| Sample source bytes | Result bytes | Outcome |
| ---: | ---: | --- |
| 51,887 | 45,032 | Applied on copy |
| 70,789 | 57,988 | Applied on copy |
| 95,393 | 39,248 | Applied on copy |
| 151,362 | 104,261 | Applied on copy |
| 257,972 | 235,043 | Applied on copy |
| 945,829 | 945,829 | Kept original; savings below policy threshold |
| **1,573,232** | **1,427,401** | **145,831 bytes saved (9.27%)** |

The five accepted candidates passed decoded-sample and non-IDAT-chunk equality
checks. Applying and restoring the temporary sample succeeded; all six restored
files matched their original SHA-256 hashes. The six live source files also
retained their original hashes.

This is a small source-size smoke test, not a representative benchmark or an
app-size measurement. No Xcode compilation, device rendering, download-size,
decode-time, or memory-performance claim follows from it.

## Ablation study

Two checks were separately removed in a disposable copy of the crate. Each
mutation was tested against its dedicated regression test:

| Removed check | Observed regression |
| --- | --- |
| Decoded-sample equality | A candidate with changed pixels and an updated hash was accepted; the pixel-safety test failed as expected. |
| Non-IDAT chunk equality | A candidate with removed text metadata and an updated hash was accepted; the metadata-safety test failed as expected. |

Both checks remain enabled in the delivered implementation. These experiments
show why artifact hashes alone cannot establish the lossless contract.

The first version has no Numi integration or external media-tool dependency.
The real sample demonstrates that the PNG workflow is useful without either;
JPEG, lossy modes, HEIC, audio/video, and compiled-size analysis remain future work.
