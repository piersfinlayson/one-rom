# Multiple ROM Sets

One ROM 24 offers two models of run-time multi-ROM image support:

- **Dynamic bank switched image sets**.  In this mode, the ROM images in a set can dynamically switched at runtime (requiring no One ROM or host reset).  This is done by using the X1 and X2 pins as bank select lines.

- **Multiple simultaneous ROM image sets**.  A set is a group of up to 3 ROM images, all of which One ROM will serve **simultaneously**.  In this model, extra chip select lines from the extra ROM sockets to be served are connected to One ROM pins X1 and X2.  This allows a single One ROM to serve up to 3 ROM images.  (Restrictions apply, see below.)

Other versions of One ROM do currently not offer these features.

The examples below show how to configure these features building One ROM from source.  A similar approach is taken when using CLI/Studio.  Example the CLI help and the [JSON config file readme](../onerom-config/README.md) for details on how to configure these features using CLI/Studio. 

## Dynamic Bank Switching

Dynamic bank switching requires hardware revision F onwards, and firmware version v0.2.1 onwards.

Dynamic bank switching supports all ROM types (2364, 2332 and 2316) with any CS configuration.

In this mode, all ROMs in a particular set must share the same ROM type and CS configuration.the

## Multiple Simultaneous ROM Image Sets

Multiple simultaneous ROM image sets requires hardware revision E onwards, and firmware version v0.2.1 onwards.

For this feature, the following conditions must be met:

- All the ROMs being replaced share the same address and data buses.
- One ROM is installed in the socket of the first ROM in the set to be replaced.
- The other ROM sockets to be replaced are empty.
- The chip select line (pin 20) of the other, empty, ROM sockets to be replaced are connected to One ROM pins X1 (2nd image of set) and X2 (optional 3rd image of set).

This feature is currently available only for 2364 ROMs, and 2332/2316 ROMs whose CS2/CS3 lines are permanently tied to the active state.  This is true, for example, for the C64 character ROM.  You can mix ROM types, so long as this condition is met - so both C64 2364 kernal/basic ROM images can share a set with the C64 2332 character ROM image.

You can find pre-built configurations in [`config`](/onerom-config/README.md), named `set-*.mk`.

[MCU Selection](/docs/old/MCU-SELECTION.md) gives guidance on the STM32F4 clock speed requirement for serving Multi-ROM sets if using One ROM Ice.

## Set Configuration

Under the covers, One ROM uses the concepts of sets, even if they are not specified - in this case, each ROM image is in its own set.  Hence the existing configuration model remains supported.  That single image set configuration looks like:

```Makefile
ROM_CONFIGS = \
    file=rom_2364_1.bin,type=2364,cs1=0 \
    file=rom_2332_1.bin,type=2332,cs1=0,cs2=0
```

That is still a perfectly valid approach, if you do not want multi-ROM sets, or dynamic bank switching.

### Multi-ROM Set Configuration

To define multi-ROM sets, in order to serve multiple ROM images simultaneously, use a `ROM_CONFIGS` setting like this:

```Makefile
ROM_CONFIGS = \
    set=0,file=rom_2364_1.bin,type=2364,cs1=0 \
    set=0,file=rom_2332_1.bin,type=2332,cs1=0,cs2=ignore \
    set=0,file=rom_2364_2.bin,type=2364,cs1=0 \
    set=1,file=rom_2364_3.bin,type=2364,cs1=0
```

Each set can contain 1, 2 or 3 images, and are selected by the regular CS line, X1 and X2 respectively.  All CS1 values must be the same within a set - `sdrr-gen` attempts to detect and reject invalid configurations.

In the example above, the first set (select if the regular image select jumpers are all open), the ROM will serve

- `rom_2361_1.bin` to the socket it is installed in, if that socket's CS line goes active.
- `rom_2332_1.bin` to the same data bus, if the CS line from socket 2 is connected to X1, when this cs line goes active.
- `rom_2364_2.bin` to the same data bus, if the CS line from socket 3 is connected to X2, when this CS line goes active.

