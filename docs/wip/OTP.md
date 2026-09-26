# OTP

Describes One ROM's use of RP2350 OTP memory.

OTP row numbers and field names are from the RP2350 datasheet, chapter 13.

OTP is 4096 rows. A row normally holds either 16 bits of data protected by ECC,
or 24 raw bits of unprotected data. A bit can be set (to 1) but never cleared.

## Layout

Raspberry Pi reserves OTP pages 0, 1 and 61–63 (rows `0x000`–`0x07f` and
`0xf40`–`0xfff`) for chip data and bootloader settings. Page 2 (rows
`0x080`–`0x0bf`) is for secure boot key hashes. Pages 3–60 (rows
`0x0c0`–`0xf3f`) are free for users. One ROM writes some bootloader settings
and keeps its own data in the free pages.

One ROM uses these rows.

| Rows | Contents |
| --- | --- |
| `0x048`–`0x04a` | `BOOT_FLAGS0` and its two copies on non-M boards |
| `0x054` | `FLASH_DEVINFO` on non-M boards |
| `0x059`–`0x05c` | `USB_BOOT_FLAGS`, its two copies and `USB_WHITE_LABEL_ADDR` |
| `0x0c0`–`0x4bf` | Commissioning area |
| `0x4c0`–`0xebd` | General store |
| `0xebe`–`0xebf` | General store terminator |
| `0xec0`–`0xf3f` | Bootloader USB strings |
| `0xf86`–`0xfa5` | Lock words for the commissioning area's pages |

## One ROM OTP Store

The One ROM OTP Store holds One ROM's own data in two areas:
- The commissioning area holds the values written when a board is commissioned.
- The general store holds values added later.

Both areas hold lists of entries, each a key and a value. Each entry is a key
row, then a length row, then the value. The length is the value's size in bytes
and is at least 1. The value takes two bytes per row, low byte first. A value
of odd length is padded with a zero byte. A string value has no terminator
because the entry's length gives its size.

