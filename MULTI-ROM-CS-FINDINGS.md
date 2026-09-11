# Multi-ROM chip select: what we found, and three designs that did not work

This file exists only on the `multi-commoned-cs-gate` branch and is not meant to
reach `main`.  It is a record of one piece of work, dated by where it lives.  It
is deliberately not in `docs/`, which is for things that stay true and get
maintained.

The commit it sits in carries the rejected code.  The commit before it carries
the two changes that survived.

## Where it started

A user reported an Apple II configuration that did not work: three 2316 images
in one multi-ROM slot, One ROM in the F8 socket with F0 on X1 and E8 on X2.

```json
{ "description": "F8", "file": "f8.bin", "type": "2316",
  "cs1": "active_low", "cs2": "active_high", "cs3": "active_low" }
```

The configuration is legal, builds cleanly, and describes the Apple II's ROM
socket correctly - pin 20 is the decoder's select, pin 18 is /INH and pin 21 is
tied to pin 20.  It passed `pio-tester` on every board that would build it.

It still drove the data bus permanently.

## F1.  A multi slot's chip select gate is an OR, and a commoned line fires it alone

The serving state machine reads one contiguous window of GPIOs and drives the
data bus when **any** bit in it is high, releasing only when all are low
(`firmware/src/piodma/piorom2.c:556-625`, with `serve_cs_low_0 == 1`).

The window holds more than the chip selects.  A **commoned** line - one shared
across every socket in the slot, asserted for any read of any chip - sits in it
too.  On the Apple II that line is /INH, which is pulled up and therefore
asserted essentially always.  Its bit is permanently high, the gate is
permanently true, and One ROM never lets go of the bus.

Measured, driving the Apple II's real levels at an emulated fire-24-c:

```
idle, nothing selected        bus_driven=YES
```

The configuration that works marks `cs2` and `cs3` as `ignore` on chip 0, with
`allow_cs_ignore: true`.  That takes both out of the window - the generator
refuses an ignored line interior to the span, which is why both must go - and
leaves the gate as the three socket selects alone.  It costs /INH support, so it
needs a machine with no card asserting /INH over $E800-$FFFF.

## F2.  The test harness was built so that F1 could not be caught

`pio-tester` does check that the data bus is released when a chip is not
selected.  It enumerates every non-selected combination of the control lines and
asserts the bus stays tristated.

It ran that check with every commoned line held **deasserted**, and said why:

> an asserted commoned line fires the gate alone

So the failing state was known, named in a comment, and the other one driven.
The harness also held chip 0's commoned lines deasserted in the background of
every secondary chip's pass.  No `pio-tester` run could reach the state that
fails.

The fix is in the commit before this one.  With it, 16 of the 23 multi sets in
the test configs fail - every set that has a commoned line.  The seven that pass
have a single-control-line primary (2364, 231024), so they have no commoned line
to assert.  Single and banked sets are unaffected, because their gate is an AND.

## F3.  A general emitter in firmware costs 2.5KB

The first design gave the firmware a **field descriptor**: a byte array in a
small language of operations and widths, which the firmware walked to build the
PIO program.  It worked.  It cost **3164 bytes** of flash, taking the firmware
from 40016 to 43180 and the free space in the 48KB reservation from about 9.0KB
to 5.8KB.

Where it went:

```
+1006  piorom2 (3452 -> 4458)
 +786  cs3_emit_fields
 +602  cs3_emit_pass
 +320  pio_setup_address_monitor
 +126  cs3_resolve_end
```

For scale, `piorom2` at 40016 bytes is 3452 bytes **in total**, and that emits
all of ALG_CS_0, ALG_CS_1, ALG_CS_2, the address state machine and the data
state machine.  One more algorithm cost 2520 in and around it.

The difference is in kind.  The existing algorithms emit fixed instruction
sequences.  The descriptor's emitter built a program from data - validate, walk,
recurse to fork each alternative with its own copy of what follows, resolve jump
addresses, twice over for the acquire and hold passes.  That is a small
assembler, and `-O3` inlines it hard.  `-Os` on the two largest functions
recovers 336 bytes, which is not the problem.

## F4.  The fork is not optional, and it is reachable today

The expensive part is the fork: a select field that has another select field
after it, so the first to match must exit early, and the shift position then
depends on which branch won.  Deleting it looked like the saving.

It is reachable from a config in the test suite.  `24-multi-2316.json` sets 3
and 4 put chip 0's own select on socket pin 18 with pin 20 commoned, and on a
fire-24-c pin 20's GPIO sits between X1's and pin 18's.  The three selects
cannot coalesce:

```
ANY 2 (X2, X1)   COMMON 1 (pin 20)   ANY 1 (pin 18)
```

Chip 0's select is whichever control line the secondaries leave alone, and
nothing makes it the lowest one.

