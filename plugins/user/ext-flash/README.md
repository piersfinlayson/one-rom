# External flash probe

Used to test various operations on the external flash connected to the RP2350's QSPI interface.

May be useful as a diagnostic tool or reference.

## Building

```bash
make
```

Builds `build/plugin_user.bin`.

## Programming

Must be deployed alongside the USB plugin.  A ROM slot must be present to avoid the firmware entering limp mode.

```bash
onerom program \
  --slot file=<rom>,type=27C400 \
  --plugin usb \
  --plugin file=build/plugin_user.bin
```

## Running

```bash
onerom console
```

A probe runs at startup:
- identify both flash chips
- report the QMI's window configuration
- say whether OTP or the plugin configured CS1
- muxes the CS1 pin, unless the bootrom already has.

The test sector is the 4KB at offset 0 of the external flash at 0x11000000.  The test pattern used is `i * 197 + 89`.

| Key | Action |
| --- | --- |
| `?` | List the commands |
| `p` | Re-run the probe |
| `e` | Erase the test sector via `flash_op`, then check it reads erased |
| `E` | Erase the test sector via `flash_range_erase` |
| `g` | Write the test sector with the pattern via direct QSPI |
| `G` | Write the test sector via `flash_op` |
| `R` | Write the test sector via `flash_range_program` |
| `v` | Read the test sector at each clock divisor from 6 down to 1 |
| `X` | Write a routine to the external flash, execute it, erase it |
| `b` | Erase and program using the bootrom's `FLASH_DEVINFO` |
| `w` | `e`, `g`, `v`, `e` in sequence |
| `x` | Hang to check the watchdog resets the board |

Notes:

- An erase parks the other core during the operation.

- The watchdog is armed around external flash operations so a hung operation resets the board.  `x` hangs the plugin on purpose to check the watchdog is operational.

## Programming OTP

`scripts/program-otp.sh` programs `FLASH_DEVINFO` and `FLASH_DEVINFO_ENABLE` in OTP so the bootrom configures CS1 itself. See [docs/wip/EXTERNAL-FLASH.md](/docs/wip/EXTERNAL-FLASH.md#programming-otp) for further details.
