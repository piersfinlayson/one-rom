# External Flash Tester

Tests a Fire board's external flash chip is present and functional, making
this One ROM suitable for commissioning as an L (large) One ROM.

It checks the chip is compatible with One ROM - specifically:
- answers on the chip select GPIO in the board's metadata
- reports 2MB in its ID
- programs and reads back, with single-line and quad reads
- is 2MB, as offset 2MB reads the same data as offset 0 and offset 1MB doesn't
- erases with the D8h block erase and the 4KB sector erase

On a One ROM commissioned with a size other than M it also checkes whether
the commissioned data matches the expected value for an L board.

Requires One ROM firmware v0.8.0 or later.

## Building

```bash
make
```

Builds `build/plugin_user.bin`.

## Running

Program it with the USB plugin and a ROM image, then open the console.  For
example, on a 40 pin One ROM:

```bash
onerom program --plugin usb --plugin ext-flash \
               --slot file=images/test/rand_256KB.rom,type=27C200 
onerom console
```

For a local build use `--plugin file=build/plugin_user.bin`.

An example passing run - the output may be slightly different

```
One ROM External Flash Tester vX.Y.Z
...
<Detected hardware info>
...
This test erases, writes and reads the external flash.
Takes 20-25s with a new flash chip and One ROM clocked at 150MHz.
Type y then Enter to proceed.
...
<Individual test information>
...
PASS
Type y then Enter to repeat.
```

A failed run replaces `PASS` with `FAIL`.

The test erases, reads and writes the external flash chip. It leaves the chip
blank at the end of a test run.

If a flash operation hangs, the watchdog resets the board after 4 seconds and
the plugin reports the error on the next start.

## Flash clock

The test runs the chip at the firmware's flash clock which is a divided value
of One ROM's own CPU clock.  To run the flash at its maximum supported 133MHz,
you must overclock One ROM to 266MHz.

Take care when performing overclocking as it can damage your One ROM.

```
onerom program --plugin usb --plugin ext-flash \
               --slot file=images/test/rand_256KB.rom,type=27C200,cpu=266MHz
```

If One ROM fails to boot, recover it using the instructions in
[unbrick.md](/docs/fragments/unbrick.md). You can try again setting vreg:

```
onerom program --plugin usb --plugin ext-flash \
               --slot file=images/test/rand_256KB.rom,type=27C200,cpu=266MHz,vreg=1.30V
```

Setting vreg above its stock 1.1V can damage your One ROM and it is not
recommended to set vreg above 1.60V.
