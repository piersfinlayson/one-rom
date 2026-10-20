# Logging

Logs can be either:
- using serial over USB
- over the SWD interface using RTT using a debug probe
- directly from RAM using PICOBOOT

Boot logging is always compiled in.  Extra debug logging is off by default, and is enabled by building from the repo root with:

```bash
DEBUG_LOGGING=1 make
```

As of firmware v0.8.0 debug logging does not compile, as the image becomes too large for the firmware's flash region.

## USB

One ROM's system USB plugin sends the log to its CDC serial port.  Use a terminal to view it, for example `onerom monitor log`, `screen`, `minicom`, etc.

Data is only sent once the terminal has opened the port.  If too much logging is queued up, the newest logs are discarded.

Any data typed into a terminal is passed to any plugin that has requested it.  By default, it is ignored.

If both a debug probe and a terminal are reading the log simultaneously, they will likely conflict, with each draining portions of logging.

### Plugin Serial Port Usage

A plugin can claim exclusive use of the serial port.  In this case, One ROM's log is moved to a different RAM location by the firmware when the plugins are started up, and the logs can be retrieved using RTT channel 1.  `onerom monitor log` automatically reads it from whatever location it's at.

### Connecting

Upon terminal connect a banner like the following is output:

```
----- One ROM USB log -----
One ROM fire-28-c v0.7.2
Serial: 2E4A671D1C92AE5C
Logging: boot, plugin-internal, error, plugin-application
---------------------------
```

If the USB plugin is unable to send any logging a warning is output. For example:

```
!!! Firmware v0.7.2 or later needed for USB logging !!!
!!! USB logging unavailable - another plugin is already reading the log !!!
```

### Log Types

| Type | What | Enabling |
| --- | --- | --- |
| `boot` | Firmware boot logs | `onerom program --boot-logging true`.  Suppressed by turbo boot |
| `debug` | Firmware verbose logs | Firmware built with `DEBUG_LOGGING=1` |
| `plugin-internal` | Plugin logs | Firmware built with `PLUGIN_LOGGING=1` |
| `plugin-debug` | Plugin verbose logs | Both `DEBUG_LOGGING=1` and `PLUGIN_LOGGING=1` |
| `error` | Firmware and plugin errors | Always enabled |
| `plugin-application` | Other logs produced by specific plugin applications | Always enabled |

All types of logging share the same channel.