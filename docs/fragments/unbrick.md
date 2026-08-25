# Recovering a bricked One ROM

You can recover a One ROM that is not responding using any of the One ROM
programming tools by following the instructions below.  The One ROM CLI is
recommended as it gives greatest control over One ROM.  The CLI commands
are shown below.

## Situation

No tool can find the device. `onerom scan` reports nothing, the browser
programmer sees nothing to connect to, and any command needing a device refuses.
The Web programmer cannot detect it and the CLI reports:

```
$ onerom scan
Scanning ... 
No matching One ROM devices found.

$ onerom inspect info
Failed to execute command.
No One ROM was found or specified.
  Specify a One ROM using --serial.
  Use 'onerom scan' to list connected One ROMs.
```

A One ROM in this state is called bricked. Nothing is damaged. Its
firmware is not running, so nothing answers on the USB bus — programming was
interrupted, or the firmware on it is not right for the board. One ROM has a
hardware bootloader which cannot be bricked, so the recovery is to boot the
device into that bootloader and program it again.

If the One ROM programming tool you are using does find the device, it is not
bricked. Program it as normal.

## Booting into the bootloader

This works on any Fire (RP2350) board whatever state its flash is in.

1. Unplug the One ROM.

2. Connect the **BOOTSEL** pad to ground. It is normally the middle pad/pin on
   the header pins' top row, and the USB shield is a good source of ground.
   The CLI command
   [`onerom board header --board <BOARD>`](/docs/CLI-MANUAL.md#board-header)
   shows the header pins.

   > Fire 24 rev A and Fire 24 USB rev B are the exceptions, and both are rare.
   > Rev A brings BOOTSEL out as a pin towards the bottom of the board, and USB
   > rev B as a small pad on the underside.

3. Plug the One ROM into USB with that connection still made. The status LED
   lights dimly, which is how you know the bootloader is running.

4. Remove the BOOTSEL to ground connection — it is needed only as power comes
   up.

## Checking the host can see it

Connect to One ROM using the programming tool as normal.

Using the CLI `--unrecognised` (`-u`) matches any attached RP2350 board, a
including a Raspberry Pi Pico 2, so make sure only the One ROM is attached:

```
$ onerom scan --unrecognised
Scanning ... 
found 1 connected device:
  Unknown           - Firmware: n/a   State: Unknown Serial: (no serial)
```

The CLI names a device's board, firmware and serial from the firmware it is
holding, and there is none in this example's bricked One ROM it can read.

## Programming it again

**You have to supply the One ROM board information.** A One ROM board type is
only identifiable from the firmware already programmed to it, and that
is the thing that is missing or wrong. The board name is the pin count and the
revision letter silkscreened on the board — `fire-24-f` is a 24-pin board,
revision F. The marking is small.
[`onerom board list`](/docs/CLI-MANUAL.md#board-list) prints every name.

To re-program with the CLI, add `--unrecognised` and `--board`:

```
onerom program --unrecognised --board fire-24-f --config c64.json
```

With the [browser programmer](https://onerom.org/web), pick the board yourself
in the same way. It will ask you to confirm the board type before it writes.

If the board was mis-flashed rather than left blank, both tools notice — the
wrong firmware is still in flash and they read the board from it.  The CLI
refuses, and needs `--force` alongside `--board`.  The browser programmer warns
and lets you continue.  Check the silkscreen once more before you do either,
because the same objection appears when the board is right and the name you
picked is wrong.

Then confirm the device came back up.  With the CLI:

```
onerom scan
```

Getting the board wrong writes the wrong firmware and leaves you with a bricked
device.  In this case, follow the instructions again.

## Ice boards

Ice (STM32) boards use `BOOT0` rather than BOOTSEL, and it is pulled **high**,
to 3.3V, rather than to ground. It is the jumper labelled `B0` or `B`. The
0.7.x CLI does not program Ice boards at all — use the
[Web Programmer](https://onerom.org/web) or
[One ROM Studio](https://onerom.org/studio).