# Multiple ROM Slots

One ROM offers two ways for a single One ROM to serve more than one ROM image
simultaneously:

- **Multiple simultaneous ROM image slots**.  A slot is a group of up to 3 ROM
  images, all of which One ROM serves **at the same time**.  The chip select
  lines of the other, empty, ROM sockets are connected to One ROM's X1 and X2
  pins, so one device stands in for up to three ROMs on the same bus.

- **Dynamic bank switched image slots**.  A slot is a group of up to 4 ROM images,
  one of which is served at a time.  X1 and X2 select which, and the selection
  is read continuously, so an image can be changed **while the host is running**
  with no reset of either the host or One ROM.

A given slot uses one or the other, never both.

This article covers One ROM Fire.

Slots were previously called sets, and the older name survives in the
configuration file format (`chip_sets`, or `rom_sets` in older files), in the
names of the ready-made configurations (`set-*.json`), and in the messages the
generator prints.

Selecting between different single ROM images at boot time, using the image
select jumpers (the normal case), is a separate mechanism described in
[Image Selection](/docs/IMAGE-SELECTION.md).  The two can be combined - choosing
the slot at boot time and then using X1/X2 within the chosen slot.

## Hardware Requirements

Both features need the X1 and X2 pins, which not every board carries.  Multi-ROM
slots ask for more than that - see [Why a slot may be
refused](#why-a-slot-may-be-refused).

| Board | Bank switching | Multi-ROM slots |
| --- | --- | --- |
| fire-24-a, fire-24-b | yes | no |
| fire-24-c, fire-24-d, fire-24-e, fire-24-f, fire-24-g | yes | yes |
| fire-28-a, fire-28-b | no | no |
| fire-28-c, fire-28-d | yes | yes |
| fire-32-a, fire-32-b, fire-32-c | no | no |
| fire-40-a, fire-40-b, fire-40-c | no | no |

## Dynamic Bank Switching

A banked slot holds 2, 3 or 4 images of the same chip type.  X1 and X2 are read
as a bank number, and the image with that number is the one served:

- 2 images: X1 alone chooses, and X2 is unused.
- 3 or 4 images: X1 is the low bit and X2 the high bit of a 2 bit bank number.

For a 3 image slot, bank 3 - both X1 and X2 closed - names an image that does not
exist.  One ROM serves 0xAA throughout for that bank rather than wrapping round
to the first image.

All images in a banked slot are the same chip type, and their chip select
polarities must agree, because every image is reached through the same physical
chip select pin on the board.

### Dynamic Bank Switching Configuration

```json
{
    "version": 1,
    "name": "C64 character sets",
    "chip_sets": [
        {
            "type": "banked",
            "description": "Switchable character sets",
            "chips": [
                { "file": "characters.901225-01.bin", "type": "2332",
                  "cs1": "active_low", "cs2": "active_high" },
                { "file": "characters.325018-02.bin", "type": "2332",
                  "cs1": "active_low", "cs2": "active_high" }
            ]
        }
    ]
}
```

Ready-made banked configurations are in [`onerom-config`](/onerom-config/), named
`bank-*.json`.

## Multiple Simultaneous ROM Image Slots

A multi-ROM slot holds 2 or 3 images, and One ROM serves all of them at once.
The following must be true:

- All the ROMs being replaced share the same address and data buses.
- One ROM is installed in the socket of the first ROM in the slot.
- The other ROM sockets in the slot are empty.
- A chip select line of each other socket is connected by a flying lead to One
  ROM's X1 pin (the second image) and X2 pin (the third).

Which chip types can go in a slot is not a fixed list.  The rule is that each of
the other chips reaches One ROM through exactly one control line - the one on the
flying lead - and every other control line it has is either commoned across the
whole slot or is not used to choose between chips.  So a slot can mix chip types,
including chips of different sizes, as long as they agree on that.

Two worked examples.  A C64 kernal (2364) can share a slot with the character ROM
(2332), because the character ROM's CS2 is tied permanently active and CS1 does
the choosing.  A pair of 2732 EPROMs can share a slot with /CE on the flying lead
and /OE commoned between them.

A chip in the slot may have fewer address lines than the chip One ROM is
installed as - a 2716 behind a 2732, or a 2332 behind a 2364.  The address lines
the smaller chip does not have are simply not connected to it, and One ROM
serves it the same byte whatever those lines are doing.

The polarities of the choosing line do not have to match across the slot.

### Multi-ROM Slot Configuration

The first chip is the one One ROM is installed as.  Each later chip names the
control line on its flying lead by leaving that line alone, and marks its other
control lines `ignore`:

```json
{
    "version": 1,
    "name": "Bally -35 board, U2 and U6",
    "chip_sets": [
        {
            "type": "multi",
            "description": "U2 in the socket, U6 on a flying lead",
            "chips": [
                { "description": "U2", "file": "ballyu2.732", "type": "2732" },
                { "description": "U6", "file": "ballyu6.732", "type": "2732",
                  "oe": "ignore" }
            ]
        }
    ]
}
```

Here both chips are chosen by /CE, so U6's /CE goes to X1, and /OE is commoned
between the two sockets.  Marking U6's `oe` as `ignore` is what says "/CE is the
line on the flying lead".

Ready-made multi-ROM configurations are in [`onerom-config`](/onerom-config/),
named `set-*.json`.

Configuration files also accept `rom_sets` and `roms` in place of `chip_sets`
and `chips`, and the files shipped in `onerom-config` use those older names.

## Why a Slot May Be Refused

One ROM reads the address lines, the chip selects and the X pins as one
contiguous run of MCU pins, in a single hardware operation.  For a multi-ROM
slot, the line that chooses between chips and the X pins are all part of that
read, so they have to sit next to each other on the MCU.  Which MCU pin each
socket pin reaches is fixed by the PCB routing, and on some boards those pins
are not neighbours.

Where that happens the build stops and says so, rather than producing a
firmware image that does not work:

```
The board fire-24-a does not support this configuration: 2364 select/control
GPIOs [9, 13] are not contiguous on this board; Multi and Banked sets require
all select, commoned, and X-pin GPIOs to form a contiguous range within a
single PIO window
```

This is why fire-24-a and fire-24-b serve banked slots but not multi-ROM slots.
Bank switching does not have the requirement, because there the X pins are read
as part of the address rather than as extra chip selects.

## Technical Details

One ROM's address PIO reads a single contiguous window of MCU pins and uses the
value it reads as an index into a table of bytes held in RAM.  That window
covers more than the address lines: the chip selects and the X pins are inside
it too.  Everything each feature does falls out of how that table is filled in,
which is the generator's job.

For a **banked** slot, the X pins are two more index bits.  The table holds each
image at the offset its bank number gives, so changing a jumper changes which
part of the table the next read lands in.  This is why a bank change takes
effect immediately and needs no reset - nothing is reloaded, the reads simply
go elsewhere.

For a **multi-ROM** slot, the chosen chip's select line is the index bit.  The
first chip's data sits where its own chip select bit is active, the second's
where X1 is active, the third's where X2 is active.  Exactly one of those bits
is active during a real read, and the table entries for the combinations
hardware cannot produce are filled with a pad byte.

Both cost more RAM than a single image, because the table has to cover the whole
window rather than one image's worth of addresses.  The generator reports the
size it has produced, and refuses a slot that will not fit.

## Acknowledgements

Original suggestion of Multi-ROM slots was made by [Adrian Black](https://www.youtube.com/@adriansdigitalbasement).
