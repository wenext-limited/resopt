# macOS app

This is a small SwiftUI shell around the existing local Rust review server. It
selects a project directory, starts the bundled `resopt web --no-open` process,
and displays its loopback page in a `WKWebView`. The app does not upload files.

Build on macOS with Xcode and Rust installed:

```sh
macos/build-app.sh
open dist/Resopt.app
```

The default build is optimized. Set `RESOPT_PROFILE=debug` for a faster local
build, or pass a new output path as the script's first argument. The script
refuses to replace an existing app; remove that build or choose another path.
It signs both executables ad hoc. Distribution requires a Developer ID
signature and notarization; an App Store sandbox build also needs an explicit
review of directory access for the bundled child process.

Reports are retained under `~/Library/Application Support/resopt/Reports` so
previous image replacements can be restored. Use **Open Existing Report** to
reopen a report directory after relaunching the app. Keep that directory until
you no longer need its backups. Closing the app stops its local server.

The **Parallel Tasks** picker controls how many different images are analyzed
at once (2, 4, or 8). On Macs with at least four active processors, the app
defaults to four tasks; smaller Macs default to two. Changes take effect on
the next scan. More tasks can shorten analysis but use more memory.

In the embedded report, Shift-click selects a range and Option/Alt-click toggles
individual resources. When multiple resources are selected, **Batch apply** and
**Restore selected** are limited to that selection. Choose the lossless, lossy,
warning, and format-change policy, review the generated plan, then confirm once.
Every applied file remains an individually restorable operation, and **Restore
all** resolves shared-file dependencies in the correct order.
