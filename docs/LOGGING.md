# Logging

One ROM logs what it is doing, and there are two ways to read it: over USB to a serial terminal, which needs nothing but a cable, and over the SWD interface using RTT, which needs a debug probe.

Boot logging is always built in.  Extra debug logging is off by default, and is enabled by building from the repo root with:

```bash
DEBUG_LOGGING=1 make
```

## Over USB

One ROM's system USB plugin sends the log to its CDC serial port.  Attach a terminal — `onerom monitor log`, or `screen`, `minicom` or any other — and the log appears there.  This needs the USB plugin, firmware v0.7.2 or later, and a device that is running.  It does not need a debug probe.

**One ROM sends nothing until a terminal opens the port.**  That is deliberate: it leaves the log readable by a debug probe on a device that merely happens to have USB.  Until then the log accumulates exactly as it would on a device with nothing attached at all.

**What built up before you attached is sent too, not thrown away.**  The log buffer drops the newest record when it is full rather than evicting the oldest, so what is waiting when you connect is the earliest output since anything last drained it.  On a device nothing has been listening to, that is its boot log.

**A debug probe and a terminal must not both read the log.**  Both advance the same read position, so with both attached the output splits arbitrarily between them and neither sees all of it.  Nothing detects this — both look like correct readers.

### The banner

Every time a terminal opens the port, One ROM writes a short block identifying itself before any log content:

```
----- One ROM USB log -----
One ROM fire-28-c v0.7.2
Serial: 2E4A671D1C92AE5C
Logging: boot, plugin-internal, error, plugin-application
---------------------------
```

The board and firmware version follow the device.  `Name:` is present only when an instance name is set, leaving `Serial:` alone when it is not.  `Logging:` lists the kinds of output that are switched on — see below.

The banner's rules are deliberately unlike the `-----` the boot log opens with, so that the block the port writes about itself is not read as part of the log that follows it.

Where One ROM cannot send its log at all, the block is followed by a line saying why:

```
!!! Firmware v0.7.2 or later needed for USB logging !!!
!!! USB logging unavailable - another plugin is already reading the log !!!
```

### What gets logged

A quiet terminal is usually correct.  Most of what One ROM can log is switched off in a released build, and the banner's `Logging:` line names what is on.

| Kind | What produces it | Switched on by |
| --- | --- | --- |
| `boot` | One ROM's own boot messages | `onerom program --boot-logging true`, per device.  Suppressed by turbo boot |
| `debug` | One ROM's verbose messages | firmware built `DEBUG_LOGGING=1`, plus boot logging as above |
| `plugin-internal` | a plugin's log messages | firmware built `PLUGIN_LOGGING=1` |
| `plugin-debug` | a plugin's verbose messages | firmware built with both of the above |
| `error` | errors, from One ROM and from plugins | always |
| `plugin-application` | output a plugin sends itself, rather than a log message | always |

**Not all of it need come from One ROM.**  `plugin-application` covers whatever a plugin chooses to send, and the [host-control plugin](/plugins/user/host-control/README.md) sends what the retro system gives it: a host can write bytes over the ROM bus using RBCP's Pipes group, and they arrive here.  So text on this terminal may be output from the machine One ROM is fitted in — a C64 printing to it, or a diagnostic ROM reporting from an Amiga with no serial port — rather than anything One ROM has to say.

It shares the one channel with everything above so a host's bytes and One ROM's own logging arrive interleaved.

`PLUGIN_LOGGING` and `DEBUG_LOGGING` both default to off and are not set by release builds, so on a downloaded firmware only `error` and `plugin-application` are unconditional, and `boot` depends on how the device was programmed.  A plugin can ask which are live through the plugin API rather than guessing — see `ora_log_category_enabled` in [`api.h`](/firmware/ora/api.h).

## Boot logging

The examples in this section are representative of what is logged, but were captured from very old firmware.

This logs the boot process.  It stops when the device enters its main ROM serving loop, as with logging enabled the device cannot hit the required performance.

Boot logging costs around 1.5ms of One ROM's startup time, taking it to roughly 3ms total.  That is substantially below most retro systems' reset circuit timers, so One ROM boots and is ready to serve the configured ROM image well before it is required.

Sample boot logs from a startup of firmware built with [`c64-no-destestmax.mk`](/old-config/c64-no-destestmax.mk), are shown below:

