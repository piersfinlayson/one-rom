# Adding a Chip Type

How to teach One ROM to emulate a chip it does not yet know about.

This describes the **v2** path — firmware v0.7.0 and later, which is Fire
(RP2350) hardware.  V2 derives how to serve a chip from the chip's pinout and
the board's routing, so a new chip type is usually a data change plus a one-line
declaration.  The pre-v0.7.0 (v1) path is closed to new chip types, and its
per-chip code in [`rust/gen/src/image.rs`](/rust/gen/src/image.rs)
(`chip_type_c_enum_val`, `handle_snowflake_chip_types`) is not part of this.

## The short version

1. Add the chip to [`rust/config/json/chip-types.json`](/rust/config/json/chip-types.json).
2. Add its `ChipType` to `SUPPORTED_CHIP_TYPES` in [`rust/gen/src/v2/builder.rs`](/rust/gen/src/v2/builder.rs).
3. Run the generators.
4. Look for the chip in [`docs/COMPATIBILITY.md`](/docs/COMPATIBILITY.md).  If it is there, it serves.
5. If it is not, find out why with the `layout` tool — and only then consider firmware work.
6. Add it to the emulator test sweep, so it stays working.

## 1. The JSON entry

Everything One ROM knows about a chip comes from this one file.  The
`onerom-config` build script reads it, validates it, and generates the
`ChipType` enum and [`docs/CHIP-TYPES.md`](/docs/CHIP-TYPES.md).  The
`onerom-metadata` build script generates the firmware's `onerom_rom_type_t` C
enum and chip-size array from the same data.  Nothing is written twice.

Here is the 2364 entry — about as simple as an entry gets — with its arrays
compacted onto single lines:

```json
"2364": {
    "description": "8KB mask ROM with 1 configurable CS line",
    "function": "ROM",
    "rbcp_chip_type": 2,
    "supported": "0.4.4",
    "aliases": ["4764", "MCM68764", "MCM68A764", "MM52164", "MK36000"],
    "bit_modes": [8],
    "pins": 24,
    "size": 8192,
    "address": [8, 7, 6, 5, 4, 3, 2, 1, 23, 22, 19, 18, 21],
    "data": [9, 10, 11, 13, 14, 15, 16, 17],
    "control": {
        "cs1": { "pin": 20, "type": "configurable" }
    },
    "power": [
        { "name": "VCC", "pin": 24, "voltage": "+5V" },
        { "name": "GND", "pin": 12, "voltage": "0V" }
    ]
}
```

- **The key** is the chip's primary name.  Unless the chip is RAM it must start
  with `23`, `27`, `28`, `SST39SF` or `HM76`, or the build panics with
  `Unsupported chip type X - needs adding to chip_family()`.  A part number
  outside those conventions needs an arm in
  [`rust/config/build/chip/mod.rs`](/rust/config/build/chip/mod.rs).
