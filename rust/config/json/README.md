# rust/config/json

This directory contains the One ROM PCB hardware configuration files, which tell the software what the port/pin mappings for the various pin types are.

This allows new hardware revisions (i.e. new PCB layouts with different pin mappings) to be supported without needing to modify the source code.

It also contains the [supported chip type hardware configuration file](chip-types.json), which tells One ROM what chips to support and how to support them.

## Usage

See the `*.json` files in this directory for the format of the configuration files.  There are some restrictions on which pins can be used for which pin types:

- All pins of a particular type (address, data, CS/CE/OE in particular) must be on the same port.
- For now, CS/CE/OE pins must share the same port as the address pins, to allow a single STM32F4 port to be read for both address and chip select status.

You should store your files either in:

- [`user/`](/rust/config/json/user/) - for your own, private, hardware configurations
- [`third-party/`](/rust/config/json/third-party/) - for hardware configurations that you plan to submit pull requests for, and want to share with the community

The `onerom-config` crate is generated and built automatically from these config files, and published to crates.io.  This is then used by tools like `sdrr-gen` to generated the One ROM firmware.

## Jumper header (`jumper_header`)

An **optional** top-level object describing the physical image-select jumper /
programming header at the top edge of the board, so host tools (e.g. the web ROM
Slot Builder) can draw an accurate diagram of which pads to jumper.  Omit it and
tools fall back to a generic description; `fire-24-f.json` is the worked example.

The header is described **column by column** — a column is a physical pin pair,
numbered from `1` at the board's **left edge**.  Within a column, row `1` is the
**top** pad and row `2` the **bottom** pad (both required); row `3` is an
**optional** extra pad below the column (the X pins).  Each pad lists up to **two**
role tokens (for pins where a signal is multiplexed):

```json
"jumper_header": {
    "columns": {
        "1": { "1": ["5v"],      "2": ["gnd"] },
        "2": { "1": ["run"],     "2": ["sel_d", "swdio"] },
        "3": { "1": ["bootsel"], "2": ["sel_c", "swclk"] },
        "4": { "1": ["sel_b"],   "2": ["gnd"], "3": ["x2"] },
        "5": { "1": ["sel_a"],   "2": ["gnd"], "3": ["x1"] }
    }
}
```

Role tokens: `5v`, `gnd`, `run`, `bootsel`, `sel_a`..`sel_e` (the letter is the
image-select **bit weight**, `a` = bit 0 = LSB), `swclk`, `swdio`, `x1`, `x2`, and
`a<N>` (a high address line broken out on the header, e.g. `a17` = A17).  Two
reserved single-token pad states: `["nc"]` (pad present but **not connected**) and
`["np"]` (pad **not populated**).  A whole **absent column** (e.g. a revision
missing its left-most 5V/GND pair) is simply **omitted** — present columns keep
their absolute number, so a board whose columns start at `2` draws shifted right.

Row `3` (the extra pad below a column) carries only the "extra config" roles: an X
pin (`x1`/`x2`) or an address line (`a<N>`).  On 24/28-pin boards it is the X pins;
on 32-pin boards the same physical pads instead break out high address lines.

The descriptor is **cross-checked against the MCU pin data at build time**, so the
physical and electrical descriptions cannot drift (a mismatch fails the build with
a specific message):

- the `sel_*` bits used must be exactly `0..len(mcu.pins.sel)`, each once;
- when the board multiplexes SWD onto an image-select pin (`mcu.pins.swclk_sel` /
  `swdio_sel` set), the `swclk`/`swdio` role must sit on the `sel_*` pad whose GPIO
  is that pin.  On boards that don't (2-select boards whose SWD pins share the
  header but aren't image selects), a standalone `swclk`/`swdio` role on a
  non-select pad is fine;
- `x1`/`x2` roles must be present exactly when the board defines `x1`/`x2` pins,
  and must sit on row `3`;
- an `a<N>` address line must exist on the board (`N < len(mcu.pins.addr)`).
