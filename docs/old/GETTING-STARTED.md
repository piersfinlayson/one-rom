# Getting Started

## Contents

- [Important](#important)
- [USB One ROMs](#usb-one-roms)
- [Hardware Identification](#hardware-identification)
- [Programming One ROM via SWD (not USB)](#programming-one-rom-via-swd-not-usb)
- [Image Selection](#image-selection)
- [Bank Selection](#bank-selection)
- [Multi-ROM Sets](#multi-rom-sets)
- [Hardware Version Build Settings](#hardware-version-build-settings)
- [Recovering a Bricked Device](#recovering-a-bricked-device)

## Important

⚠️ **Do Not** externally power your One ROM device when it is installed in your retro system, other than via USB.

If the retro system is off or unplugged, this will cause your power supply and One ROM to attempt to power the entire 5V rail of your retro system, which may damage your One ROM, your retro system, your power supply or all of them.

You may program your One ROM when it is installed in your retro system, so long as it is powered on.  In this case **do not** provide external power.

⚠️ **Always** install One ROM in the correct orientation.  Pin 1 is clearly marked with a white L (corner) shape on the PCB.  The USB connector is always **at the other end** of the PCB from pin 1.

## USB One ROMs

Most people will be getting started with a USB One ROM device, which can be programmed over USB.  You are recommended to use either:

- [One ROM Studio](https://onerom.org/studio) - a desktop application for Windows, macOS and Linux
- [One ROM Web](https://onerom.org/web) - a web tool for programming One ROM from your browser.

See those links for more information on how to use those tools.

## Hardware Identification

Each variant of the One ROM hardware has a unique revision ID printed on the PCB, and the boards have different numbers of breakout pins at the top of the PCB.

Identify your One ROM hardware version using the following image - in particular look for the revision ID, or match with the number of pins at the top of the PCB.

Fire boards use the RP2350 microcontroller, Ice boards STM32.

![Identify your One ROM Hardware Version](/docs//images/id-g.png)

Not all later revisions are shown here - they are very similar to those shown, but with a different silkscreen marking.

## Programming One ROM via SWD (not USB)

You need an SWD programmer to program the One ROM hardware.  The Raspberry Pi Debug Probe and a Raspberry Pi Pico programmed as a Debug Probe are suitable programmers.  An ST-Link or any other SWD programmer should also work.

You need to connect 3 cables from the SWD programmer to the One ROM:
- SWDIO (data)
- SWCLK (clock)
- GND (ground)

For the programming pins locations see:
- [STM32 Revisions 24-D/E/F Programming Pins](#stm32-revisions-24-def-programming-pins)
- [STM32 Rev G and RP2350 Rev A Programming Pins](#stm32-rev-g-and-rp2350-rev-a-programming-pins)

You also need to power the One ROM in order to program it.  This is most easily done by installing it in a retro system, and powering it on.

Alternatively, you can power the One ROM from a 5V power supply by connecting to the 5V and GND [pins that usually connect the One ROM to the retro system](#external-power):
- GND to pin 12 (bottom left)
- 5V to pin 24 (top right)

At present, when using the standard One ROM programming utility `probe-rs`, the RP2350 version must also be manually put into DFU/bootloader mode to be programmed, by shorting BOOT to GND when One ROM is powered on.  See the [Recovering a Bricked Device](#recovering-a-bricked-device) section for details.

Once connected, you can run the appropriate `make` command from the repository root to build and flash the firmware.

### Example `make` Commands

One ROM rev G, STM32F405RG MCU, serving the standard set of C64 images:

```bash
HW_REV=24-g MCU=f405rg CONFIG=old-config/c64.mk run
```

One ROM rev A, RP2350 MCU, serving the standard set of VIC20 PAL images:

```bash
HW_REV=p24-a MCU=rp2350 CONFIG=old-config/vic20-pal.mk run
```

There are lots of other build configurations possible - see:
- [Hardware Version Build Settings](#hardware-version-build-settings) for standard `HW_REV` and `MCU` settings
- [config](/old-config/README.md) for a list of standard ROM image configurations
- [Advanced Configuration](/docs/CONFIGURATION.md) for other configuration options.

### Ice Revisions 24-D/E/F Programming Pins

<img src="./images/prog-d-f.png" alt="Programming Pins for Revisions STM revs D/E/F" width="50%">

### Fire rev A and Ice Rev G Programming Pins

<img src="./images/prog-g-rp.png" alt="Programming Pins for Revisions STM rev G and RP rev A" width="50%">

### Fire rev C onwards and Ice Rev I Onwards Programming Pins

These are at the top of the board and are labelled on the underside of the board.

### External Power

<img src="./images/power.png" alt="External Power Pins" width="50%">

## Image Selection

One ROM supports a number of different ROM images being installed at once, with those images being selected by using the One ROM jumpers.  One ROM reads these jumpers at boot time to detect which image to serve.  Using this mechanism, One ROM will not dynamically switch between images while the retro system is running - see [Bank Selection](#bank-selection) below for dynamic switching.

The image select jumpers are always found at the top of the One ROM PCB, and differerent hardware revisions have different numbers of supported jumpers.

The images below show which jumper indicates which bit of the image selection value.  If closed, the bit is a 1, if open, the bit is a 0.  Therefore to select:
- image 0, leave all jumpers open
- image 1, close the jumper marked 0
- image 2, close the jumper marked 1
- image 3, close the jumpers marked 1 and 0
- etc.

If you select an image number higher than the total number of images installed, One ROM will start counting again from image 0.  For example:
- if you have 1 image installed, it will always be selected
- if you have 2 images installed, only bit 0 will take effect
- if you have 3 images installed and close jumper bits 1 and 0, image 0 will be selected (3 modulo 3 = 0)

### Fire Image Selection Jumpers

Fire 24 revision A has 3 image select jumpers, with the least significant bit on the **right**.  All subsequent Fire boards have the least significant bit on the right also, with the following number of image select jumpers:

<img src="./images/sel-rp-a.png" alt="Image Selection Jumpers for RP rev A" width="50%">

24 pin:

- A - 3
- B - 3
- C - 2
- D - 4

28 pin:

- A - 2

### Ice Revision 24-D Image Selection Jumpers

Revision D has 3 image select jumpers, with the least significant bit on the left.

<img src="./images/sel-d.png" alt="Image Selection Jumpers for STM rev D" width="50%">

### Ice 24-E/F Image Selection Jumpers

Revisions E and F have 4 image select jumpers, with the least significant bit on the left.

<img src="./images/sel-ef.png" alt="Image Selection Jumpers for STM revs E/F" width="50%">

### Ice 24-G/H Image Selection Jumpers

Revision G has 5 image select jumpers, with the least significant bit on the **right**.

<img src="./images/sel-g.png" alt="Image Selection Jumpers for STM revs G/H" width="50%">

## Ice 24-I/J Image Selection Jumpers

These revisions have 4 image select jumpers, with the least significant bit on the **right**.

## Bank Selection

When using [bank switched configurations](/docs/MULTI-ROM-SETS.md#dynamic-bank-switching-configuration) (those that start `bank-`), One ROM dynamically switches between images using the bank select jumpers ("X pins" or "expansion pins") X1 and X2.  These are always found at the top of the PCB.

<img src="./images/x1-x2.png" alt="Bank Selection Jumpers" width="50%">

Note that One ROM 28 does not have X pins. 

See the [Multi-ROM Sets](/docs/MULTI-ROM-SETS.md) documentation for more information on banks and multi-ROM sets.

## Multi-ROM Sets

When using [multi-ROM slots](/docs/MULTI-ROM-SETS.md#multi-rom-slot-configuration) (those that start `set-`), One ROM serves ROM images up to 3 ROM sockets simultaneously, using pins X1 and X2 as extra chip select lines.  Connect flying leads from the X1 and X2 pins to the chip select pins of the other ROM sockets to be served.

<img src="./images/x1-x2.png" alt="Bank Selection Jumpers" width="50%">

Note that One ROM 28 does not have X pins. 

See the [Multi-ROM Sets](/docs/MULTI-ROM-SETS.md) documentation for more information on banks and multi-ROM sets.

## Hardware Version Build Settings

When building the One ROM firmware you must identify the hardware version and the MCU type to the build system.  The following table lists the identifiers to use for each hardware version:

| Type | Pins | PCB Revision ID  | `HW_REV=` | Supported `MCU=` | `STATUS_LED=` |
|------|-----------|-----|-----------|------------------|---------------|
| Fire | 24 | A | fire-24-a | rp2350 | 0/1 |
| Fire | 24 | B | fire-24-b | rp2350 | 0/1 |
| Fire | 24 | C | fire-24-c | rp2350 | 0/1 |
| Fire | 24 | D | fire-24-d | rp2350 | 0/1 |
| Fire | 28 | A/A2/A3 | fire-28-a | rp2350 | 0/1 |
| Ice | 24 | D | ice-24-d | f401rb/f401rc/f401re/f405rg/f411rc/f411re/f446rc/f446re | Not supported
| Ice | 24 | E | ice-24-e | f401rb/f401rc/f401re/f405rg/f411rc/f411re/f446rc/f446re | 0/1 |
| Ice | 24 | F/F2 | ice-24-f | f401rb/f401rc/f401re/f405rg/f411rc/f411re/f446rc/f446re | 0/1 |
| Ice | 24 | G | ice-24-g | f401rb/f401rc/f401re/f405rg/f411rc/f411re/f446rc/f446re | 0/1 |
| Ice | 24 | H | ice-24-h | f401rb/f401rc/f401re/f405rg/f411rc/f411re/f446rc/f446re | 0/1 |
| Ice | 24 | I | ice-24-i | f401rb/f401rc/f401re/f405rg/f411rc/f411re/f446rc/f446re | 0/1 |
| Ice | 24 | J | ice-24-j | f401rb/f401rc/f401re/f405rg/f411rc/f411re/f446rc/f446re | 0/1 |

Note that Fire boards can be fabbed with an RP2354 instead of the RP2350.  In this case the external flash should be un-populated.

## Recovering a Bricked Device

If your programmer will not connect to your One ROM device, or you have accidentally flashed incorrect firmware, you may have bricked your device and need to recover the device by forcing it into DFU/bootloader mode and then programming it using your SWD programmer as normal.

To enter DFU/bootloader mode, you must set the BOOT (Fire) or BOOT0 (Ice) pin to the correct level, and then reset the device, either by power cycling, replugging into USB or briefly pulling the RESET/RUN pin low.

## Fire

On the RP2350 BOOT must be pulled to GND, while resetting the device, to enter DFU/bootloader mode.  These should be shorted together to pull BOOT low.

Fire 24 rev A:

<img src="./images/boot-rp.png" alt="BOOT Pin Location - RP rev A" width="50%">

On Fire 24 B, BOOT and GND are exposed as pads on the board underside.

On Fire 24 C onwards and 28 A onwards, BOOT and GND are exposed at the top of the board and are labelled on the underside.

### Ice

On revisions D, E, and F, BOOT0 and 3.3V are exposed at the top right of the board as shown - short the two indicated pins together to pull BOOT0 high.

<img src="./images/boot0-d-f.png" alt="BOOT0 Pin Location - STM revs D/E/F" width="50%">

On revision G and H, BOOT0 and 3.3V are exposed at the bottom of the board as part of the programming pins, as shown.  Short the two indicated pins together to pull BOOT0 high.

<img src="./images/boot0-g.png" alt="BOOT0 Pin Location - STM rev G" width="50%">

On revisions I/J BOOT0 and 3.3V are exposed at the top of the board, and are labelled on the underside.