- Key 0 with length 0 ends a list. An unwritten row reads as 0.
- Key 0 with any other length marks a deleted entry. See
  [Correcting Errors](#correcting-errors).
- If a key appears more than once in a list the last entry is used.

Each key is declared in `rust/metadata/metadata_schema.toml`, which generates
its C and Rust definitions. A key keeps its number permanently.

A key can be deprecated in the schema. Its number stays reserved. Parsers keep
reading it because commissioned boards still hold it.

A parser skips an unknown key using its length. A host tool reports each key it
skips.

## Commissioning Store Keys

| Key | Name | Value |
| --- | --- | --- |
| 1 | `COMMISSIONING_BOARD` | Board name matching firmware metadata's `hw_rev` |
| 2 | `COMMISSIONING_SIG` | 64-byte Ed25519 signature |
| 3 | `COMMISSIONING_MANUFACTURER` | Manufacturer's name |
| 4 | `COMMISSIONING_DATE` | UTC commissioning date as 8 ASCII digits, `YYYYMMDD` |
| 5 | `COMMISSIONING_SIGNER` | 16-bit ID of the signing key |

All of these keys are required in every commissioning instance.

## General Store Keys

Currently none.

### Commissioning Area

The commissioning area is pages 3–18, rows `0x0c0`–`0x4bf`. It holds one or more
commissioning instances. Each starts on a page boundary and consists of 1 or
more pages.

A commissioning instance starts with the following rows:
- Magic value `0x524f` ("OR")
- The format version, which is currently 1.

Its entries follow. `COMMISSIONING_SIG` is always the last
entry so a parser finds a commissioning instance's end by reading its entries.
The next commissioning instance, if present, starts at the following page
boundary.

A commissioning instance without `COMMISSIONING_SIG` is ignored. Its entries
end at key 0 with length 0, and the next instance starts at the page boundary
after those two rows. Its entries also end at the end of the commissioning
area, as an instance interrupted in the area's last page may leave no room
for key 0 and length 0. An instance whose version row is unwritten ends at its
version row.

Every page of a commissioning instance is locked once written. See
[Locking](#locking).

The last complete commissioning instance is the current one. Earlier
commissioning instances stay in place as a record, as they are locked.

If a parser finds a commissioning instance with an unknown version it stops
processing commissioning data and reports no valid commissioning data. It is
likely the commissioning instance was written by more recent tooling and the
current tooling cannot properly interpret any data in the commissioning
area.

A parser may fail to parse commissioning data when:
- an entry's length runs past the end of the commissioning area
- the page boundary after a commissioning instance holds neither the magic
  value nor 0

If this happens, it checks each following page boundary and restarts parsing
at the first holding the magic value as the next commissioning instance.
If it doesn't find any, it reports no valid commissioning data.

Firmware without a valid commissioning instance falls back to metadata.

### General Store

The general store starts at row `0x4c0` with the magic value and format version.
Its entries follow. Rows `0xebe` and `0xebf` stay unwritten so a full general
store always ends with key 0 and length 0.

A tool starting the general store writes the magic row, then the version row.
An unwritten version row reads as 0. Version 0 is not valid so a general store
interrupted between the two reads as absent.

A parser doesn't read a general store with an unknown version. A host tool
reports the version.

## Manufacturer Signature

`COMMISSIONING_SIG` shows who signed a commissioning instance. The signer and the
manufacturer can differ. piers.rocks can sign boards it white-labels for another
manufacturer. That manufacturer's name goes in `COMMISSIONING_MANUFACTURER`.

Every RP2350 holds a unique 64-bit ID called CHIPID in rows `0x000`–`0x003`.
Raspberry Pi writes and locks it when the chip is made.

`COMMISSIONING_SIG` holds an Ed25519 signature over one message. The message
comprises the following in order with no separator:
- the ASCII bytes of `onerom-commissioning-sig-v1`
- the 8 bytes of CHIPID starting with the low byte of row `0x000`
- the commissioning instance's rows from its magic row to the row before the
  signature's key row including any null padding byte in the last row.  Each row
  provides two bytes, low byte first.

A valid signature shows that the commissioning instance's values were written
together on that chip, by the organisation that owns the signing key. Each
RP2350 has a unique CHIPID so each One ROM has its own unique signature.

A host verifies the signature with One ROM stopped in the One ROM Bootloader.
It reads CHIPID and the store from the bootloader. The bootloader is stored in
the RP2350's mask ROM so it cannot be modified to report another chip's CHIPID.

The signature catches a simple copy of the board but not a copy that imitates
the bootloader. A simple copy runs the real bootloader and so reports its own
CHIPID. No manufacturer has signed that CHIPID so verification fails. An
imitation is the copy's own software behaving like the bootloader over USB. It
can report a CHIPID and signature copied from a genuine board. A host cannot
tell it from the real bootloader.

The CLI and other host tools share a library that holds a table of known
signing keys. Each key in the table is assigned a unique ID.
`COMMISSIONING_SIGNER` holds the ID of the key used to sign the commissioning
instance.

A host verifies the signature only with the key identified by
`COMMISSIONING_SIGNER`. It reports an ID missing from the table as an unknown
signer.

### Signing Keys

A signer's key is added to the table by a pull request. The pull request
provides:
- the key's ID
- the signer's name
- the public key
- a proof

The proof is the signer's signature over the ASCII bytes of `onerom-signer-v1`
followed by the signer's name.

`onerom hardware commission` refuses a signing key that isn't in the table.

ID 0 is invalid. IDs 1–255 are reserved for piers.rocks. Other signers' keys
are assigned IDs from 256 onwards.

CI rejects the row if:
- the proof fails to verify
- the key or the ID is already in the table
- the key is weak.

A weak key is one of Ed25519's small-order points.

### Retiring a Key

piers.rocks's signing server records every signature it makes before returning
it. The record is a public git repository with one file per signing key. Each
line holds a CHIPID and the SHA-256 hash of the signature. A hash is chosen so
that neither the signature nor the non-CHIPID values it covers are revealed.

A signature in OTP cannot be withdrawn. If a signing key or the server's PIN
leaks, the key stays in the table and is marked retired. `onerom hardware
validate` then accepts that key's signature only if its hash is in the key's
file. Genuine boards keep validating and signatures made with the leaked key
don't. A retired key's file no longer changes.

If there isn't a record file associated with a retired key, hardware
validation rejects every signature made with it.

## Bootloader USB Strings

The RP2350 bootloader's USB strings can be replaced using OTP. The table gives
each string's default and One ROM's value. The VID and PID are unchanged.

| String | Seen in | Default | One ROM | Rows |
| --- | --- | --- | --- | --- |
| USB manufacturer | OS device list | `Raspberry Pi` | `piers.rocks` | 6 |
| USB product | OS device list | `RP2350 Boot` | `One ROM Bootloader` | 9 |
| USB serial number | OS device list | CHIPID in hex | Unchanged | 0 |
| Volume label | USB drive name | `RP2350` | `ONEROM` | 3 |
| INDEX.HTM link | Web page on the USB drive | raspberrypi.com device page | `https://onerom.org` | 9 |
| INDEX.HTM link name | Web page on the USB drive | `raspberrypi.com` | `onerom.org` | 5 |
| INFO_UF2.TXT model | Text file on the USB drive | `Raspberry Pi RP2350` | `One ROM` | 4 |
| INFO_UF2.TXT board ID | Text file on the USB drive | `RP2350` | `COMMISSIONING_BOARD` value, e.g. `fire-24-f` | 5–7 |
| SCSI vendor, product and version | OS disk details | `RPI`, `RP2350` and a version | Unchanged | 0 |

Defaults are from datasheet section 5.7. A row holds two characters.

The replacement strings are listed in the USB white label table. The table
starts at row `0xec0` and the strings follow it. `USB_WHITE_LABEL_ADDR` holds
`0xec0`.

This table and its strings use pages 59 and 60. They don't share a page with the
store. The table above takes 16 of the 128 rows. These strings take 41–43 of the
remaining 112 depending on the board name length.

Strings left unchanged can be replaced later because each has its own valid
bit. There is room for all of them at their maximum lengths.

## Board Sizes

| Size | Built-in flash | External flash on chip select 1 |
| --- | --- | --- |
| M | 2MB | None |
| L | 2MB | 2MB |
| XL | 2MB | 16MB |

XL is currently reserved for future use and is not implemented at this time.

A non-M board's external flash is configured in OTP. `FLASH_DEVINFO` gives
both chips' sizes, the GPIO used for chip select 1 and whether the chips support
the D8h block erase command. The `FLASH_DEVINFO_ENABLE` bit (bit 5) of
`BOOT_FLAGS0` tells the bootloader to use `FLASH_DEVINFO`.

A fire-40-a with a 2MB flash chip on chip select 1 is an L board.
Its `FLASH_DEVINFO` is `0x99af`.

## Firmware

Firmware reads the store through the unguarded ECC alias. This is a memory
window at `0x40130000` where row n appears as 16 bits at `0x40130000`+(2*n).
A damaged row returns bad data instead of a bus fault.

At boot, firmware compares `COMMISSIONING_BOARD` from the current commissioning
instance with firmware metadata's `hw_rev` before driving GPIOs. If they differ
it reboots into the bootloader.

`clk_ref` must be 25MHz or less while firmware reads OTP. After firmware boot
One ROM's `clk_ref` is currently 3MHz - the external 12MHz crystal divided by 4.

When `FLASH_DEVINFO_ENABLE` is set, firmware reads `FLASH_DEVINFO` to learn
whether a second flash chip is fitted.

## Host Tools

`onerom scan` and `onerom inspect` report the current commissioning instance.
`onerom hardware validate` checks every commissioning instance:
- That the commissioning data is included and valid.
- A signature is present, from a known source, and reports the signer.

When the flash doesn't hold One ROM firmware, the CLI takes the board type from
`COMMISSIONING_BOARD` in the current commissioning instance. The CLI refuses to
program an image built for another board unless an override option is enabled.

## Locking

The RP2350 can lock each OTP page to make it read-only or unreadable. A page is
64 rows.

Before launching any plugin, firmware makes OTP pages 1–63 read-only for Secure
and Non-secure code in order to prevent a plugin from writing OTP. It also
stops OTP writes arriving through the USB plugin's PICOBOOT interface while One
ROM runs. Page 0 is already read-only from manufacture.

The firmware uses software lock registers `SW_LOCK1`–`SW_LOCK63`. A software
lock can be tightened but not loosened until the next reset.

A reset clears the software locks enabling OTP to be programmed via the
bootloader.

`onerom hardware commission` locks each page of a commissioning instance
read-only for Secure, Non-secure and bootloader access by writing the page's
lock word. This is permanent.

## Commissioning

`onerom hardware commission` programs a board's OTP with the commissioning
data. It requires One ROM to be stopped in the One ROM Bootloader and writes
through the bootloader's PICOBOOT USB interface.

The board's name and size are always given on the command line so a non-M
board cannot be commissioned as M by omitting its size.

Before writing anything it checks that every ECC row it will write is unwritten
or already holds the value it would write. It skips a row that already holds
its value. An interrupted commission can then be run again and a board whose
`FLASH_DEVINFO` was written earlier can still be commissioned. It reads back
each row as soon as it writes it and stops at the first mismatch.

1. It reads CHIPID from One ROM and looks up the signing key's ID in the table.
   It builds the commissioning instance's rows and gets their signature. It
   verifies the signature.
2. It displays every row it will write and asks for confirmation. It refuses a
   commissioning instance that would overlap row `0x4c0`.
3. On a non-M board it writes `FLASH_DEVINFO`.
4. It writes the commissioning instance's magic row, then its version row, then
   each entry with `COMMISSIONING_SIG` last. Each entry is written length row
   first, then its value, then its key row.
5. It locks the commissioning instance's pages.
6. It writes the bootloader USB strings, then the white label table, then
   `USB_WHITE_LABEL_ADDR`.
7. It sets the white label valid bits in `USB_BOOT_FLAGS` and its two copies.
8. On a non-M board it checks that row `0x055` holds a valid ECC value and then
   sets `FLASH_DEVINFO_ENABLE` in `BOOT_FLAGS0` and its two copies.

piers.rocks signs through a signing server that holds its private key and
requires a PIN. The CLI fetches the matching public key from the server. Other
manufacturers sign with their own private key file or signing server.

Row `0x055` is `FLASH_PARTITION_SLOT_SIZE`. One ROM does not write it. Once
`FLASH_DEVINFO_ENABLE` is set the boot path reads it together with
`FLASH_DEVINFO`. If it is not a valid ECC value the chip never boots again.

On a board that already has a commissioning instance it checks that a new one
should be supplied, and if so writes the new one at the next page boundary
after the last. If it cannot parse existing data, it writes the new instance
at the first page boundary after the last written row.

It refuses to commission a board whose commissioning area holds an unknown
version or whose current commissioning instance holds an unknown key. It is
likely that it was written by a newer version of the tool.

As an entry's key row is written last an entry interrupted before its key row
reads as a deleted entry and the rest of the store stays readable.

Commissioning writes the bootloader's enable and valid bits last because they
cannot be cleared. It writes each one only after reading back the rows it
enables.

## Correcting Errors

An existing commissioning instance cannot be changed because its pages are locked.
However, a board can be commissioned again. The new commissioning instance
becomes current and the old one remains as a permanent record.

A wrong general store value can be corrected by appending a new entry with the
same key.

An unwanted general store entry is deleted by setting its key row to all ones
with a raw write. The hardware inverts a raw row whose bits 23:22 are both set
before checking its ECC, so the row reads back as a valid 0.

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
