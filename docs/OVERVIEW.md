# One ROM Overview

## Background - ROM Replacements

ROM (Read-Only Memory) chips are used in a huge assortment of electronic
devices to supply those systems with pre-programmed data - Operating System
and BIOS images, character sets, programming languages, games, etc.  On old
systems these sometimes fail and need replacing, or users would like to
upgrade their system, by replacing with a ROM with newer data.  The original
ROM chip is removed from the system and a replacement is installed in its place.

Traditional ROM replacements are built around EPROMs, EEPROMs and flash chips.
Those are similar devices to the ROM being replaced - that is dumb devices
that have a persistent memory store and some control lines - but with higher capacity
than the original ROM and/or different chip select behaviour.  The chip
select logic is typically fixed with additional on-board logic.

## Introduction to One ROM

One ROM takes a different approach - it consists of a microcontroller paired
with flash and RAM, physically laid out on a PCB so that the original ROM's
address, data and chip select lines connect to the MCU's general-purpose
input/output (GPIO) pins.  This makes One ROM a ROM emulator, also known as a
Software Defined ROM.

Firmware is loaded to One ROM's flash, along with the ROM image(s) to be
served.  One ROM is installed in the system.  On power on, it boots and first
loads and runs its firmware, and then loads the ROM image(s) to be served into
RAM, and serves them from there.

Because One ROM is microcontroller based, it is controlled by software rather
than by hardwired, hardware-based logic.  This makes One ROM the most powerful
and flexible ROM replacement available.   Nearly any original ROM of a
particular form factor can be replaced by an equivalent One ROM.

One ROM's software is open source and has a modular architecture with a rich
plugin API.  This means One ROM is a platform that can be extended with new
capabilities, not just a ROM replacement.  Some examples of ways in which
people have extended One ROM include:

- A comprehensive USB stack and interface to allow One ROM to be managed
  while running, built on the public plugin API.
- Communication between a retro system and a PC using One ROM's USB port.
- A retro system controlling its own reset line using one of One ROM's header
  pins.
- Reprogramming One ROM dynamically as part of a car's ECU tuning process.

## Hardware

One ROM comes in multiple physical variants to support each supported ROM package
size - 24, 28, 32 and 40 pin.  All One ROMs replace original 600 mil (15.24mm)
wide DIP packages.  The supported ROM types are listed in
[COMPATIBILITY](/docs/COMPATIBILITY.md).  Each pin variant has multiple hardware
revisions, marked on the board.

One ROM Fire, based on the RP2350 microcontroller, is the current family and is
available in all four sizes.

One ROM Ice is a deprecated 24 pin only family based on an STM32F4.  It is
supported on Base Firmware v0.6.x.  New feature releases for One ROM Ice are not
anticipated.

## Important concepts

- **Base Firmware** - a binary including the core code that causes a One ROM to
  serve ROM images.  Does not include any information about the One ROM hardware
  variant or the ROM images to serve.  Should not be used on its own to program
  a One ROM device.

- **ROM image** - a binary blob that is the contents of a ROM to be served.  Can
  be supplied to the programming tool as a raw binary file, or in other formats,
  including Intel HEX and Motorola S-record.  The programming tool converts the
  ROM image to the format One ROM requires.

- **Slot** - a region of One ROM's flash holding a single ROM image, set of ROM
  images, or plugin.

- **Image selection** - the choice of which ROM image a One ROM serves.  A One
  ROM can hold multiple images, selected at boot using the image select jumpers
  on the board.

- **Configuration** - the description of what a One ROM should do, provided to
  the programmer.  It names the ROM images to serve, the chip type and chip
  select behaviour of each, and any settings.

- **Programmer** - a tool that composes ROM images, configuration, Base Firmware
  and metadata into a complete image, and flashes it to the device.  There are
  three - the [web programmer](https://onerom.org/web), the
  [CLI](https://onerom.org/cli) and [Studio](https://onerom.org/studio).

- **Metadata** - a binary block that describes the hardware properties of a
  physical One ROM device, and the properties of the ROM images to be served.
  The programmer generates this metadata from the configuration provided, and
  it is included in the firmware that is flashed to the device.

- **Bootloader** - a small program contained in One ROM's own read-only memory
  that allows One ROM to be reflashed with new firmware and metadata.  The
  bootloader is always accessible, even if the firmware and metadata are
  corrupted or missing and, because it is ROM based itself, it cannot be
  corrupted or erased.

- **Header pins** - pins on the One ROM board exposing power, ground, the image
  select lines, the debug interface and spare pins for expansion.  They allow
  One ROM to be wired to the system it is installed in, or to external hardware.

- **Plugin** - a binary blob consisting of code that extends One ROM's
  capabilities beyond those provided by the core firmware.  The most common
  plugin is the System Plugin.  A user chooses which plugins to include in
  their firmware (if any) at programming time.  Up to two plugins can be
  included with a single One ROM firmware image - the System Plugin, and a
  User Plugin.

- **System Plugin** - a plugin that provides a USB stack and other capabilities
  to One ROM.  One ROM's System Plugin is shipped alongside One ROM's Base
  Firmware.  Its use is recommended for all users, except those who want to
  replace it with their own custom, replacement plugin.

- **User Plugin** - a plugin installed alongside the System Plugin providing
  additional capabilities to One ROM.  User Plugins can be developed by anyone,
  and a number of them are shipped alongside Base Firmware and System Plugin.
  User Plugins require a System Plugin to be included.  An example User Plugin
  is One ROM's Host Control plugin, which allows One ROM to be controlled by the
  system it is installed in, with no extra wiring using the ROM Bus Control
  Protocol (RBCP).

- **Setting** - a named value that changes how a One ROM behaves rather than
  what it serves.  Settings are configured by the user and written by the
  programmer.  The status LED and the CPU frequency are example settings
  supported by the Base Firmware.  Plugins may have their own settings.

- **ROM Bus Control Protocol (RBCP)** - a
  [protocol](https://github.com/piersfinlayson/rom-bus-control-protocol) that
  allows ROM emulators like One ROM to be controlled by a system it is
  installed in, with no extra wiring.  Supported by One ROM's Host Control plugin.

- **Bricked One ROM** - a One ROM device that is non-functional because it has
  been flashed with corrupted firmware or metadata, metadata for a different
  physical One ROM device, no metadata, or firmware or metadata with some other
  issue.  Bricked One ROMs can be recovered by entering One ROM's bootloader
  and reflashing them with valid firmware and metadata.  It is very unlikely
  that a bricked One ROM cannot be recovered.

## Types of One ROM Deployments

### Minimal

Consists of:

- Base Firmware
- Metadata
- At least one ROM image

Contains no plugins.  Serves the ROM image(s) to the system it is installed in,
but cannot be managed while running.

In this deployment type, when One ROM is plugged in via USB it drops
automatically into its bootloader, allowing the device to be reprogrammed, and
simultaneously stops serving the ROM image(s) to the system it is installed in.

### Standard

A Minimal Deployment plus One ROM's System Plugin, which includes a USB stack
enabling comprehensive management of the device while it is running.

This is the recommended deployment type for most users.

### Extended

A Standard Deployment plus a User Plugin.  The User Plugin may be One ROM's own,
or one written by a third party.

### Custom

A Custom Deployment is one where one or more of the following holds:

- The physical One ROM device is not manufactured from a
  [published design](/hardware/pcb/README.md).  It may be a derivative, or a
  fully custom design.

- The Base Firmware is a fork of or replacement for One ROM's Base Firmware.

- The System Plugin is replaced with a fork of or replacement for One ROM's
  System Plugin.
