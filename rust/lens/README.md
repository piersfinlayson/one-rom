# One ROM Lens

One ROM Lens (`onerom-lens`) is a browser tool that runs the One ROM PIO/DMA
serving algorithms and visualises the resulting waveforms — a cycle-exact logic
analyser for the emulated ROM.

It compiles to WebAssembly and runs the **real** firmware serving code via
[`onerom-fw-emulator`](../fw-emulator); the browser front-end
([`web/logic-analyzer.js`](web/logic-analyzer.js)) drives it through a small C
ABI (`onerom_*`) exposed with Emscripten's `ccall`/`cwrap`.

Like the emulator, the ROM image, board and pin geometry are baked in at build
time from a `CONFIG` + `BOARD`, so each build targets one hardware variant and
ROM image. To look at a different chip, rebuild with a different config/board.

## Prerequisites

- The Emscripten SDK on `PATH` (`emcc`, `emar`). Install with
  [`ci/install-emscripten.sh`](../../ci/install-emscripten.sh), or on macOS
  `brew install emscripten`.
- The Rust wasm target: `rustup target add wasm32-unknown-emscripten`.
- `python3` (for the local web server).

## Build and run

From the repo root:

```bash
rust/lens/serve.sh [CONFIG] [BOARD] [PORT]
```

For example, a random-content 2316 on a Fire 24 rev A:

```bash
rust/lens/serve.sh onerom-config/test/24-random-23xx.json fire-24-a
```

Then open <http://localhost:8000/> in a browser. With no arguments the script
defaults to that config on `fire-24-a`, port `8000`.

## Building the wasm directly

`serve.sh` is a thin wrapper over a normal cargo build:

```bash
cd rust
CONFIG=onerom-config/test/24-random-23xx.json BOARD=fire-24-a \
    cargo build -p onerom-lens --target wasm32-unknown-emscripten
```

This emits `target/wasm32-unknown-emscripten/debug/onerom-lens.js` (the
Emscripten loader, a `OneRomLens()` factory) and `onerom_lens.wasm`. Serve those
alongside the files in [`web/`](web/).

The wasm build is exercised in CI by
[`ci/build-lens.sh`](../../ci/build-lens.sh) (an 8-bit 2364 and a 16-bit 27C400).
The underlying serving correctness is regression-tested by the emulator suite
(`ci/test-emu.sh` / `pio-tester`), which shares this crate's engine.
