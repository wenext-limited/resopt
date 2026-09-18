# Development

The CLI is Rust. Maud generates the HTML shell and the UI scripts in `src/ui/` are embedded in the binary, so end users need no frontend toolchain. See [architecture.md](architecture.md) for module responsibilities and invariants, and [acceptance.md](acceptance.md) for the requirement checklist.

## Native checks

```sh
cargo fmt --all --check
cargo clippy --locked --workspace --all-targets -- -D warnings
cargo test --locked --workspace --all-targets
cargo test --locked --workspace --doc
node --test tests/report-ui.cjs
```

Tests that need platform tools skip themselves when the tool is missing: ImageIO tests are macOS-only, and the AAPT2 test runs when Android SDK build-tools are installed (`ANDROID_HOME` or the default SDK location).

### Real-browser verification

`tests/browser/e2e.mjs` drives the embedded UI in headless Chrome through the DevTools protocol (Node 22+, no npm packages). It **modifies the project it is given**, so always pass a disposable copy:

```sh
cargo build --release
mkdir /tmp/resopt-e2e-project && node tests/browser/fixture.mjs /tmp/resopt-e2e-project   # or copy a real project
node tests/browser/e2e.mjs target/release/resopt /tmp/resopt-e2e-project /tmp/resopt-e2e-shots
```

It checks live results, filters, keyboard use, control labels, themes, language switching, the comparison dialog, warning confirmation, single apply/restore, batch apply, restore-all (byte-exact project snapshot afterwards), phone-width layout, reduced motion and the absence of script errors. Set `CHROME_BIN` if Chrome is not in a standard location.

### SVGA corpus check

```sh
RESOPT_SVGA_SAMPLES=/path/to/svga/files cargo test --release --lib svga_samples -- --ignored --nocapture
```

### Working with real projects

Analysis is read-only, but apply/restore tests must run on copies. Never point `apply`, the browser test or a batch at a working checkout you care about without a clean VCS state.

## Browser/WASM edition

Requirements: Rust 1.89+, a C compiler for native libwebp/libdeflate, Clang with a WebAssembly backend, Bun 1.3.14.

```sh
rustup target add wasm32-unknown-unknown
rustup component add llvm-tools-preview
cargo install wasm-bindgen-cli --version 0.2.128 --locked
cd web
bun install --frozen-lockfile
bun run build
bun run typecheck
bun test
bun run preview
```

`bun run dev` enables page/CSS hot updates. Restart after changing Rust, the Worker, or Maud HTML. The build script uses Rust's `llvm-ar`; `WASM_BINDGEN`, `AR_wasm32_unknown_unknown`, and `CARGO_TARGET_DIR` can override tool locations.

`web/dist` is a static HTTP(S) site; Worker and WASM files must remain same-origin. The public site has no upload API. `bun run host` is a macOS-only loopback development adapter, while the product entry point is `resopt web`.

`src/portable.rs` holds the byte-oriented PNG API. `crates/resopt-wasm` provides thin wasm-bindgen bindings. PNGs in the browser are limited to 16 MiB and 2,097,152 pixels, with 500 selected images and 128 MiB of result cache.

Native API integration tests run with `RESOPT_TEST_BIN=/absolute/path/to/resopt bun test tests/native-integration.test.ts`. GitHub Actions runs Rust checks on macOS, Linux, and Windows, and publishes a browser-site build artifact separately from CLI releases.