- **`rbcp_chip_type`** is the wire value: unique, and permanent once shipped.
  Take the next free number.  The firmware's C enum and chip-size array are
  generated from it.  It is also part of the
  [ROM Bus Control Protocol](https://github.com/piersfinlayson/rom-bus-control-protocol),
  so a new chip type needs a matching pull request against that repository — the
  value has to mean the same thing on both sides of the bus.
- **`supported`** is the firmware version the chip first ships in — the
  in-development version from the [`Makefile`](/Makefile).  Omit it and the chip
  is known but unsupported: absent from `COMPATIBILITY.md` and from
  `onerom firmware chips`, and marked `✗` in `CHIP-TYPES.md`.
- **`aliases`** are other names for the same silicon.  Every alias becomes
  selectable everywhere a chip type can be named — the CLI, the web programmer
  and Studio, as well as its own row in `COMPATIBILITY.md` — so a user can pick
  the number actually stamped on their chip rather than having to know it is the
  same part as something else.  Be generous with them.
- **`address`** lists the address lines in bit order, lowest first, as physical
  pin numbers, and **`data`** does the same for the data lines from D0.  `size`
  must equal `2^address.len()` (the 48KB 23QL384 is the sole exception).
- **`bit_modes`** is `[8]`, or `[8, 16]` for a 16-bit part with a `/BYTE` line.
  On a 16-bit part the lowest address line is **A-1**, not A0 — it selects which
  half of the word a byte-mode read returns, and it shares a pin with the top
  data line.  So the 27C400's 19 entries are A-1 to A18, and `2^19` is still its
  512KB.  Nothing keys off the chip name: anything that needs to know works it
  out from `bit_modes` containing 16.
- **`control`** names the enables.  `cs1`-`cs4` may be `configurable` (polarity
  mask-programmed, so the user supplies it), `fixed_active_low` or
  `fixed_active_high`.  `ce` and `oe` carry the JEDEC convention and must be
  `fixed_active_low`.  Mixing CS with CE/OE needs `"allow_mixed_control": true`.

[`rust/config/build/chip/validation.rs`](/rust/config/build/chip/validation.rs)
enforces the pin, size and control-line rules, and fails the build on a bad
entry.

## 2. Declare it servable

`SUPPORTED_CHIP_TYPES` in
[`rust/gen/src/v2/builder.rs`](/rust/gen/src/v2/builder.rs) is the list of chip
types v2 firmware serves.  Add the variant and bump the array length together:

```rust
pub const SUPPORTED_CHIP_TYPES: &[ChipType; 36] = &[
    ...
    ChipType::Chip2532,
];
```

Nothing else in the tooling or the firmware C needs touching for an ordinary ROM
chip type.  A **RAM** type also needs an arm in `rom_slot_type`
([`rust/gen/src/v2/rom_info.rs`](/rust/gen/src/v2/rom_info.rs)) — and no v2
firmware serves RAM yet, which is why `Chip6116` sits commented out of the list.

## 3. Run the generators

From `rust/`:

```bash
cargo run -p onerom-gen --bin compat                     # docs/COMPATIBILITY.md
cargo run -p schema-gen --bin schema-gen                 # onerom-config/schema.json
cargo run -p onerom-gen --bin layout -- --write-baseline # ci/layout-baseline.txt
```

`docs/CHIP-TYPES.md` needs no command — the `onerom-config` build script
rewrites it on any build.  All four are checked in and all four are a CI gate.
`ci/rust-tests.sh` regenerates them and fails if the committed copy differs, so
commit them with the change.

## 4. Did it work?

Look for the chip in [`docs/COMPATIBILITY.md`](/docs/COMPATIBILITY.md), or ask
the CLI — `cargo run -p onerom-cli --bin onerom -- firmware chips --board
fire-24-e`.  If it is listed, that is the whole job.  Check the flash cost while
you are there with `layout --check`: an image several times the chip's own size
is normal where a board's routing puts an unrelated signal inside the chip's
address range, but it is worth knowing.

## 5. When it does not appear

`cargo run -p onerom-gen --bin layout -- --board fire-24-e` says why.  An
unservable chip has `-` in every size column and the reason in the `blocked by`
column at the end of its row.  The three you will see:

- *"physical pin N has no GPIO mapping on this board"* — the chip is bigger than
  the socket and that pin cannot be reached.  Expected, not a fault — the chip is
  still listed for boards that can reach it.
- *"does not support chip type X with this firmware version"* — step 2 was
  missed, or the chip is deliberately excluded.
- *"select/control GPIOs [10, 11, 16] are not contiguous on this board; Multi
  and Banked sets require all select, commoned, and X-pin GPIOs to form a
  contiguous range within a single PIO window"* — the chip's enables land on
  GPIOs no existing chip-select algorithm can express.

Only the last needs firmware work, and it is a serving-algorithm change rather
than a chip-type change.  A new algorithm is:

- a variant and a `NUM_*_ALGS` bump in
  [`rust/metadata/metadata_schema.toml`](/rust/metadata/metadata_schema.toml),
  with its parameter fields and `ALG_*_PARAMS_LEN` constant — that generates
  both the C header and the Rust serialiser,
- a `case` in [`firmware/src/piodma/piorom2.c`](/firmware/src/piodma/piorom2.c)
  emitting the PIO program,
- a preference variant in
  [`rust/gen/src/v2/alg_preference.rs`](/rust/gen/src/v2/alg_preference.rs), plus
  the derivation that selects it in `alg_cs.rs`, `cs_data_layout.rs` or
  `addr_layout.rs`,
- a CS-to-data cycle cost in
  [`rust/fw-tester/src/cs_timing.rs`](/rust/fw-tester/src/cs_timing.rs), which
  asserts against the firmware's own `NUM_*_ALGS` and so will not compile until
  the new algorithm has one.

That is a much bigger piece of work than adding a chip type — and it benefits
every chip that can use the new algorithm, not just the one that prompted it.

## 6. Add it to the test sweep

A chip that serves today but is not tested will break quietly later.
[`ci/test-emu.sh`](/ci/test-emu.sh) lists its chips explicitly, one line each, so
a new type is covered only once you add it.  Put it with the others for its pin
count, with a test image of the right size from [`images/test`](/images/test):

```sh
run_test   $board images/test/rand_16KB.rom  type=23128  3
run_no_cs  $board images/test/rand_8KB.rom   type=2764
```

`run_test` takes the number of configurable chip select lines and sweeps **every
polarity combination** — 3 means all 8 are tested.  `run_no_cs` is for chips
whose enables have a polarity fixed by the silicon, so there is nothing to sweep.

Run just your pin count while iterating — `ci/test-emu.sh 24` — as the full sweep
takes hours, and only one can run in a given working tree at a time.

## Folding it back in

If you want your chip type in One ROM proper rather than kept in your own tree,
please do — that is how the supported list has grown, and a chip somebody has
actually got in front of them is worth more than one added speculatively.  Open
an issue or a pull request, and talk to the maintainer early if the chip needs a
new serving algorithm, since that is a change with consequences beyond your chip
and is better designed together than reviewed after the fact.
