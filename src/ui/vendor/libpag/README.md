# Bundled PAG browser runtime

Unmodified artifacts from Tencent's `libpag` npm package, version **4.3.51**.
Source repository: https://github.com/Tencent/libpag
Package: https://registry.npmjs.org/libpag/-/libpag-4.3.51.tgz
License: Apache-2.0 and the included third-party notices in `LICENSE.txt`.

Only `lib/libpag.min.js`, `lib/libpag.wasm`, and `LICENSE.txt` are included.
`libpag.min.js` is stored as `libpag.js`; its bytes are unchanged.
Use `python3 scripts/vendor-libpag.py` from the repository to reproduce the files.
The script verifies the pinned package SHA-512 before copying only these entries.
There is no network access, package installation or build step at runtime.

SHA-256:

- JS: `d3a1ed1078c5be9a78214414004c61daf605013d63e892a02cc6d46804d7560c`
- WASM: `513fa7af309564855e701b706fcf97b0dcf57c4444906b513c20890b35d64ee6`
- License: `8740cd229af587a2a6e67601cf9c932777eac31bd8bd5c080da8f03b88ccda72`

This version is intentionally pinned. Updating it requires rechecking actual
TCMP4/PAG samples, scrubbing, disposal, and the native report server's CSP.
Unrecognized vendor extensions or runtime incompatibilities remain preview errors;
the renderer is not used as proof of lossless conversion.