```log
13:49:28.943: -----
13:49:28.943: SDRR v0.2.0 (build 1) - https://piers.rocks/u/sdrr
13:49:28.943: Copyright (c) 2025 Piers Finlayson <piers@piers.rocks>
13:49:28.943: Build date: Jul 13 2025 13:49:23
13:49:28.943: Git commit: bed08d2
13:49:28.943: -----
13:49:28.943: Hardware info ...
13:49:28.943: STM32F411RE
13:49:28.943: PCB rev 24-d
13:49:28.943: Flash size: 512KB
13:49:28.943: Flash used: 82KB
13:49:28.943: RAM: 128KB
13:49:28.943: Target freq: 100MHz
13:49:28.943: Oscillator: HSI
13:49:28.943: PLL MNPQ: 8/100/0/4
13:49:28.943: MCO: disabled
13:49:28.943: Bootloader: disabled
13:49:28.943: -----
13:49:28.943: Pin Configuration ...
13:49:28.943: ROM emulation: 24 pin ROM
13:49:28.943: Data pins D[0-7]: PA7,6,5,4,3,2,1,0
13:49:28.943: Addr pins A[0-15]: PC5,4,6,7,3,2,1,0,8,13,11,12,9,255,255,255
13:49:28.943: CS pins - 2364: PC10 2332: PC10,9 2316: PC10,12,9 X1: PC255 X2: PC255
13:49:28.943: Sel pins: PB0,1,2,255
13:49:28.943: Status pin: PNONE255
13:49:28.943: -----
13:49:28.943: ROM info ...
13:49:28.943: # of ROM sets: 4
13:49:28.943: Set #0: 1 ROM(s), size: 16384 bytes
13:49:28.943:   ROM #0: kernal.901227-03.bin, 2364, CS1: Active Low, CS2: -, CS3: -
13:49:28.943: Set #1: 1 ROM(s), size: 16384 bytes
13:49:28.943:   ROM #0: basic.901226-01.bin, 2364, CS1: Active Low, CS2: -, CS3: -
13:49:28.943: Set #2: 1 ROM(s), size: 16384 bytes
13:49:28.943:   ROM #0: characters.901225-01.bin, 2332, CS1: Active Low, CS2: Active High, CS3: -
13:49:28.943: Set #3: 1 ROM(s), size: 16384 bytes
13:49:28.943:   ROM #0: dead%20test.BIN, 2364, CS1: Active Low, CS2: -, CS3: -
13:49:28.943: -----
13:49:28.943: Running ...
13:49:28.943: !!! VOS not ready - proceeding anyway
13:49:28.943: Set VOS to scale 1
13:49:28.943: Configured PLL MNPQ: 8/100/0/4
13:49:28.943: Set flash config: 3 ws
13:49:28.943: ROM sel/index 0/0
13:49:28.943: ROM kernal.901227-03.bin preloaded to RAM 0x20000000 size 16384 bytes
13:49:28.943: Set ROM count: 1, Serving algorithm: 0, multi-ROM CS1 state: -
13:49:28.943: Start main loop - logging ends
```

Pulling out some highlights:

- The `Hardare info` section logs provide information about the One ROM hardware that this firmware image **was built for**.  This may not necessarily the hardware you are running the firmware on.
- The `Pin Configuration` section logs show the pin mapping used by the firmware, including:
  - The data pins (D[0-7])
  - The address pins (A[0-15])
  - The chip select (CS) pins for each ROM type
  - The image select pins (SEL0, SEL1, SEL2)
  - The status LED pin (if supported)
- The `ROM info` section logs show you how many ROM images are included in the firmare, and some details about them:
  - their file names (as seen by the build process)
  - the ROM type (`2364`, `2332`, `2316`)
  - the chip select line configuration:
    - `0` means active low
    - `1` means active high
    - `-` means not used for this ROM type
- The `Running ...` section logs show One ROM's main activities as it starts up, including:
  - Whether the VOS (voltage scaling) is set to scale 1 (only done on the F411 and only when `FREQ` is > 84Mhz).  While the F405 also required VOS set to scale 1 for high frequency operation, this is its power-on default.
  - What the PLL MNPQ values have been set to (to allow you to check they are as intended to achieve the target `FREQ`).
  - How many flash wait states have been configured (the STM32 required a different number of wait states based on the `FREQ`).
  - `ROM sel/index` shows you what value the image select jumpers were set to, and what index that corresponds to in the firmware.
  - The logs also show the filename of the active ROM image, and whether it has been preloaded to RAM (the default behaviour).
  - Finally, the last line shows that the main loop has started, and that logging has stopped.

## `DEBUG_LOGGING`

This logs extra debug information, on top of the boot logging above.  Its added verbosity can sometimes cause RTT to lose some logs - this is typically shown as blank logs.  However, the RTT buffer has been increased in size, so this should not be a problem in most cases.

It is disabled by default, and is enabled by setting the `DEBUG_LOGGING` configuration option to `1`.  This type of logging is useful for debugging One ROM itself.

