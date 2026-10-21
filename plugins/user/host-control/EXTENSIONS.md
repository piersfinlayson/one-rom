# One ROM RBCP Extensions

The host-control plugin implements the [ROM Bus Control Protocol (RBCP)](https://github.com/piersfinlayson/rom-bus-control-protocol).

This document specifies the values and commands host-control adds in the ranges reserved by RBCP for implementation-specific use.

All multi-byte values are little-endian.

---

## Implementation Specific Command Groups

| Group | Name | Since | Valid Modes | Description |
|-------|------|-------|-------------|-------------|
| 0x80 | Device Info | 0.1.7 | Command-Response only | Read One ROM's commissioning values, device metadata and other device information |

The Since column is the host-control version.

---

## Group 0x80 — Device Info

Each command in this group corresponds to a plugin API call and uses the same keys.

- Commissioning keys are the `ORA_OTP_KEY_*` and `ORA_COMMISSIONING_VERSION_KEY` values in [onerom_constants_generated.h](/firmware/ora/onerom_constants_generated.h). Commissioning values are written to OTP when a One ROM is commissioned.
- Metadata keys are the `ORA_METADATA_KEY_*` values in [onerom_metadata_keys_generated.h](/firmware/ora/onerom_metadata_keys_generated.h). Each metadata value is a string, a number or an array.

Key numbers are permanent.

| CMD | Name | Since | Args | Description |
|-----|------|-------|------|-------------|
| 0x00 | GET_COMMISSIONING_VERSION | 0.1.7 | 0 | Writes the version of One ROM's commissioning data into the response data section. The version is 0 if One ROM isn't commissioned. If the version is unsupported GET_COMMISSIONING_VALUE fails for every key. See [GET_COMMISSIONING_VERSION Response Format](#get_commissioning_version-response-format). Fails if the response data section is smaller than 4 bytes. |
| 0x01 | GET_COMMISSIONING_VALUE | 0.1.7 | 5: A0=count, A1/A2=offset, A3/A4=key | Writes up to count bytes of key's commissioning value from offset into the response data section. A count of zero indicates 256 bytes. See [Value Response Format](#value-response-format). Fails if the value isn't present, if offset is beyond the end of the value, or if the response data section is smaller than 4 bytes plus count. An A4 value of 0xAA is invalid and rejected. |
| 0x02 | GET_METADATA_STR | 0.1.7 | 7: A0=count, A1/A2=offset, A3–A6=key | Writes up to count bytes of key's metadata string from offset into the response data section. A count of zero indicates 256 bytes. An unset string has length 0. See [Value Response Format](#value-response-format). Fails if key is unknown or isn't a string, if offset is beyond the end of the string, or if the response data section is smaller than 4 bytes plus count. An A6 value of 0xAA is invalid and rejected. |
| 0x03 | GET_METADATA_UINT | 0.1.7 | 4: A0–A3=key | Writes key's metadata number into the response data section. See [GET_METADATA_UINT Response Format](#get_metadata_uint-response-format). Fails if key is unknown or isn't a number, or if the response data section is smaller than 4 bytes. An A3 value of 0xAA is invalid and rejected. |
| 0x04 | GET_METADATA_ARRAY | 0.1.7 | 7: A0=count, A1=index, A2=width, A3–A6=key | Writes up to count elements of key's metadata array from element index into the response data section. Each element is width bytes. Width is 1, 2 or 4. A count of zero indicates 256 elements. See [GET_METADATA_ARRAY Response Format](#get_metadata_array-response-format). Fails if key is unknown or isn't an array, if index is beyond the last element, if width isn't 1, 2 or 4, if an element is too large for width, or if the response data section is smaller than 4 bytes plus count × width. An A6 value of 0xAA is invalid and rejected. |
| 0x05 | GET_COMMISSIONING_INSTANCE | 0.1.7 | 3: A0=count, A1/A2=offset | Writes up to count bytes of One ROM's current commissioning instance from offset into the response data section. The instance is the OTP rows from its magic row to the end of its signature entry, two bytes per row, low byte first, as [OTP.md](/docs/OTP.md) specifies. A count of zero indicates 256 bytes. See [Value Response Format](#value-response-format). Fails if One ROM isn't commissioned or its commissioning version is unsupported, if offset is beyond the end of the instance, or if the response data section is smaller than 4 bytes plus count. An A2 value of 0xAA is invalid and rejected. |

CMD 0xAA is reserved and must never be assigned.

### GET_COMMISSIONING_VERSION Response Format

| Offset | Size | Field | Description |
|--------|------|-------|-------------|
| 0 | 2 | version | Version of One ROM's commissioning data. 0 if One ROM isn't commissioned. |
| 2 | 2 | Reserved | Must be zero |

### Value Response Format

For GET_COMMISSIONING_VALUE, GET_COMMISSIONING_INSTANCE and GET_METADATA_STR.

| Offset | Size | Field | Description |
|--------|------|-------|-------------|
| 0 | 2 | length | Length of the whole value in bytes |
| 2 | 2 | Reserved | Must be zero |
| 4 | n | value | The value's bytes from offset. n is the smaller of count and the number of bytes after offset. |

A string isn't null-terminated.

### GET_METADATA_UINT Response Format

| Offset | Size | Field | Description |
|--------|------|-------|-------------|
| 0 | 4 | value | The metadata value |

### GET_METADATA_ARRAY Response Format

| Offset | Size | Field | Description |
|--------|------|-------|-------------|
| 0 | 1 | returned | Number of elements that follow. Fewer than count if the array is shorter than index plus count. A value of 0 indicates 256 elements. |
| 1 | 3 | Reserved | Must be zero |
| 4 | returned × width | elements | Elements from index |

### Example — Board Size and Chip ID

Non-normative.

`ORA_METADATA_KEY_BOARD_SIZE` is 0x14 and `ORA_METADATA_KEY_CHIP_ID` is 0x1A.

| Command | Bytes | Response data |
|---------|-------|---------------|
| GET_METADATA_UINT for BOARD_SIZE | `80 03 14 00 00 00` | `02 00 00 00` (size L) |
| GET_METADATA_ARRAY for 8 1-byte elements of CHIP_ID | `80 04 08 00 01 1A 00 00 00` | `08 00 00 00 E1 26 C9 F9 7C 10 AD AC` (chip ID E126C9F97C10ADAC) |

---

## Auxiliary Pin Group Types

Implementation specific group types are returned by GET_AUX_GROUP_INFO.

| Value | Since | Group |
|-------|-------|-------|
| 0x80 | 0.1.3 | Image select pins. Pin 0 is SEL0. |
| 0x81 | 0.1.3 | X pins. Pin 0 is X1. |

## LED Modes

Implementation specific LED modes used by SET_LED, GET_LED_INFO and GET_LED_MODE_INFO.

| Value | Since | Mode |
|-------|-------|------|
| 0x80 | 0.1.3 | Flame |
