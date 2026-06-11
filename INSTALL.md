# INSTALL

One ROM requires a fairly involved toolchain to build, due to the extent of the project (embedded firmware, extensive RUST tooling, desktop application, webassembly, etc).

You are _strongly_ recommended to use the [Docker container](ci/docker/README.md#building-one-rom) to build the One ROM firmware, as this contains a pre-configured build environment that works across multiple host platforms.

If you'd like to build the toolchain and dependencies locally, read on.

This document covers installing the toolchain and dependencies on linux (primarily focusing on an x86_64 Debian-based distribution, although notes are also provided for an ARM64 based host).

Other hosts (Mac, Windows) are possible, and it is recommended to use macOS for building One ROM Studio for Mac, and Windows for building Windows installers.

However, we strongly recommend sticking to a *nix based host (Linux or macOS) for building the One ROM firmware itself, and instructions for settig up a full Windows build host are not included below.

0. Install pre-requisites

    ```bash
    sudo apt -y install git build-essential curl pkg-config
    ```

1. Clone the repository:

    ```bash
    git clone https://github.com/piersfinlayson/one-rom.git
    cd one-rom
    ```

2. Install the required ARM GNU toolchain.  You have options here.

    - Install it [from ARM's website](https://developer.arm.com/downloads/-/arm-gnu-toolchain-downloads) for AArch32 bare-metal target (arm-none-eabi).

        Recommended approach - download the toolchain from ARM's developer site (this is quite large, so may take a while) - this is for x86_64 linux hosts::

        ```bash
        wget https://developer.arm.com/-/media/Files/downloads/gnu/14.3.rel1/binrel/arm-gnu-toolchain-14.3.rel1-x86_64-arm-none-eabi.tar.xz
        tar -xvf arm-gnu-toolchain-14.3.rel1-x86_64-arm-none-eabi.tar.xz
        sudo mv arm-gnu-toolchain-14.3.rel1-x86_64-arm-none-eabi /opt/
        ```

        If you are on ARM64 linux, or a Mac (Intel or ARM), select the correct version from ARM's site.  Again update TOOLCHAIN.

    - Install it via your package manager, e.g., on Debian/Ubuntu:

        ```bash
        sudo apt -y install gcc-arm-none-eabi
        ```

    Now you will need to update the `TOOLCHAIN` environment variable in your shell or variable in the [Makefile](firmware/Makefile) to point to the correct compiler binary directory.  It should probably `/usr/bin` or `/opt/arm-gnu-toolchain-14.3.rel1-darwin-arm64-arm-none-eabi/bin` or similar.

    If on an ARM64 host you will also need x86_64-linux-gnu cross tools:

    ```bash
    sudo apt -y install gcc-x86-64-linux-gnu
    ```

3. Install the following packages required for building and testing.  `vice` is optional.  (`vice` is used to build some Commodore demo programs.):

    ```bash
    sudo apt -y install jq libcurl4-openssl-dev libzip-dev libjson-c-dev libudev-dev vice
    ```

    If you are using a different package manager, the package name may vary slightly, e.g., `libcurl-devel` on Fedora.

    On macOS you would be using [Homebrew](https://brew.sh/).

4. Install [Rust](https://www.rust-lang.org/tools/install) - this will take a while:

    ```bash
    curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs | sh
    source $HOME/.cargo/env
    rustup target install thumbv7em-none-eabihf
    rustup target install thumbv8m.main-none-eabihf
    cargo install cross
    cargo install wasm-pack   # Only required to build one-rom-wasm
    cargo install cargo-dist  # Only required to build One ROM Studio installers
    ```

    If planning to build One ROM Studio for all possible targets (you likely only want to build a subset!) you will also need to install additional Rust targets and the mingw-w64 toolchain for Windows targets.  If you just want to build the One ROM firmware you do not need to do this step.
    
    ```bash
    rustup target install \
        x86_64-unknown-linux-gnu \
        aarch64-unknown-linux-gnu \
        x86_64-pc-windows-gnu \
        aarch64-pc-windows-gnullvm \
        x86_64-pc-windows-msvc \
        aarch64-pc-windows-msvc \
        x86_64-apple-darwin \
        aarch64-apple-darwin
    sudo apt -y install mingw-w64
    ```

5. Install [probe-rs](https://probe.rs/) for flashing the firmware to One ROM using an SWD programmer.  This is optional if you want to just build the firmware and use another tool to flash it.

    ```bash
    curl --proto '=https' --tlsv1.2 -LsSf https://github.com/probe-rs/probe-rs/releases/latest/download/probe-rs-tools-installer.sh | sh
    probe-rs complete install
    ```

6. Connect up One ROM to your [programmer](README.md#programmer).

At this point you can follow the instructions below to build and flash the firmware.

## Building the Firmware

To build the firmware, you use a command like this:

```bash
scripts/onerom.sh fire-24-d onerom-config/vic20-pal.json
```

To flash, use `-f`, to include regular and debug logging use `-l` and `-d` respectively.

You can also use make commands as described below, but running make directly has been deprecated in favour of the `scripts/onerom.sh` script.

## Programming the Firmware

### USB

USB is the simplest way to program One ROM if your hardware revision supports.

After building the firmware as above, use the binary from `firmware/build/onerom-rp235x.bin` and one of the following tools to update the firmware of your device.  You have two official One ROM options:
- [One ROM CLI](https://onerom.org/cli)
- [One ROM Studio](https://onerom.org/studio)
- [One ROM Web](https://onerom.org/web)

If both cases, you need to select the option to upload a local firmware binary, and then program it.

You also have board specific, third-party, options:

#### Fire Boards

- [pico⚡flash](https://picoflash.org) - A web based RP2040/RP2350 flash by One ROM's author. 
- [picotool](https://github.com/raspberrypi/picotool) - A command line tool from Raspberry Pi for programming Raspberry Pi RP2040/RP2350-based boards.

As well as `firmware/build/onerom-rp235x.bin`, if you have [picotool](https://github.com/raspberrypi/picotool) installed and in your path, a UF2 file is created as part of the build for Fire boards at `firmware/build/onerom-rp235x.uf2`.

For a factory fresh Fire board, you can copy this UF2 to the RP2350 filesystem that mounts when you plug in the Fire board to program it.

Note that the RP2350 filesystem is not automatically mounted when plugged into USB once you have One ROM firmware v0.6.0+ installed, but you can access it by pulling BOOT to GND on power up to enter this mode.

### SWD Programmer

Using SWD has an advantage over USB of being able to view debug logs from One ROM after re-programming.

However, you will need to find some way to power One ROM while programming, as, unlike USB, SWD does not provide power.  You can power One ROM by installing it in your retro system and powering that on, or by providing 5V and GND to the appropriate pins on One ROM directly.  ⚠️ If you are powering One ROM directly, **do not** install it in a retro system at the same time, as this may damage your One ROM, programmer, or retro system.

There are many tools that can be used to program One ROM via an SWD programmer.  We use [probe-rs](https://probe.rs/), which you may have installed already.

If you installed `probe-rs`, you can a command like this to build and flash the firmware using an SWD programmer in a single step - replace XXX with the appropriate build config for your hardware revision, MCU and ROM set configuration:

```bash
XXX make run
```

Note that as well as `firmware/build/onerom-rp235x.bin`, an ELF file is created at `firmware/build/onerom-rp235x.elf` which can be used with other SWD programming tools, as it contains build symbols.  This is particularly useful for attaching to One ROM with the programmer, after it has been programmed, to view logs. 

See [Pi-PICO-PROGRAMMER](/docs/PI-PICO-PROGRAMMER.md) for details of using a Raspberry Pi Pico as an inexpensive SWD programmer.  Many other SWD programmers are available, like the Raspberry Pi Debug Probe, generic DAPLink, ST-Link, etc. 

Occassionally your One ROM may lock up, particularly if you are experimenting with overclocking or other advanced configuration options, or debugging firmware changes.  If this is is the case, try rebooting your programmer, One ROM, or both, and try again.  If you still have problems, see [Recovering a Bricked Device](docs/GETTING-STARTED.md#recovering-a-bricked-device) for help.

## Additional Make Targets

To build and then review the contents of the firmware run:

```bash
XXX make info
XXX make info-detail # More details
```

To perform consistency checking on the firmware run the following:

```bash
XXX make test
```

Not all ROM types support this testing.  Please raise an issue if your specific ROM type fails this test.

## Debugging

To enable both high-level logging and debug logging, use the following when building:

```bash
BOOT_LOGGING=1 DEBUG_LOGGING=1 CONFIG=config/vic20-pal.mk make
```
