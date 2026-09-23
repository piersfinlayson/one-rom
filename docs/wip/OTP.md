# OTP

Describes One ROM's use of RP2350 OTP memory.

OTP row numbers and field names are from the RP2350 datasheet, chapter 13.

OTP is 4096 rows. A row normally holds either 16 bits of data protected by ECC,
or 24 raw bits of unprotected data. A bit can be set (to 1) but never cleared,
and an ECC row can be written only once.

## Layout

Raspberry Pi reserves OTP pages 0, 1 and 61–63 (rows `0x000`–`0x07f` and
`0xf40`–`0xfff`) for chip data and bootloader settings. Page 2 (rows
`0x080`–`0x0bf`) is for secure boot key hashes. Pages 3–60 (rows
`0x0c0`–`0xf3f`) are free for users. One ROM writes some bootloader settings
and keeps its own data in the free pages.

One ROM writes these rows.

| Rows | Contents |
| --- | --- |
| `0x048`–`0x04a` | `BOOT_FLAGS0` and its two copies on XL boards |
| `0x054` | `FLASH_DEVINFO` on XL boards |
| `0x059`–`0x05c` | `USB_BOOT_FLAGS`, its two copies and `USB_WHITE_LABEL_ADDR` |
| `0x0c0`–`0xefd` | One ROM OTP Store |
| `0xf00`–`0xf3f` | Bootloader USB strings |

## One ROM OTP Store

The One ROM OTP Store holds One ROM's own data as a list of entries, each a key
and a value.

Row `0x0c0` holds the magic value `0x524f` ("OR"). Row `0x0c1` holds the format
version, which is 1. Entries follow from row `0x0c2`.

Each entry is a key row, then a length row, then the value. The length is the
value's size in bytes and is at least 1. The value takes two bytes per row, low
byte first. A value of odd length is padded with a zero byte. A string value
has no terminator because the entry's length gives its size.

- Key 0 with length 0 ends the store. An unwritten row reads as 0. Rows `0xefe`
  and `0xeff` stay unwritten so a full store still ends with key 0 and length 0.
