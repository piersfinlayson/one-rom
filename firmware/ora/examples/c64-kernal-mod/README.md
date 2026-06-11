# c64-kernal-mod

This example modifies the C64 kernal ROM in response to a specific "knock" sequence of bytes from a program running on the C64.

The plugin listens for "ONEROM!" + an unused subsequent byte, then updates the kernal at location 0xE475, which is where the boot banner lives.

The provided `modifykernal.prg` program can be used to test the plugin on a real C64. Load and run it with the One ROM serving the kernal.

You can then either `SYS 64738` to soft reset the C64 or, if the One ROM is otherwise powered (for example by USB), turn the C64 off and on again to see the modified boot banner.

## Building the Plugin

As usual, from the `examples` directory, run:

```bash
make
```

This plugin is created in the `build` directory as `plugin_user.bin`.  Load it onto a One ROM like this:

```bash
onerom program --plugin usb --plugin file=c64-kernal-mod/build/plugin_user.bin --slot file=/path/to/kernal.bin,type=2364,cs1=0
```

## Building the C64 Program

From this (`c64-kernal-mod`) directory, run:

```bash
./build-c64.sh
```

This will create `build/modifykernal.prg`, which can be loaded and run on a real C64 with the One ROM serving the kernal to test the plugin like:

```bash
LOAD "MODIFYKERNAL.PRG",8
RUN
```

## Notes

It is imperative that the C64 program DOES NOT run from the kernal ROM directly, as then the 6510 retrieving instructions would interlave the knock sequence, meaning it goes undetected.

To embed the code in the kernal ROM itself, the program must load itself into RAM first and then jump to it.

It is also critical the program suspends interrupts while the knock sequence is being sent, otherwise the 6510 may interleave an interrupt service routine between the bytes, which would also cause the knock sequence to be missed.
