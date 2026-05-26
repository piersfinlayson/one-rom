# CLI Changelog

## v0.1.9 - 2026-??-??

- New 28 and 32 pin boards - fire-28-c and fire-32-b.

## v0.1.8 - 2026-05-26

- Re-added 23QL384.
- Add `onerom chips` to show supported chip types and their aliases.
- Fixed `onerom program --scan-slots`, to correctly re-query One ROM after programming.
- More ROM type aliases.

## v0.1.7 - 2026-05-18

- Added 27C100 type as synonym of 27C301/27C1000
- Added `onerom image swap-bytes` to swap bytes in a 16-bit ROM image file.

## v0.1.6 - 2026-05-14

- Moved 23QL384 support to a 23QL512 type.
- Fixed bug introduced in 0.1.5 where a (benign) error is reported requerying One ROM after programming.

## [v0.1.5] - 2025-05-12

- Added _prototype_ support for the new 23QL384 ROM type.  May be deprecated or modified in a future release.
- Add support for `onerom program .. --scan-slots` to auto-run `onerom scan --slots` after programming. 

## [0.1.4] - 2026-05-08

- Add 2364, 2732, 2716, 2708 and 2704 support on One ROM 28 boards.  See the main [CHANGELOG](/CHANGELOG.md) for important notes on this support, including warnings about potential damage if not used correctly.

## [0.1.3] - 2026-04-25

- Add support for labels/names for slots.

# [0.1.2] - 2026-04-02

- Add batch programming mode for programming multiple devices with the same firmware more quickly.
- Add 27C080, 28C16, 28C64, 28C256 and 28C512 ROM support.

## [0.1.1] - 2026-03-26

- Allow program --fw as well as --firmware.
