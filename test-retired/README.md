# roms-test

This is a test suite for the `roms.c` output from `sdrr-gen`, verifying that the generated images and data are correct - that is, they are properly mangled in the way that the STM32F4 expects (based on the STM32F4 wiring to the address, data and CS lines).

## Usage

The best way to execute this test suite is to run `make test` from the root directory of the repository.  Pass in whatever environment variables are being using to build your firmware.  For example:

```bash
HW_REV=24-f CONFIG=config/set-c64.mk make test
```

## Details

This

- builds the firmware
- builds the test suite with the same auto-generated `roms.c` file
- test that against the original ROM files.

These tests have an external dependency on `libcurl` and `libzip` which are used to retrieve any ROM files specified with URLs (as opposed to local files), and extract the appropriate file, if in an archive.

```bash
sudo apt install libcurl4-openssl-dev libzip-dev libjson-c-dev
```

On macOS with Homebrew:

```bash
brew install curl libzip json-c
```

Note that this test suite does _not_ yet use the pin mapping from [`rust/config/json`](/rust/config/json/README.md), so will report errors for hardware revisions other than `24-d`, `24-e` and `24-f`.
