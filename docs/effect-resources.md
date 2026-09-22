# Effect resource previews

## VAP in MP4

Analysis recognizes a bounded top-level `vapc` box in MP4 files and records the
VAP 2 canvas, frame count, frame rate, RGB/alpha rectangles and dynamic-source
count. It stages an unchanged copy for local playback, without an encoder or
ffprobe requirement. Ordinary MP4 files retain their existing media inspection.
Invalid sizes, duplicate configs, unsupported timing and out-of-bounds rectangles
are rejected. Config JSON is capped at 1 MiB, with at most 10,000 container boxes.

The browser reconstructs transparency from the alpha region's decoded red channel
and renders the RGB region into a canvas capped at 640 pixels on its longest side.
Play/pause and frame scrubbing use the browser's decoder. A bounded local blob
keeps seeking available on static hosts without HTTP Range support. Switching
resources stops callbacks, cancels loads, releases object URLs and pauses video.
Open with `resopt serve` (or a local HTTP host); file-URL fetch restrictions can
prevent media loading. No runtime CDN requests or source transcoding occur.

Dynamic app-provided text/image overlays are counted and explicitly excluded from
the preview. This is not a frame-equivalence check against a native player, and
no animation optimization or automatic merging is offered.

Validation: malformed-container/region fixtures, an end-to-end report preserving
source bytes, and real VAP playback/scrubbing of a 1125×2436, 25 fps, 31-frame
WeNext rocket effect. Removing alpha-plane reconstruction makes the pixel test
fail, confirming that ordinary opaque video rendering is not equivalent.

Format reference: https://github.com/Tencent/vap/blob/master/Introduction.md

## PAG content in TCMP4 files

Inventory recognizes the `PAG` signature independently of the suffix. Analysis
validates the uncompressed version-1 header and body length, caps input at 16 MiB,
and stages unchanged source bytes. Full decode is performed by the browser SDK;
a recognized header does not establish rendering compatibility.

Reports containing PAG content carry a pinned libpag 4.3.51 JS/WASM renderer and
its complete license notices. Its canvas, frame rate/count, editable text/image
counts and embedded video count appear after decode. Play/pause and frame
scrubbing preview original template content. App-provided replacements can differ.
Preview canvas size is capped at 640 pixels; source canvas is capped at 8192 per
side / 16 megapixels and duration at one hour. Unsupported inputs show an explicit
preview error and never become conversion candidates.

The SDK requires dynamic JavaScript bindings. It runs in an `allow-scripts`-only
iframe without `allow-same-origin`, with an opaque origin and `connect-src 'none'`.
The frame's policy permits the SDK's evaluator, while the main report keeps its
no-eval policy. The parent passes only the selected source and pinned WASM bytes
by transferable buffers. Playback messages are validated and accepted only from
that frame. No external network, parent DOM or mutation API is available to it.
Changing selection aborts parent loads and removes the frame and its decoder.

The vendored files are reproducible with `scripts/vendor-libpag.py`, using the
pinned npm package SHA-512. See `src/ui/vendor/libpag/README.md` for provenance and
artifact hashes. Version 4.4.31 failed its VectorString binding initialization in
this environment; 4.3.51 was verified with real TCMP4 content. The SDK is deliberately
pinned rather than fetched at runtime. Open reports through `resopt serve` or a
local HTTP server; browser file-URL restrictions can block buffer loading.

Validation includes mislabeled-file detection, malformed headers, offline runtime
staging and native-server playback of the real 590×332, 25 fps, 50-frame honor
animation. Removing content-signature detection makes the TCMP4 inventory test
fail. Playback is an inspection feature, not evidence for safe transcoding or
native-device pixel parity.
