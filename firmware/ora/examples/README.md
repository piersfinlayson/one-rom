# Plugin Examples

This directory contains some simple example One ROM plugins.

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