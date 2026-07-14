# Changelog

# v0.1.23 - 2026-06-05

- Change USB Fire erase timeout to 20s from 10s (to match CLI).

## v0.1.22 - 2026-06-02

- New 28 and 32 pin boards - fire-28-c and fire-32-b.

## v0.1.21 - 2026-05-26

- Re-added 23QL384 ROM type
- More ROM aliases

## v0.1.20 - 2026-05-14

- Moved 23QL384 support to a 23QL512 type.

## v0.1.19 - 2026-05-12

- Added _prototype_ support for the new 23QL384 ROM type.  May be deprecated or modified in a future release.

## v0.1.18 - 2026-05-08

- Add 2364, 2732, 2716, 2708 and 2704 support on One ROM 28 boards.  See the main [CHANGELOG](/CHANGELOG.md) for important notes on this support, including warnings about potential damage if not used correctly.

## v0.1.17 - 2026-04-02

- Add 27C080, 28C16, 28C64, 28C256 and 28C512 ROM support.

## v0.1.16 - 2026-03-26

- Pulls in new boards and chip types (including chip type alias support)
- Adds a "reload" button next to selected config, to streamline config update workflow
- Inherits new, more robust picoboot crate 0.2.1
- Make errors new helpful.
- Supports "running" boards - stop them using the UI before progamming.

## v0.1.15 - 2026-02-25

- Added 2704/2708 ROM support.
- Added prototype One ROM 32 support (27C010, 27C020, 27C040, 27C301 and 27C080 via stacked One ROM 32s).

## v0.1.14 - 2026-02-22

- Fixed #152 (Studio fails to flash large images to One ROM 40 ROMs)

## v0.1.13 - 2026-02-22

Adds One ROM 40 ROM support.

## v0.1.12

## v0.1.11 - 2026-02-3

- Add some keyboard shortcuts to One ROM Studio:
  - r: Rescan devices
  - f: Flash firmware
- Added 231024 support.

## v0.1.9 - 2026-01-22

### Fixed

- Fixed #90 in v0.6.0 when older (pre v0.1.8) versions of One ROM Studio are used to build firmware images with more than one ROM set, One ROM will not boot on any ROM set other than ROM set 0.
- 2732 ROM type serving was broken - the top 2K replicated the bottom 2K.  Fixed (#103).  This included fixing the testing, which had also not caught this issue.

## v0.1.8 - 2026-01-14

### Added

- Support for low level config of firmware at runtime using JSON files #87.

## v0.1.7 - 2026-01-03

### Added

- #77 - Support for serving multi-ROM sets using Fire PIO algorithm.

## v0.1.6 - 2026-01-01

### Added

- 231024 ROM support in JSON config files.
- fire-24-c
- ice-24-i

## v0.1.5 - 2025-12-12

### Added

- fire-28-a

## v0.1.4 - 2025-12-09

### Added

- Support for local files to onerom-fw and onerom-studio.

### Fixed

- Set Windows PE file and product versions to match Cargo version.

## v0.1.3 - 2025-11-24

### Fixed

- Panic (crash) when analyzing a Fire with a debug probe.  Moved to fork of probe-rs with fix for panic.
- Allow multi-rom sets to be built
- Statically link with vcruntime

## v0.1.2 - 2025-11-09

- Built with rustc 1.91
- Move to probe-rs 0.30
- Added ability to load ROM config JSON files from disk in Create view
- Added online manifest to access latest URLs, with local cache file backup, and defaults as further backup
- Added app version update check and download link

## v0.1.1 - 2025-10-30

- Built with rustc 1.90
- Mac and Windows releases now signed.
- Mac app now uses the One ROM liquid glass icon.
- Moved to libusb-less DFU implementation using `dfu-rs` and `nusb` crates.
- Moved to manual rescanning to detect probes and USB devices.
- Added network connectivity icon.
- Single universal macOS dmg installer instead of separate Intel and Apple Silicon versions.
- Added ability to load ROM config JSON files from disk in Create view.
