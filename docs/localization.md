# Localization review

resopt reads localization files with [langcodec](https://crates.io/crates/langcodec)
and reports, for each table, how complete every language is and where a
translation disagrees with the source language. This is inspection only:
files are parsed from the bytes that were hashed, nothing is written back,
and no candidate or apply action is offered.

## What is read

| Files | Table | Source language |
|---|---|---|
| `Name.xcstrings` | The catalog itself | The catalog's `sourceLanguage` |
| `<lang>.lproj/Name.strings` | All `.lproj` folders of one directory with the same file name | `Base`, else `en`, else the language with the most keys |
| `<lang>.lproj/Name.stringsdict` | Same, kept separate from `.strings` | Same |
| Android `res/values[-<locale>]/*.xml` with `<string>` or `<plurals>` | All locale folders of one `res/` directory with the same file name and the same non-locale qualifiers (`values-night`, `values-v21` stay separate) | The default `values/` folder |

UTF-8 and BOM-prefixed UTF-16 `.strings` are both read; compiled
binary-plist `.strings` (from build products) are left as inventory rows.
Android `values` XML without strings (colors, dimensions, styles) keeps its
ordinary row. `<![CDATA[…]]>` text is part of the value. Keys marked
`translatable="false"` or "Don't translate" are excluded from every count.
In a catalog, a key with no source value counts as translated in the source
language, because Xcode shows the key itself. Android folders that normalize
to the same language (`values-in` and `values-id`) keep separate rows,
labelled with their folder, instead of being merged.

## What is reported

- **Coverage per language:** translated, missing, empty, needs review
  (marked "needs review" or "stale" by the translation state) and extra keys
  (present in a translation but no longer in the source).
- **Stale keys:** keys Xcode marked as no longer found in code. They are
  candidates for removal after you confirm they are unused.
- **Issues**, each with the key, language and the translation text:
  - `placeholder_type` — the same arguments with different types, for
    example `%d` in the source and `%@` in a translation. Formatting such a
    string can crash or print garbage.
  - `placeholder_count` — arguments missing, added or renumbered.
  - `empty_value` — an explicitly empty translation.

Placeholders are compared by argument identity: `%@ %d` equals
`%1$@ %2$d`, reordered positional arguments are equal, `%@` equals Android
`%s`, and integer (`%d`/`%ld`/`%u`) or floating-point (`%f`/`%g`) variants are
treated as the same type. A percent sign followed by a space and a word
(`5% bonus`) is prose, not a placeholder. Plural forms count towards coverage
but are not compared for arguments: a language may omit the number from its
`one` form, and the file does not say which argument selects the form.

The source file (or a single catalog) lists the issues of every language; a
translation file lists and counts only its own. Each file lists at most 500
issues, and the counts always include all of them.

## Limits

- Values that exist only as device variations or substitutions in an
  `.xcstrings` catalog count as missing.
- `.stringsdict` files with nested, select/gender or multi-variable rules
  are listed as unsupported, with the parser's reason.
- `%@` and `%s` are treated as the same argument, so an Apple translation
  that uses the C-string `%s` where the source has `%@` is not flagged.
- Stopping an analysis skips the whole localization pass: tables span files,
  and a partial table would be compared against the wrong source.
- A table is compared within itself. Keys used from code, unused keys outside
  Xcode's stale marking, and strings in other formats (JSON, Flutter `.arb`,
  gettext `.po`) are not analyzed.

Validation: on the three WeNext projects (1,268 localization files,
478k entries, including Git worktrees) every file parsed without errors. Android
coverage matched an independent count exactly (766 translatable keys, 4
missing in `values-ar`). Sampled argument issues were real: `%@個粉絲` against
the source `%d fans`, and a Turkish translation that turned `%@` into `1$s %@`.
