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
