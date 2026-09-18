# Development

The CLI is Rust. Maud generates the report HTML embedded in the binary. End users do not need a frontend toolchain.

## Native checks

```sh
cargo fmt --all --check
cargo clippy --locked --workspace --all-targets -- -D warnings
cargo test --locked --workspace --all-targets
cargo test --locked --workspace --doc
node --test tests/report-ui.cjs
```

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