### Dynamic Bank Switching Configuration

To define dynamic bank switching, use a `ROM_CONFIGS` setting like this:

```Makefile
ROM_CONFIGS = \
    set=0,bank=0,file=rom_2364_1.bin,type=2364,cs1=0
    set=0,bank=1,file=rom_2364_2.bin,type=2364,cs1=0
    set=0,bank=2,file=rom_2364_3.bin,type=2364,cs1=0
    set=1,filename=rom2364_5.bin,type=2364,cs1=0
```

Here, when set 0 is selected, by all the image select jumpers being open, the ROM within that set that is served is selected by using X1 and X2 as image select jumpers.

- X1/X2 open - `rom_2364_1.bin` is served
- X1 closed/X2 open - `rom_2364_2.bin` is served
- X1 open/X2 closed - `rom_2364_3.bin` is served
- X1/X2 closed - `rom_2364_1.bin` is served

The last line takes some explaining.  As only 3 banks are configured, images 0-2 exist.  However, bank 3 has been selected by closing both X1 and X2.  One ROM takes the bank select **modulo** the number of images (i.e. wraps and starts counting again), so reverts to image 0.

## Future Enhancements

### Multi-ROM sets: 2332s With Varying CS2

It should be possible to support a set of 2332 ROMs with CS2 varying, so long as all of the chips being replaced share the same CS2 line (i.e. are connected together).  The serving algorithm to implement this may be less efficient again than the stock mutli-rom set serving algorithm, but it is likely to be required by less powerful machines, so this is unlikely to be an issue.

It may be possible support multiple 2316 ROMs with varying CS2/CS3, so long as all CS2s are tied together and all CS3s are tied together.

It may also be possible to support combined 2332/2316 sets, so long as CS2 on the 2316 is permanantly active (and CS2 for all the 2332 and CS3 for the 2316s are all tied together).  (Somewhat confusingly, 2332 CS2 and 2316 CS3 are the same pin - 2316 CS2 is distinct.)

## Technical Details

In order to implemented this feature, all ROM images in the set are combined into a single 64KB image before storing into the firmware, and then the entirety of the STM32 port C GPIO state is read as an offset into that image. The ROM images are placed in the correct place for this lookup to work by the pre-processing step, using `sdrr-gen`.

### Multi-ROM Set Serving Details

If only 2 ROM images are in the set, the final bit of port C (PC15) is expected to be unconnected, and pulled low internally, just as when serving a single ROM image, PC14/PC15 are both pulled low, to ensure a 16KB offset into a 16KB image.  A 64Kb image is still used in the 2 ROM images in a set case, although a future enhancement could reduce the space required to 32KB.

A different ROM serving algorithm is used in the rom set case, to serve addresses only when a chip select line is active, and to detect if _any_ chip select line goes active.  The standard algorithm:

- is constantly reading the address lines and looking up the data byte even when CS isn't selected - this isn't possible here as the CS line going active determines which byte to read
- detects when _all_ chip selects (for that ROM type) go active, and only sets the data lines to output when this happens.

It is this different algorithm that is roughly 10% less efficient than the single ROM image serving algorithm.  The pre-loading of 64KB at startup instead of 16KB probably also adds around 200-300us at boot.  Given systems typically have a decent reset circuit, this is not expected to be an issue.

See [Multi-ROM Support](/docs/old/TECHNICAL-SUMMARY.md#multi-rom-support) for more details on the performance on this feature.

### Dynamic Bank Switching Serving Details

In this case, again, 64Kb is used for the total ROM image, and X1/X2 act as an index into that.  The total ROM image is created differently than in the multi-ROM set case, as the two cases vary in which byte should be returned given the precise CS/X1/X2 states.

## Acknowledgements

Original suggestion of Multi-ROM sets was made by [Adrian Black](https://www.youtube.com/@adriansdigitalbasement).
