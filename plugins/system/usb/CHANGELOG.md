# Changelog

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