## F5.  The plugin API sees chip 0 of a multi slot and nothing else

The CS monitor's input window is narrowed to chip 0's own select before the
state machine is configured (`firmware/src/piodma/pioplugin.c:321-324`).  An
access to the chip on X1 or X2 raises no interrupt, so it is **missed** - not
captured and misattributed.  A plugin cannot tell which chip an access was to,
and never sees two of the three.

## F6.  X pin identification is dead on multi slots, in shipped firmware

`v2_get_x_pin_gpios` finds the X pins by skipping everything in the CS range
(`pioplugin.c:146-147`).  On a multi slot the X pins are **inside** that range,
so both come back `GPIO_NONE`.  The X-inactive check in
`pio_demangle_observed_addr` never rejects anything, and `ora_knock_t.x_mask` is
always zero - the filter its own comment calls "the only thing needed here".
Banked slots are unaffected, because there the X pins really are outside the CS
range.

This is in v0.7.2 and is independent of everything else here.

## F7.  A monitor that re-arms on "everything deselected" is a bet on host timing

The CS monitor re-arms by seeing the select go inactive, after three agreeing
samples of a three instruction loop - nine PIO cycles, 60ns at 150MHz.  The
comment at `pioplugin.c:176-178` says that is well inside the gap between two
accesses.

Widening the monitor to watch all three selects would make that bet larger: it
would have to hold across a handoff from one chip in the slot to the next, not
just the gap between two accesses to one chip.

No measurement settles it.  Systems deselect in different ways - some gate the
decode with the clock, some with a bus strobe, some combinatorially on address
alone - and One ROM has to work on every machine that used a mask ROM, not on
the ones we can put a scope on.

What is true at a handoff regardless of timing is that **the selected chip
changed**.  A monitor that holds the pattern it armed on and re-arms when the
pattern differs does not make the bet.  That costs a register and instructions,
in a block whose tightest budget is ALG_CS_1's 20 instruction monitor.

One floor no design removes: two back-to-back accesses to the **same** chip with
no deassertion between them cannot be told from one long access, if all you
watch is the selects.

## The three designs, and why each was rejected

**D1.  Field descriptor.**  The firmware decodes a small language into a PIO
program.  Rejected on size - F3 - and on the observation that it is a language
whose only consumer decodes it straight back into what the host already had.

**D2.  Raw PIO instructions, shaped around this algorithm.**  The host writes
the program and the firmware loads it.  Rejected because the first cut kept the
old design's assumptions in the metadata - a field for the all-ones constant
this algorithm's comparisons happen to need.  That is not a raw-PIO algorithm,
it is D1 wearing PIO.

**D3.  Raw PIO instructions, generic.**  The algorithm says nothing about chip
selects.  What distinguishes it is who wrote the program.  Not rejected - this
is where the thread ended up, and it is the starting point for whatever comes
next.  What stopped it landing here is F5: the plugin API needs to describe
multi-ROM slots, the firmware cannot recover that from an opaque program, and
the metadata is a shipped contract, so that description has to be right before
anything ships rather than added later.

The shape under discussion when the thread stopped: the PIO program carried as
instructions, **and alongside it** a description of the configuration the slot
was generated from - per chip, which bits of the captured address word are that
chip's select and what value means selected.  That description is the
generator's own input, not an invented intermediate, so anything servable is
describable by construction.  `onerom_rom_info_t` already carries much of it.

## What is in this commit, and what it does not do

It **does not build**.  It is two generations of rejected work on top of each
other: the D1 firmware emitter against the D1 schema, plus the D2 schema
reshape that replaced it.  They are mutually incompatible.  The D1 state alone
did build, and is where the 43180 byte figure came from.

Worth reading if you pick this up:

- `firmware/src/piodma/piorom2.c` - the descriptor emitter, its fork emission
  and its direct form.  Hand-verified against disassembly, never run.
- `rust/gen/src/v2/cs3_fields.rs` - deriving a descriptor from a CS window,
  with tests naming the real configurations.
- `rust/metadata/metadata_schema.toml` - the D2 raw-PIO variant.
- `rust/metadata/build/*.rs` - a `trailing_array` field kind, including u16
  elements.  Nothing uses it once ALG_CS_3 goes.
- `rust/metadata/build/host_gen.rs` - the C generator writing 0xFF rather than
  0x00 for reserved bytes, matching what the binary serializer leaves.  That one
  is a real fix and belongs somewhere.

## Open

- F6 wants its own issue.  It is a shipped defect with nothing to do with this.
- F7 is a design constraint on any monitor that serves more than chip 0.
- `GROUP` - a chip whose select is more than one line - cannot be produced by
  any config, because `rust/gen/src/builder.rs:1327` requires a multi set's
  secondaries to name exactly one active control line.  Lifting that is what
  issue #295 needs, and it is what makes two 27xxx sharing a slot expressible.