- Key 0 with any other length marks a deleted entry. See
  [Correcting Errors](#correcting-errors).
- If a key appears more than once the last entry is used.

Each key is declared in `rust/metadata/metadata_schema.toml`, which generates
its C and Rust definitions. A key keeps its number permanently.

A key can be deprecated in the schema. Its number stays reserved. Parsers keep
reading it because commissioned boards still hold it.

A parser skips an unknown key using its length. A host tool reports each key it
skips.

| Key | Name | Value |
| --- | --- | --- |
| 1 | `BOARD` | Board name matching firmware metadata's `hw_rev` |
| 2 | `MANUFACTURER_SIG` | 64-byte Ed25519 signature |

Firmware that doesn't find a magic value at row `0x0c0` doesn't use the store.
It uses firmware metadata instead.

## Manufacturer Signature

`MANUFACTURER_SIG` identifies the board's manufacturer.

Every RP2350 holds a unique 64-bit ID called CHIPID in rows `0x000`–`0x003`.
Raspberry Pi writes and locks it when the chip is made.

`MANUFACTURER_SIG` holds an Ed25519 signature over a 34-byte message. The
message is the 26 ASCII bytes of `onerom-manufacturer-sig-v1` followed by the 8
bytes of CHIPID, starting with the low byte of row `0x000`.

Each RP2350 has a unique CHIPID so each One ROM has its own unique signature.

A host verifies the signature with One ROM stopped in the One ROM Bootloader.
It reads CHIPID and the store from the bootloader. The bootloader is stored in
the RP2350's mask ROM so it cannot be modified to report another chip's CHIPID.

A host holds public keys for one or more manufacturers and uses these to identify
the manufacturer.

## Bootloader USB Strings

The RP2350 bootloader's USB strings can be replaced using OTP. The table gives
each string's default and One ROM's value. The VID and PID are unchanged.

| String | Seen in | Default | One ROM | Rows |
| --- | --- | --- | --- | --- |
| USB manufacturer | OS device list | `Raspberry Pi` | Manufacturer's name, e.g. `piers.rocks` | 6 |
| USB product | OS device list | `RP2350 Boot` | `One ROM Bootloader` | 9 |
| USB serial number | OS device list | CHIPID in hex | Unchanged | 0 |
| Volume label | USB drive name | `RP2350` | `ONEROM` | 3 |
| INDEX.HTM link | Web page on the USB drive | raspberrypi.com device page | `https://onerom.org` | 9 |
| INDEX.HTM link name | Web page on the USB drive | `raspberrypi.com` | `onerom.org` | 5 |
| INFO_UF2.TXT model | Text file on the USB drive | `Raspberry Pi RP2350` | `One ROM` | 4 |
| INFO_UF2.TXT board ID | Text file on the USB drive | `RP2350` | `BOARD` value, e.g. `fire-24-f` | 5–7 |
| SCSI vendor, product and version | OS disk details | `RPI`, `RP2350` and a version | Unchanged | 0 |

Defaults are from datasheet section 5.7. A row holds two characters.

The replacement strings are listed in the USB white label table. The table
starts at row `0xf00` and the strings follow it. `USB_WHITE_LABEL_ADDR` holds
`0xf00`.

The white label table takes 16 of page 60's 64 rows. These strings take 41–43
of the remaining 48 depending on the board name. A manufacturer name of up to
22 characters fits.

A string left unchanged can be replaced later because each is controlled by its
own valid bit.

## XL Boards

An XL board has a second flash chip on chip select 1.

`FLASH_DEVINFO` gives both chips' sizes, the GPIO used for chip select 1 and
whether the chips support the D8h block erase command. The
`FLASH_DEVINFO_ENABLE` bit (bit 5) of `BOOT_FLAGS0` tells the bootloader to use
`FLASH_DEVINFO`.

On an XL board `FLASH_DEVINFO` is set to indicate both primary and secondary
flash sizes. A fire-40-a is XL when built with its external flash chip on chip
select 1. Its `FLASH_DEVINFO` is `0x99af`.

## Firmware

Firmware reads the store through the unguarded ECC alias. This is a memory
window at `0x40130000` where row n appears as 16 bits at `0x40130000`+(2*n).
A damaged row returns bad data instead of a bus fault.

`clk_ref` must be 25MHz or less while firmware reads OTP. After firmware boot
One ROM's `clk_ref` is currently 3MHz - the external 12MHz crystal divided by 4.

When `FLASH_DEVINFO_ENABLE` is set, firmware reads `FLASH_DEVINFO` to learn
whether a second flash chip is fitted.

## Locking

The RP2350 can lock each OTP page to make it read-only or unreadable. A page is
64 rows.

Before launching any plugin, firmware makes OTP pages 1–63 read-only for Secure
and Non-secure code in order to prevent a plugin from writing OTP. Page 0 is
already read-only from manufacture.

The firmware uses software lock registers `SW_LOCK1`–`SW_LOCK63`. A software
lock can be tightened but not loosened until the next reset.

A reset clears the software locks enabling OTP to be programmed via the
bootloader.

## Commissioning

`onerom commission` programs a board's OTP. It requires One ROM to be stopped
in the One ROM Bootloader and writes through the bootloader's PICOBOOT USB
interface.

Before writing anything it checks that every ECC row it will write is
unwritten. It reads back each row as soon as it writes it and stops at the first
mismatch.

1. It reads CHIPID from One ROM and creates a signature.
2. It displays every row it will write and asks for confirmation. It refuses a
   store that would reach row `0xefe`.
3. On an XL board it writes `FLASH_DEVINFO`.
4. It writes the One ROM OTP Store version row, then its magic row, then each
   entry. Each entry is written length row first, then its value, then its key
   row.
5. It writes the bootloader USB strings, then the white label table, then
   `USB_WHITE_LABEL_ADDR`.
6. It sets the white label valid bits in `USB_BOOT_FLAGS` and its two copies.
7. On an XL board it sets `FLASH_DEVINFO_ENABLE` in `BOOT_FLAGS0` and its two
   copies.

On a board that already has a store it writes new entries after the last one.

The version row is written before the magic row. A store interrupted between
the two has no magic value and reads as absent.

As an entry's key row is written last an entry interrupted before its key row
reads as a deleted entry and the rest of the store stays readable.

Commissioning writes the bootloader's enable and valid bits last because they
cannot be cleared. It writes each one only after reading back the rows it
enables.

## Correcting Errors

A wrong store value can be corrected by appending a new entry with the same key.

An unwanted entry is deleted by setting its key row to all ones with a raw
write. The hardware inverts a raw row whose bits 23:22 are both set before
checking its ECC, so the row reads back as a valid 0.

Once `FLASH_DEVINFO_ENABLE` is set an incorrect `FLASH_DEVINFO` is permanent.
Only replacing the RP2350 fixes it.

- An incorrect primary flash size can stop the board booting or being
  programmed.
- An incorrect secondary flash size or GPIO stops the host programming the
  second chip through the bootloader. Firmware can still use the chip by
  overwriting the bootrom's RAM copy of `FLASH_DEVINFO`.

Blanking the row sets both sizes to zero and the board no longer boots. Before
the enable bit is set an incorrect row does no harm, but that board can never
describe its second chip in OTP. That is why the enable bit is written last,
after the check.

An incorrect bootloader string reverts to the bootloader's default when its
white label table row is set to all ones with a raw write. It cannot be
replaced. Setting `USB_WHITE_LABEL_ADDR` to all ones reverts every bootloader
string.

## To Test

Open items requiring testing before implementation.

- An all-ones raw write to a written ECC row makes it read back as a valid 0.

## Open Issues

- How firmware and hosts use `BOARD`, including what firmware does when it
  differs from firmware metadata's `hw_rev`.
