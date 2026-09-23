# Similar image review

The native report and live inspector show one result row per similarity group,
with member thumbnails, total source size, and the lowest–highest member score.
The tab count and pagination count groups. Searching or filtering any member
retains its whole group; selecting the row opens all members together. The score
range excludes the reference itself and remains unavailable for unscored reports.

Each group is compared against a fixed reference
(the largest file, with report index breaking ties). Each remaining member must
match that reference directly. A match between A and B plus a match between B and
C is insufficient to claim that C matches A. Groups are disjoint, so a resource
assigned to an earlier reference is not also shown in another group.

The detector keeps its 16×16 area-averaged luminance and opacity fingerprints,
plus the alpha-weighted mean RGB color. Luminance is composited over mid-gray.
Non-identical matches require detailed luminance, relative aspect difference at
most 3%, per-channel mean color difference at most 14/255, mean brightness
error at most 0.022, mean opacity error at most 0.03, and no brightness or opacity
cell error above 0.16. Animations require identical file hashes; matching poster
frames do not establish matching animations. Intended renditions of the same
asset are not compared directly.

## Score contract

For normalized differences in [0, 1]:

```
d = max(mean absolute luminance difference,
        mean absolute opacity difference,
        largest absolute mean RGB channel difference / 255)
score = min(99.9, 100 × (1 − d))
```

Identical SHA-256 file hashes receive 100 instead. The UI displays one decimal
place and separately shows the largest luminance/opacity cell difference so a
localized change is not hidden by the averages. Scores always refer to the
reference, even when another group member is selected. The score sort uses
similarity in the Similar groups view, and compression quality elsewhere.

This is a fingerprint agreement score, not a probability, SSIMULACRA2 quality
measurement, or proof that one file may replace another. A re-encoded image can
have identical fingerprints but different bytes; it receives 99.9. Tiny details
can disappear during averaging or 8-bit fingerprint quantization. Local color
changes with equal luminance and equal global mean color can also be missed.
Flat images only match by file hash. Check the previews and resource usage before
removing anything. Bytes in other group members are not confirmed savings.

Existing reports without scores remain readable and explicitly request a new
analysis for scores. Refreshing HTML alone cannot recover discarded fingerprints.
No cached fingerprint format changes are required.

## Validation and ablation

Synthetic badge images isolate tint while preserving shape and opacity. The
focused Rust fixture produces these scores against the same reference:

| Change | Score |
| --- | ---: |
| Identical bytes | 100 |
| Identical fingerprint, different file bytes | 99.9 |
| Red tint 0.90 → 0.89 | 99.2 |
| Red tint 0.90 → 0.86 | 96.9 |

These examples verify ordering, not perceptual calibration on a real dataset.

Each safeguard was temporarily removed, its focused regression test executed,
and production code restored:

| Ablation | Observed regression |
| --- | --- |
| Remove the 99.9 cap | Different file hashes with equal fingerprints score 100; score-contract test fails |
| Ignore peak opacity error | One fully changed alpha cell passes because its average error is only 1/256; local-opacity test fails |
| Permit a match to any existing member | A~B~C becomes one group although A and C fail direct matching; reference test fails |

Additional coverage includes resized badges, intended Apple/Android variants,
large tint/shape/opacity differences, flat and tiny images, malformed fingerprints,
animated posters, reference ordering, serialization, and old reports without scores.

The group-list projection was also ablated: returning individual matched files
instead of group references makes the 120-file / two-group pagination and
non-reference search regression fail. With grouping restored, the two groups
occupy two rows on one page and searching a member retains all its peers.

## Files removed during a live review

After analysis, the server's authenticated presence endpoint checks original paths
and any recorded conversion targets without hashing image payloads. The browser
polls every two seconds and refreshes only after a change, preserving playback
and scroll position on unchanged ticks. Deleted files are hidden from results,
summary totals and batch previews. Groups with fewer than two present members
are hidden. If a reference disappears, remaining members get a new reference
label but their old comparison scores are cleared until re-analysis.

Report indexes, analysis JSON and recovery journals are not rewritten. Returning
a file to its original path makes its analyzed row visible again. An unavailable
project root or a failed check preserves the last known list and shows a retry
notice. New files and changes to file contents require a new analysis; a static
HTML report has no live filesystem connection. Presence checks defer during
in-app apply/restore and server batch operations.

Validation covers deleted directories, reappearing files, authenticated polling,
conversion target paths, group/reference removal, and unchanged-tick behavior.
Removing missing-member filtering reproduces the stale-group regression.
