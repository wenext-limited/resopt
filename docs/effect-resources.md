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
