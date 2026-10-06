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
  Unknown           - Firmware: n/a   State: Stopped Serial: DE3F9C232F655B6B
```

`scan` reads a device's board and firmware version from its firmware. Where it
can't read the firmware it reads the board from the device's commissioning data
if present.

## Programming it again

**You have to supply the One ROM board information** unless the board has been
commissioned. An uncommissioned One ROM's board type is only identifiable from
the firmware already programmed to it, and that either missing or wrong.
The board name is the pin count and the
revision letter silkscreened on the board — `fire-24-f` is a 24-pin board,
revision F. The marking is small.
[`onerom board list`](/docs/CLI-MANUAL.md#board-list) prints every name.

To re-program with the CLI, add `--unrecognised` and `--board`:

```
onerom program --unrecognised --board fire-24-f --config c64.json
```

With the [browser programmer](https://onerom.org/web), pick the board yourself
in the same way. It will ask you to confirm the board type before it writes.

If an uncommissioned board was mis-flashed rather than left blank, the browser
programmer reads the board from the wrong firmware still in flash.  A warning
appears and you can continue.  Check the silkscreen once more before you do,
because the same warning appears when the board is right and the name you
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