# Changelog

## [0.3.0] - 2026-??-??

The command a host sends after one the device refused is no longer lost.
Refusing halts both USB endpoints, and clearing them left the device and the
host expecting different packets, so the next command was discarded.

Answer a picoboot host's request for device information as the RP2350 datasheet
specifies, from picobootx v0.3.0.  The flags word saying what was answered was
missing and the leading count was one short, and a request naming anything this
part cannot answer was refused rather than answered in part.

Four GPIOs can be under a timed hold at once, rather than eight.  Four is every
spare pin a board exposes that a caller can realistically drive, so the old
figure reserved plugin RAM for pins that do not exist.

Answer what One ROM's LEDs are doing.  A new picobootx command reports one
LED's mode, colour, brightness, period and GPIO, read from the firmware's LED
engine rather than from anything the plugin remembers.  A board without the LED
asked for still answers, saying it has none, which is what lets a host tell that
apart from a One ROM too old to be asked.

Forward One ROM's log to the CDC serial port.  Attach a terminal and the
firmware's and the other plugin's logging appears there, including whatever
accumulated before you attached — on a device nothing had been listening to,
that is its boot log.  Nothing is forwarded until a terminal is present, so a
debug probe still reads the log on a device that merely has USB.

- Every time a terminal opens the port, One ROM names itself first — board,
  firmware version, instance name and serial, and which kinds of logging are
  switched on.  A device that cannot forward its log says so there, rather
  than leaving the port silent.
- A debug probe and a terminal must not both read the log at once.  Both
  advance the same read position, so the output splits arbitrarily between
  them and neither sees all of it.

Take the millisecond clock from the firmware instead of keeping one here.
Firmware v0.7.2 counts plugin uptime itself, derived from TIMER0's free-running
microsecond counter, so the plugin no longer takes TIMER0 out of reset, arms its
alarm or handles an interrupt a thousand times a second.  LED beacons, bounded
GPIO holds, the terminal settle window and tinyusb's own timing all read the
firmware's count.

- `min_fw_version` rises to v0.7.2.  The plugin does not load on firmware older
  than that.
- No `TIMER0_IRQ_0` handler is registered any more, so that interrupt is left
  to whatever else wants it.  The firmware arms no alarm of its own.

Restore the status LED correctly after an LED beacon.  The state to restore now
comes from the firmware, which is where the live one lives.  The plugin used its
own record of what it had last set, so a beacon on a device it had not already
driven the LED on left the LED off.

Rebuilt for One ROM firmware v0.7.2, whose logging functions are now checked by
the compiler.  Several log calls passed `uint32_t` to conversions expecting
`unsigned int`.  The output was already correct, so nothing behaves differently.

## [0.2.1] - 2026-08-09

Add support for overriden serial #s.

Add GPIO control over picobootx: `ONEROM_CMD_GET_CAPS`, `ONEROM_CMD_GPIO_SET`
and `ONEROM_CMD_GPIO_QUERY`.  A host can now drive a GPIO high, low or high
impedance, optionally for a bounded period after which the plugin reverts it
itself, and read back what One ROM is using each GPIO for.

- The hold is timed on the device, not by the host, so a pulse still ends if
  the host goes away part-way through it - the point of the exercise, since the
  motivating case is a wire from a header pad to the host system's reset line.
  Up to 8 GPIOs can be held at once, for at most 60s each; a second
  `ONEROM_CMD_GPIO_SET` on a GPIO replaces that GPIO's pending release.
- `ONEROM_CMD_GPIO_SET` is validated and applied in the picoboot dispatch
  handler rather than deferred to the task loop, so the firmware's refusal of a
  GPIO One ROM is itself using reaches the host as a command status instead of
  being swallowed.
- Capabilities are decided once at plugin init from which plugin-API functions
  the running firmware provides.  On firmware that predates the GPIO API the
  feature bits are clear, `num_gpios` is 0 and the host never sends the
  commands, so `min_fw_version` stays at 0.7.0 and the plugin keeps working
  otherwise.
- The custom-command dispatcher no longer rejects every command that has a data
  phase - it validates the length per command instead, which is what lets the
  two commands that return data work at all.

## [0.2.0] - 2026-07-20

Support firmware v0.7.x

## [0.1.1] - 2026-03-26

Fix live ROM image peek/poke for 28 pin chips

## [0.1.0] - 2026-03-25

First release