Example debug logging:

```log
13:50:58.502: -----
13:50:58.502: SDRR v0.2.0 (build 1) - https://piers.rocks/u/sdrr
13:50:58.502: Copyright (c) 2025 Piers Finlayson <piers@piers.rocks>
13:50:58.502: Build date: Jul 13 2025 13:50:51
13:50:58.502: Git commit: bed08d2
13:50:58.502: -----
13:50:58.502: Hardware info ...
13:50:58.502: STM32F411RE
13:50:58.502: PCB rev 24-d
13:50:58.502: Flash size: 512KB (524288 bytes)
13:50:58.502: Flash used: 84KB 85112 bytes
13:50:58.502: RAM: 128KB (131072 bytes)
13:50:58.502: Target freq: 100MHz
13:50:58.502: Oscillator: HSI
13:50:58.502: PLL MNPQ: 8/100/0/4
13:50:58.502: MCO: disabled
13:50:58.502: Bootloader: disabled
13:50:58.502: -----
13:50:58.502: Pin Configuration ...
13:50:58.502: ROM emulation: 24 pin ROM
13:50:58.502: Data pins D[0-7]: PA7,6,5,4,3,2,1,0
13:50:58.502: Addr pins A[0-15]: PC5,4,6,7,3,2,1,0,8,13,11,12,9,255,255,255
13:50:58.502: CS pins - 2364: PC10 2332: PC10,9 2316: PC10,12,9 X1: PC255 X2: PC255
13:50:58.502: Sel pins: PB0,1,2,255
13:50:58.502: Status pin: PNONE255
13:50:58.502: -----
13:50:58.502: ROM info ...
13:50:58.502: # of ROM sets: 4
13:50:58.502: Set #0: 1 ROM(s), size: 16384 bytes
13:50:58.502:   ROM #0: kernal.901227-03.bin, 2364, CS1: Active Low, CS2: -, CS3: -
13:50:58.502: Set #1: 1 ROM(s), size: 16384 bytes
13:50:58.502:   ROM #0: basic.901226-01.bin, 2364, CS1: Active Low, CS2: -, CS3: -
13:50:58.502: Set #2: 1 ROM(s), size: 16384 bytes
13:50:58.502:   ROM #0: characters.901225-01.bin, 2332, CS1: Active Low, CS2: Active High, CS3: -
13:50:58.502: Set #3: 1 ROM(s), size: 16384 bytes
13:50:58.502:   ROM #0: dead%20test.BIN, 2364, CS1: Active Low, CS2: -, CS3: -
13:50:58.502: Execute from: Flash
13:50:58.502: -----
13:50:58.502: Running ...
13:50:58.502: !!! VOS not ready - proceeding anyway
13:50:58.502: Set VOS to scale 1
13:50:58.502: HSI cal value: 0x7C
13:50:58.502: Not trimming HSI
13:50:58.502: Configured PLL MNPQ: 8/100/0/4
13:50:58.502: PLL started
13:50:58.502: SYSCLK/2->APB1
13:50:58.502: Set flash config: 3 ws
13:50:58.502: PLL->SYSCLK
13:50:58.502: ROM sel/index 0/0
13:50:58.502: ROM filename: kernal.901227-03.bin
13:50:58.502: ROM type 2364
13:50:58.502: ROM size 16384 bytes
13:50:58.502: ROM kernal.901227-03.bin preloaded to RAM 0x20000000 size 16384 bytes
13:50:58.502: Set ROM count: 1, Serving algorithm: 0, multi-ROM CS1 state: -
13:50:58.502: Start main loop - logging ends
13:50:58.502: Serve ROM #0: kernal.901227-03.bin via mode: 0
13:50:58.502: ROM type: 2364
13:50:58.502: CS1 active low
13:50:58.502: -----
13:50:58.502: Register locations and values:
13:50:58.502: GPIOA_MODER: 0x28000000
13:50:58.502: GPIOA_PUPDR: 0x24000000
13:50:58.502: GPIOA_OSPEEDR: 0x0000AAAA
13:50:58.502: GPIOC_MODER: 0x00000000
13:50:58.502: GPIOC_PUPDR: 0xA0000000
13:50:58.502: VAL_GPIOA_ODR: 0x40020014
13:50:58.502: VAL_GPIOA_MODER: 0x40020000
13:50:58.502: VAL_GPIOC_IDR: 0x40020810
13:50:58.502: CS check mask: 0x00000400
13:50:58.502: CS invert mask: 0x00000000
13:50:58.502: Data output mask: 0x28005555
13:50:58.502: Data input mask: 0x28000000
13:50:58.502: ROM table: 0x20000000
13:50:58.502: -----
```
