# Changelog

## [0.1.2] - 2026-08-09

- Discover the neopixel and status-LED GPIOs from device metadata at runtime
  (via `ORA_ID_GET_METADATA_UINT`) instead of hard-coding them by MCU package.
- On boards where the status LED and neopixel share a GPIO, honour the live
  status-LED state (`STATUS_LED_STATE`): the neopixel keeps cycling while the
  shared discrete LED reflects whether the status LED is on or off.
- Requires firmware v0.7.1 (metadata getter API); `min_fw_version` raised to
  0.7.1.

## [0.1.1]

- Earlier releases predate this changelog.
