# Plugin Examples

This directory contains some simple example One ROM plugins.

| Example | What it does |
| --- | --- |
| [blink](blink) | Blinks One ROM's status LED |
| [hello](hello) | Logs a message in a loop |
| [c64-char-led](c64-char-led) | Watches C64 character ROM accesses and drives the status LED |
| [c64-kernal-mod](c64-kernal-mod) | Rewrites the C64 kernal boot banner in response to a knock sequence |

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

## Using

Once built, configure the plugins as One ROM slots.  For example:

```bash
onerom program --slot file=examples/hello/build/plugin_system.bin,type=system_plugin \
               --slot file=examples/blink/build/plugin_user.bin,type=user_plugin \
               --slot file=some-rom.bin,type=27128
```

The first plugin prints "Hello, world!" to RTT logging once the system has booted, and the other blinks One ROM's status LED.