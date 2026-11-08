# Plugin Examples

This directory contains some simple example One ROM plugins.

| Example | What it does |
| --- | --- |
| [blink](blink) | Blinks One ROM's status LED |
| [hello](hello) | Logs a message in a loop |
| [log-write](log-write) | Claims a log channel and writes raw bytes to it |
| [c64-char-led](c64-char-led) | Watches C64 character ROM accesses and drives the status LED |

Plugins written to test the plugin API on a device, rather than to show how it
is used, live in [tests](../tests/README.md).

## Pre-requisites

### Linux

For Ubuntu/Debian:

```bash
sudo apt -y install build-essential gcc-arm-none-eabi
```

### macOS

Using [Homebrew](https://brew.sh/):

```zsh
brew install --cask gcc-arm-embedded
```

## Building

To build the examples as user plugins:

```bash
make
```

To build them as system plugins:

```bash
make PLUGIN_TYPE=SYSTEM
```

## Programming

Build `hello` as a system plugin and `blink` as a user plugin, then:

```bash
onerom program --plugin file=hello/build/plugin_system.bin \
               --plugin file=blink/build/plugin_user.bin \
               --slot file=some-rom.bin,type=27128
```

The first plugin prints "Hello One ROM" to RTT logging once the system has booted, and the other blinks One ROM's status LED.