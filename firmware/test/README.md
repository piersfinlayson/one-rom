# Firmware test support

Two different kinds of thing live here, driven by different harnesses. Which
one a test belongs in comes down to whether it needs the firmware running as a
whole.

## Emulator support — `ffi.c`, `stub_rp235x.c`

These are not tests. They are the glue that lets the whole firmware be built
for the host and driven from Rust:

- `stub_rp235x.c` replaces the RP235x routines that touch hardware registers,
  so the firmware links and runs without a chip under it.
- `ffi.c` exposes firmware internals — runtime info, PIO and DMA state — to the
  Rust side.

They are built by [`../test.mk`](../test.mk), whose `SRCS` compiles them
alongside the real `firmware/src` sources. The tests that use them live in the
Rust workspace, in `onerom-fw-emulator` and `onerom-fw-tester`, and are run
from [`../../ci/test-emu.sh`](../../ci/test-emu.sh) — a full sweep, several
hours, so run one board and config at a time while developing:

    env BOARD=fire-24-a CONFIG=onerom-config/test/24-random-23xx.json make test-emu
    env BOARD=fire-24-a CONFIG=onerom-config/test/24-random-23xx.json make test-api

`test.mk`'s `WASM=1` mode cross-compiles the same firmware to WebAssembly for
One ROM Lens.

## Host C unit tests — `rtt/`

Self-contained C tests that compile one firmware source for the host and
exercise it directly, with no chip, no emulator and no Rust. They suit logic
that is pure enough to run anywhere, and they are quick — a second or so, so
they can be run on every change.

Run them all with:

    ci/c-tests.sh

Each test directory supplies its own stand-in for the headers the source under
test includes, which is what keeps the test independent of a full firmware
build. `rtt/include.h` stands in for `firmware/include/include.h` and provides
just the SEGGER control block layout that `firmware/src/rtt.c` needs, so the
test directory must come first on the include path — see
[`../../ci/c-tests.sh`](../../ci/c-tests.sh).

They are built with `-fsanitize=address`, so a memory error is reported at the
point it happens rather than as a puzzling failure later. That is not
incidental: `rtt/test_rtt.c`'s `test_rdoff_clamp` exists because a host written
`RdOff` can otherwise drive a write past the end of the ring buffer, and
AddressSanitizer is what turns that into a precise, legible failure.

### Adding one

Add the sources under a new subdirectory here, then add a `run_test` line to
`ci/c-tests.sh`. Keep to the existing shape: arrange, stimulate, then check
both that the intended thing happened and that the unintended thing did not.
A test that only shows nothing changed will not catch a regression.

## `old/`

Superseded tests from before the emulator existed, kept for reference. Not
built, and not run by anything.
