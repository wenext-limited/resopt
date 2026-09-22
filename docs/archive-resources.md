# ZIP resource inspection

ZIP resources are analyzed as parent packages with their own searchable,
paginated entry list. Each entry shows its original path, format, expanded bytes,
and compressed bytes. Up to 32 of the largest decodable images get previews.
Nested ZIP entries are listed but are not recursively expanded in this version.
No paths are extracted into the source project. Report thumbnails use generated
numeric names, never archive entry paths.

Inspection bounds are 64 MiB input, 10,000 entries, 64 MiB per expanded entry,
and 256 MiB total expanded bytes. Image decoding retains the analysis pixel cap.
ZIP64/multidisk archives, encrypted entries, symlinks, non-UTF-8 or unsafe paths,
case-insensitive path collisions, and file/directory conflicts are rejected.
Preview decoding errors remain visible on individual entries. Cancellation stops
preview work. Archive inspection bypasses the single-image cache because its
preview set differs from an image candidate's artifacts.

Package manifests (`manifest.json`, `pack-info.json`, `sniff.json`) and signature
metadata are identified for the package publisher. Archives with these integrity
contracts must not be silently rewritten. Finder and AppleDouble entries are
labeled, not deleted. Sizes in the contents list are not additional top-level
resources and do not inflate total potential savings.

Validation includes hostile path/size fixtures, archive entry round-trip reads,
manifest detection, and an end-to-end report that preserves source bytes and
creates a preview without extracting the source paths. The path-validation
ablation removes the unsafe-name guard and makes its regression fixture fail.

## Lossless embedded PNG optimization

Ordinary ZIP resources without recognized integrity metadata can produce one
lossless, same-format ZIP candidate. Only static PNG image data is recompressed;
filenames, decoded pixels, PNG metadata, atlas files and all other file payloads
stay unchanged. Unchanged entries are copied as their original compressed streams.
PNG entries with extra ZIP metadata or unsupported layout remain unchanged.
Both candidate creation and apply independently verify entry order, paths,
permissions, timestamps, comments, extra data and every expanded payload.

The actual ZIP file size determines savings, never the sum of expanded PNG sizes.
Apply and restore operate on the whole package using the existing source hashes,
project lock and recovery journal. Cancellation stops between entries. Nested
archives are not rewritten. Hash maps in Cocos/custom JSON block rebuilding;
Spine skeleton hashes refer to the unchanged skeleton and do not block texture
recompression. External signatures and hashes cannot be inferred from the ZIP:
packages managed by an external publisher still require its release process.

On a temporary copy of the real 357-entry Ludo ZIP, effort 0 produced a verified
candidate saving 392,632 bytes (383.43 KiB). Apply and restore returned the temporary
source to the exact original bytes. This is source ZIP savings, not IPA savings.
The payload-check ablation admits a changed atlas and fails the regression test.
