# One ROM Chip Compatibility — firmware v0.7.1

This document shows which chips each One ROM Fire hardware variant emulates.

**ROM size** is the chip's actual storage capacity.

**Image size** is the space used on One ROM device's internal flash to emulate that chip. This can be larger than the original ROM itself, due to the way One ROM works.

One ROM typically has a 2MB flash, with 64KB reserved for the firmware and metadata. The remainder is available for ROM images. The total number of images that can be supported is based on the image size of each ROM.

Some lower pin count ROMs can be emulated by larger One ROMs, by inserting the larger One ROM in the smaller socket, with the top pins (1, 2, ...) hanging out (and it is not necessary to solder these pins to One ROM if using like this). If doing this, it is _extremely_ important that power is rerouted to One ROM's VCC pin, or to the 5V header pin, or One ROM may be damaged.

Some greater pin count ROMs can be emulated by a smaller One ROM, provided the ROM's extra address pins fall on socket positions that One ROM does not use. In this case, the smaller One ROM gets installed to the bottom of the larger socket, with the top pins of the socket unpopulated. A short fly-lead must be run from each additional address pin of those socket pins to the X1 (and, if two are needed, X2) header pin on One ROM.

**Every image size below assumes One ROM monitors all of the chip's control lines** — every chip select, or /CE and /OE — which is what the tools produce unless told otherwise. A chip that One ROM can only serve with one of those lines left unmonitored is shown as unsupported here, because doing that requires the `allow_cs_ignore` config option and cannot be expressed on the `onerom` command line at all.

| Cell | Meaning |
|:---|:---|
| `64KB` | Chip is supported on this board; shows the image size |
| `64KB*` | Supported with One ROM overhanging the socket (top pins exposed — power reroute required) |
| `64KB†` | Supported with fly-lead wire(s) from the chip socket's address pin(s) to One ROM's X1 (and X2) header pin |
| `-` | Not supported on this board |

## 24-pin boards

*24-pin chips (native)*

| Chip | ROM size | 24A | 24B | 24C | 24D | 24E | 24F |
|:---|---:|---:|---:|---:|---:|---:|---:|
| 2704 | 512B | 2KB | 2KB | 512B | 512B | 512B | 512B |
| HM7641 | 512B | 2KB | 2KB | 512B | 512B | 512B | 512B |
| 2708 | 1KB | 4KB | 4KB | 1KB | 1KB | 1KB | 1KB |
| 2316 | 2KB | 32KB | 32KB | 2KB | 2KB | 2KB | 2KB |
| 2716 | 2KB | 32KB | 32KB | 2KB | 2KB | 2KB | 2KB |
| 28C16 | 2KB | 32KB | 32KB | 2KB | 2KB | 2KB | 2KB |
| 9316 | 2KB | 32KB | 32KB | 2KB | 2KB | 2KB | 2KB |
| 9316A | 2KB | 32KB | 32KB | 2KB | 2KB | 2KB | 2KB |
| 2332 | 4KB | 64KB | 64KB | 8KB | 8KB | 8KB | 8KB |
| 2732 | 4KB | 32KB | 32KB | 4KB | 4KB | 4KB | 4KB |
| 27C32 | 4KB | 32KB | 32KB | 4KB | 4KB | 4KB | 4KB |
| 4732 | 4KB | 64KB | 64KB | 8KB | 8KB | 8KB | 8KB |
| 9332 | 4KB | 64KB | 64KB | 8KB | 8KB | 8KB | 8KB |
| 2364 | 8KB | 64KB | 64KB | 8KB | 8KB | 8KB | 8KB |
| 4764 | 8KB | 64KB | 64KB | 8KB | 8KB | 8KB | 8KB |
| MCM68364 | 8KB | 64KB | 64KB | 8KB | 8KB | 8KB | 8KB |
| MCM68764 | 8KB | 64KB | 64KB | 8KB | 8KB | 8KB | 8KB |
| MCM68A364 | 8KB | 64KB | 64KB | 8KB | 8KB | 8KB | 8KB |
| MCM68A764 | 8KB | 64KB | 64KB | 8KB | 8KB | 8KB | 8KB |
| MK36000 | 8KB | 64KB | 64KB | 8KB | 8KB | 8KB | 8KB |
| MM52164 | 8KB | 64KB | 64KB | 8KB | 8KB | 8KB | 8KB |

*28-pin chips (with fly-leads)*

| Chip | ROM size | 24A | 24B | 24C | 24D | 24E | 24F |
|:---|---:|---:|---:|---:|---:|---:|---:|
| 2764 | 8KB | 32KB† | 32KB† | 32KB† | 32KB† | 32KB† | 32KB† |
| 27C64 | 8KB | 32KB† | 32KB† | 32KB† | 32KB† | 32KB† | 32KB† |
| 27LC64 | 8KB | 32KB† | 32KB† | 32KB† | 32KB† | 32KB† | 32KB† |
| 28C64 | 8KB | 32KB† | 32KB† | 32KB† | 32KB† | 32KB† | 32KB† |

## 28-pin boards

*28-pin chips (native)*

| Chip | ROM size | 28A | 28B | 28C | 28D |
|:---|---:|---:|---:|---:|---:|
| 2764 | 8KB | 32KB | 32KB | 32KB | 32KB |
| 27C64 | 8KB | 32KB | 32KB | 32KB | 32KB |
| 27LC64 | 8KB | 32KB | 32KB | 32KB | 32KB |
| 28C64 | 8KB | 32KB | 32KB | 32KB | 32KB |
| 23128 | 16KB | 32KB | 32KB | 32KB | 32KB |
| 27128 | 16KB | 32KB | 32KB | 32KB | 32KB |
| 27C128 | 16KB | 32KB | 32KB | 32KB | 32KB |
| 27LC128 | 16KB | 32KB | 32KB | 32KB | 32KB |
| 23256 | 32KB | 64KB | 64KB | 64KB | 64KB |
| 27256 | 32KB | 64KB | 64KB | 64KB | 64KB |
| 27C256 | 32KB | 64KB | 64KB | 64KB | 64KB |
| 27LC256 | 32KB | 64KB | 64KB | 64KB | 64KB |
| 27SF256 | 32KB | 64KB | 64KB | 64KB | 64KB |
| 28C256 | 32KB | 32KB | 32KB | 32KB | 32KB |
| 23QL384 | 48KB | 128KB | 128KB | 256KB | 256KB |
| 23512 | 64KB | 64KB | 64KB | 64KB | 64KB |
| 23QL512 | 64KB | 128KB | 128KB | 256KB | 256KB |
| 27512 | 64KB | 64KB | 64KB | 64KB | 64KB |
| 27C512 | 64KB | 64KB | 64KB | 64KB | 64KB |
| 27LC512 | 64KB | 64KB | 64KB | 64KB | 64KB |
| 27SF512 | 64KB | 64KB | 64KB | 64KB | 64KB |
| 231024 | 128KB | 256KB | 256KB | 128KB | 128KB |
| 23C1000 | 128KB | 256KB | 256KB | 128KB | 128KB |
| 23C1000A | 128KB | 256KB | 256KB | 128KB | 128KB |
| MX23C1000 | 128KB | 256KB | 256KB | 128KB | 128KB |
| TC531000 | 128KB | 256KB | 256KB | 128KB | 128KB |

*24-pin chips (with overhang)*

| Chip | ROM size | 28A | 28B | 28C | 28D |
|:---|---:|---:|---:|---:|---:|
| 2704 | 512B | 4KB* | 4KB* | 4KB* | 4KB* |
| HM7641 | 512B | - | - | 4KB* | 4KB* |
| 2708 | 1KB | 16KB* | 16KB* | 8KB* | 8KB* |
| 2716 | 2KB | 32KB* | 32KB* | 32KB* | 32KB* |
| 28C16 | 2KB | 32KB* | 32KB* | 32KB* | 32KB* |
| 2732 | 4KB | 32KB* | 32KB* | 32KB* | 32KB* |
| 27C32 | 4KB | 32KB* | 32KB* | 32KB* | 32KB* |
| 2364 | 8KB | 128KB* | 128KB* | 256KB* | 256KB* |
| 4764 | 8KB | 128KB* | 128KB* | 256KB* | 256KB* |
| MCM68364 | 8KB | 128KB* | 128KB* | 256KB* | 256KB* |
| MCM68764 | 8KB | 128KB* | 128KB* | 256KB* | 256KB* |
| MCM68A364 | 8KB | 128KB* | 128KB* | 256KB* | 256KB* |
| MCM68A764 | 8KB | 128KB* | 128KB* | 256KB* | 256KB* |
| MK36000 | 8KB | 128KB* | 128KB* | 256KB* | 256KB* |
| MM52164 | 8KB | 128KB* | 128KB* | 256KB* | 256KB* |

*32-pin chips (with fly-leads)*

| Chip | ROM size | 28A | 28B | 28C | 28D |
|:---|---:|---:|---:|---:|---:|
| 28C512 | 64KB | 64KB† | 64KB† | 64KB† | 64KB† |
| 23C1010 | 128KB | - | - | 128KB† | 128KB† |
| 27C010 | 128KB | - | - | 128KB† | 128KB† |
| 27C1000A | 128KB | - | - | 128KB† | 128KB† |
| 27C1001 | 128KB | - | - | 128KB† | 128KB† |
| 29F010 | 128KB | - | - | 128KB† | 128KB† |
| 39SF010 | 128KB | - | - | 128KB† | 128KB† |
| SST39SF010 | 128KB | - | - | 128KB† | 128KB† |

## 32-pin boards

*32-pin chips (native)*

| Chip | ROM size | 32A | 32B |
|:---|---:|---:|---:|
| 28C512 | 64KB | 256KB | 64KB |
| 23C1001 | 128KB | - | 128KB |
| 23C1010 | 128KB | 512KB | 128KB |
| 27C010 | 128KB | 512KB | 128KB |
| 27C100 | 128KB | 512KB | 256KB |
| 27C1000 | 128KB | 512KB | 256KB |
| 27C1000A | 128KB | 512KB | 128KB |
| 27C1001 | 128KB | 512KB | 128KB |
| 27C301 | 128KB | 512KB | 256KB |
| 29F010 | 128KB | 512KB | 128KB |
| 39SF010 | 128KB | 512KB | 128KB |
| D23C1001 | 128KB | - | 128KB |
| SST39SF010 | 128KB | 512KB | 128KB |
| 27C020 | 256KB | 512KB | 256KB |
| 27C2001 | 256KB | 512KB | 256KB |
| 29F020 | 256KB | 512KB | 256KB |
| 39SF020 | 256KB | 512KB | 256KB |
| SST39SF020 | 256KB | 512KB | 256KB |
| 27C040 | 512KB | 512KB | 512KB |
| 27C4001 | 512KB | 512KB | 512KB |
| 29F040 | 512KB | - | 512KB |
| 39SF040 | 512KB | - | 512KB |
| SST39SF040 | 512KB | - | 512KB |
| 27C080 | 1MB | 512KB | 512KB |
| 27C801 | 1MB | 512KB | 512KB |

*28-pin chips (with overhang)*

| Chip | ROM size | 32A | 32B |
|:---|---:|---:|---:|
| 2764 | 8KB | 256KB* | 32KB* |
| 27C64 | 8KB | 256KB* | 32KB* |
| 27LC64 | 8KB | 256KB* | 32KB* |
| 28C64 | 8KB | 256KB* | 32KB* |
| 27128 | 16KB | 256KB* | 32KB* |
| 27C128 | 16KB | 256KB* | 32KB* |
| 27LC128 | 16KB | 256KB* | 32KB* |
| 23256 | 32KB | 256KB* | 32KB* |
| 27256 | 32KB | 256KB* | 32KB* |
| 27C256 | 32KB | 256KB* | 32KB* |
| 27LC256 | 32KB | 256KB* | 32KB* |
| 27SF256 | 32KB | 256KB* | 32KB* |
| 28C256 | 32KB | 256KB* | 64KB* |
| 23512 | 64KB | 256KB* | 64KB* |
| 27512 | 64KB | 256KB* | 64KB* |
| 27C512 | 64KB | 256KB* | 64KB* |
| 27LC512 | 64KB | 256KB* | 64KB* |
| 27SF512 | 64KB | 256KB* | 64KB* |
| 231024 | 128KB | 512KB* | 256KB* |
| 23C1000 | 128KB | 512KB* | 256KB* |
| 23C1000A | 128KB | 512KB* | 256KB* |
| MX23C1000 | 128KB | 512KB* | 256KB* |
| TC531000 | 128KB | 512KB* | 256KB* |

*24-pin chips (with overhang)*

| Chip | ROM size | 32A | 32B |
|:---|---:|---:|---:|
| 2704 | 512B | 32KB* | 4KB* |
| HM7641 | 512B | 32KB* | - |
| 2708 | 1KB | 64KB* | 8KB* |
| 2716 | 2KB | 256KB* | 32KB* |
| 28C16 | 2KB | 256KB* | 32KB* |
| 2732 | 4KB | 256KB* | 32KB* |
| 27C32 | 4KB | 256KB* | 32KB* |

## 40-pin boards

| Chip | ROM size | 40A | 40B |
|:---|---:|---:|---:|
| 27C200 | 256KB | 512KB | 256KB |
| HN62402 | 256KB | 512KB | 256KB |
| 23C4100 | 512KB | 512KB | 512KB |
| 27C400 | 512KB | 512KB | 512KB |
| 27C4100 | 512KB | 512KB | 512KB |
| AT27C400 | 512KB | 512KB | 512KB |
| HN62404 | 512KB | 512KB | 512KB |
| HN62424 | 512KB | 512KB | 512KB |
| M27C400 | 512KB | 512KB | 512KB |
| MB834200 | 512KB | 512KB | 512KB |
| MX23C4100 | 512KB | 512KB | 512KB |
| MX27C4100 | 512KB | 512KB | 512KB |
| TCS534200 | 512KB | 512KB | 512KB |

---

# Per-board details

Full chip list for each board. Where a particular ROM type goes by multiple identifiers (for example 27512, 27C512, 27SF512), each type appears as a separate row.

The **Fit** column says how the chip sits in the board's socket:

| Fit | Meaning |
|:---|:---|
| `native` | Chip and board have the same pin count — it goes straight in |
| `overhang` | Chip has *fewer* pins than the board, so One ROM's top pins hang out of the socket |
| `larger socket (no fly-leads)` | Chip has *more* pins than the board, but no address line among the extra ones: One ROM sits in the bottom of the socket with nothing to wire |
| `fly-lead to X1` (and `X2`) | Chip has more pins than the board, and the overhanging address line(s) must be wired to One ROM's X1 (and X2) header pin |

Every fit other than `native` is a cross-size fit, and in all of them One ROM's power pins may not line up with the socket's — power must be rerouted to One ROM's own VCC or 5V header pin. `larger socket (no fly-leads)` means no *signal* wiring is needed; it does not mean the chip simply drops in.

## One ROM Fire 24 (rev A/A2) — fire-24-a

*24-pin chips (native)*

| Chip | ROM size | Image size | Fit |
|:---|---:|---:|:---|
| 2704 | 512B | 2KB | native |
| HM7641 | 512B | 2KB | native |
| 2708 | 1KB | 4KB | native |
| 2316 | 2KB | 32KB | native |
| 2716 | 2KB | 32KB | native |
| 28C16 | 2KB | 32KB | native |
| 9316 | 2KB | 32KB | native |
| 9316A | 2KB | 32KB | native |
| 2332 | 4KB | 64KB | native |
| 2732 | 4KB | 32KB | native |
| 27C32 | 4KB | 32KB | native |
| 4732 | 4KB | 64KB | native |
| 9332 | 4KB | 64KB | native |
| 2364 | 8KB | 64KB | native |
| 4764 | 8KB | 64KB | native |
| MCM68364 | 8KB | 64KB | native |
| MCM68764 | 8KB | 64KB | native |
| MCM68A364 | 8KB | 64KB | native |
| MCM68A764 | 8KB | 64KB | native |
| MK36000 | 8KB | 64KB | native |
| MM52164 | 8KB | 64KB | native |

*28-pin chips (with fly-leads)*

| Chip | ROM size | Image size | Fit |
|:---|---:|---:|:---|
| 2764 | 8KB | 32KB | fly-lead to X1 |
| 27C64 | 8KB | 32KB | fly-lead to X1 |
| 27LC64 | 8KB | 32KB | fly-lead to X1 |
| 28C64 | 8KB | 32KB | fly-lead to X1 |

## One ROM Fire 24 (rev B) — fire-24-usb-b

*24-pin chips (native)*

| Chip | ROM size | Image size | Fit |
|:---|---:|---:|:---|
| 2704 | 512B | 2KB | native |
| HM7641 | 512B | 2KB | native |
| 2708 | 1KB | 4KB | native |
| 2316 | 2KB | 32KB | native |
| 2716 | 2KB | 32KB | native |
| 28C16 | 2KB | 32KB | native |
| 9316 | 2KB | 32KB | native |
| 9316A | 2KB | 32KB | native |
| 2332 | 4KB | 64KB | native |
| 2732 | 4KB | 32KB | native |
| 27C32 | 4KB | 32KB | native |
| 4732 | 4KB | 64KB | native |
| 9332 | 4KB | 64KB | native |
| 2364 | 8KB | 64KB | native |
| 4764 | 8KB | 64KB | native |
| MCM68364 | 8KB | 64KB | native |
| MCM68764 | 8KB | 64KB | native |
| MCM68A364 | 8KB | 64KB | native |
| MCM68A764 | 8KB | 64KB | native |
| MK36000 | 8KB | 64KB | native |
| MM52164 | 8KB | 64KB | native |

*28-pin chips (with fly-leads)*

| Chip | ROM size | Image size | Fit |
|:---|---:|---:|:---|
| 2764 | 8KB | 32KB | fly-lead to X1 |
| 27C64 | 8KB | 32KB | fly-lead to X1 |
| 27LC64 | 8KB | 32KB | fly-lead to X1 |
| 28C64 | 8KB | 32KB | fly-lead to X1 |

## One ROM Fire 24 (rev C) — fire-24-c

*24-pin chips (native)*

| Chip | ROM size | Image size | Fit |
|:---|---:|---:|:---|
| 2704 | 512B | 512B | native |
| HM7641 | 512B | 512B | native |
| 2708 | 1KB | 1KB | native |
| 2316 | 2KB | 2KB | native |
| 2716 | 2KB | 2KB | native |
| 28C16 | 2KB | 2KB | native |
| 9316 | 2KB | 2KB | native |
| 9316A | 2KB | 2KB | native |
| 2332 | 4KB | 8KB | native |
| 2732 | 4KB | 4KB | native |
| 27C32 | 4KB | 4KB | native |
| 4732 | 4KB | 8KB | native |
| 9332 | 4KB | 8KB | native |
| 2364 | 8KB | 8KB | native |
| 4764 | 8KB | 8KB | native |
| MCM68364 | 8KB | 8KB | native |
| MCM68764 | 8KB | 8KB | native |
| MCM68A364 | 8KB | 8KB | native |
| MCM68A764 | 8KB | 8KB | native |
| MK36000 | 8KB | 8KB | native |
| MM52164 | 8KB | 8KB | native |

*28-pin chips (with fly-leads)*

| Chip | ROM size | Image size | Fit |
|:---|---:|---:|:---|
| 2764 | 8KB | 32KB | fly-lead to X1 |
| 27C64 | 8KB | 32KB | fly-lead to X1 |
| 27LC64 | 8KB | 32KB | fly-lead to X1 |
| 28C64 | 8KB | 32KB | fly-lead to X1 |

## One ROM Fire 24 (rev D) — fire-24-d

*24-pin chips (native)*

| Chip | ROM size | Image size | Fit |
|:---|---:|---:|:---|
| 2704 | 512B | 512B | native |
| HM7641 | 512B | 512B | native |
| 2708 | 1KB | 1KB | native |
| 2316 | 2KB | 2KB | native |
| 2716 | 2KB | 2KB | native |
| 28C16 | 2KB | 2KB | native |
| 9316 | 2KB | 2KB | native |
| 9316A | 2KB | 2KB | native |
| 2332 | 4KB | 8KB | native |
| 2732 | 4KB | 4KB | native |
| 27C32 | 4KB | 4KB | native |
| 4732 | 4KB | 8KB | native |
| 9332 | 4KB | 8KB | native |
| 2364 | 8KB | 8KB | native |
| 4764 | 8KB | 8KB | native |
| MCM68364 | 8KB | 8KB | native |
| MCM68764 | 8KB | 8KB | native |
| MCM68A364 | 8KB | 8KB | native |
| MCM68A764 | 8KB | 8KB | native |
| MK36000 | 8KB | 8KB | native |
| MM52164 | 8KB | 8KB | native |

*28-pin chips (with fly-leads)*

| Chip | ROM size | Image size | Fit |
|:---|---:|---:|:---|
| 2764 | 8KB | 32KB | fly-lead to X1 |
| 27C64 | 8KB | 32KB | fly-lead to X1 |
| 27LC64 | 8KB | 32KB | fly-lead to X1 |
| 28C64 | 8KB | 32KB | fly-lead to X1 |

## One ROM Fire 24 (rev E) — fire-24-e

*24-pin chips (native)*

| Chip | ROM size | Image size | Fit |
|:---|---:|---:|:---|
| 2704 | 512B | 512B | native |
| HM7641 | 512B | 512B | native |
| 2708 | 1KB | 1KB | native |
| 2316 | 2KB | 2KB | native |
| 2716 | 2KB | 2KB | native |
| 28C16 | 2KB | 2KB | native |
| 9316 | 2KB | 2KB | native |
| 9316A | 2KB | 2KB | native |
| 2332 | 4KB | 8KB | native |
| 2732 | 4KB | 4KB | native |
| 27C32 | 4KB | 4KB | native |
| 4732 | 4KB | 8KB | native |
| 9332 | 4KB | 8KB | native |
| 2364 | 8KB | 8KB | native |
| 4764 | 8KB | 8KB | native |
| MCM68364 | 8KB | 8KB | native |
| MCM68764 | 8KB | 8KB | native |
| MCM68A364 | 8KB | 8KB | native |
| MCM68A764 | 8KB | 8KB | native |
| MK36000 | 8KB | 8KB | native |
| MM52164 | 8KB | 8KB | native |

*28-pin chips (with fly-leads)*

| Chip | ROM size | Image size | Fit |
|:---|---:|---:|:---|
| 2764 | 8KB | 32KB | fly-lead to X1 |
| 27C64 | 8KB | 32KB | fly-lead to X1 |
| 27LC64 | 8KB | 32KB | fly-lead to X1 |
| 28C64 | 8KB | 32KB | fly-lead to X1 |

## One ROM Fire 24 (rev F) — fire-24-f

*24-pin chips (native)*

| Chip | ROM size | Image size | Fit |
|:---|---:|---:|:---|
| 2704 | 512B | 512B | native |
| HM7641 | 512B | 512B | native |
| 2708 | 1KB | 1KB | native |
| 2316 | 2KB | 2KB | native |
| 2716 | 2KB | 2KB | native |
| 28C16 | 2KB | 2KB | native |
| 9316 | 2KB | 2KB | native |
| 9316A | 2KB | 2KB | native |
| 2332 | 4KB | 8KB | native |
| 2732 | 4KB | 4KB | native |
| 27C32 | 4KB | 4KB | native |
| 4732 | 4KB | 8KB | native |
| 9332 | 4KB | 8KB | native |
| 2364 | 8KB | 8KB | native |
| 4764 | 8KB | 8KB | native |
| MCM68364 | 8KB | 8KB | native |
| MCM68764 | 8KB | 8KB | native |
| MCM68A364 | 8KB | 8KB | native |
| MCM68A764 | 8KB | 8KB | native |
| MK36000 | 8KB | 8KB | native |
| MM52164 | 8KB | 8KB | native |

*28-pin chips (with fly-leads)*

| Chip | ROM size | Image size | Fit |
|:---|---:|---:|:---|
| 2764 | 8KB | 32KB | fly-lead to X1 |
| 27C64 | 8KB | 32KB | fly-lead to X1 |
| 27LC64 | 8KB | 32KB | fly-lead to X1 |
| 28C64 | 8KB | 32KB | fly-lead to X1 |

## One ROM Fire 28 (rev A/A2/A3/A4) — fire-28-a

*28-pin chips (native)*

| Chip | ROM size | Image size | Fit |
|:---|---:|---:|:---|
| 2764 | 8KB | 32KB | native |
| 27C64 | 8KB | 32KB | native |
| 27LC64 | 8KB | 32KB | native |
| 28C64 | 8KB | 32KB | native |
| 23128 | 16KB | 32KB | native |
| 27128 | 16KB | 32KB | native |
| 27C128 | 16KB | 32KB | native |
| 27LC128 | 16KB | 32KB | native |
| 23256 | 32KB | 64KB | native |
| 27256 | 32KB | 64KB | native |
| 27C256 | 32KB | 64KB | native |
| 27LC256 | 32KB | 64KB | native |
| 27SF256 | 32KB | 64KB | native |
| 28C256 | 32KB | 32KB | native |
| 23QL384 | 48KB | 128KB | native |
| 23512 | 64KB | 64KB | native |
| 23QL512 | 64KB | 128KB | native |
| 27512 | 64KB | 64KB | native |
| 27C512 | 64KB | 64KB | native |
| 27LC512 | 64KB | 64KB | native |
| 27SF512 | 64KB | 64KB | native |
| 231024 | 128KB | 256KB | native |
| 23C1000 | 128KB | 256KB | native |
| 23C1000A | 128KB | 256KB | native |
| MX23C1000 | 128KB | 256KB | native |
| TC531000 | 128KB | 256KB | native |

*24-pin chips (with overhang)*

| Chip | ROM size | Image size | Fit |
|:---|---:|---:|:---|
| 2704 | 512B | 4KB | overhang |
| 2708 | 1KB | 16KB | overhang |
| 2716 | 2KB | 32KB | overhang |
| 28C16 | 2KB | 32KB | overhang |
| 2732 | 4KB | 32KB | overhang |
| 27C32 | 4KB | 32KB | overhang |
| 2364 | 8KB | 128KB | overhang |
| 4764 | 8KB | 128KB | overhang |
| MCM68364 | 8KB | 128KB | overhang |
| MCM68764 | 8KB | 128KB | overhang |
| MCM68A364 | 8KB | 128KB | overhang |
| MCM68A764 | 8KB | 128KB | overhang |
| MK36000 | 8KB | 128KB | overhang |
| MM52164 | 8KB | 128KB | overhang |

*32-pin chips (with fly-leads)*

| Chip | ROM size | Image size | Fit |
|:---|---:|---:|:---|
| 28C512 | 64KB | 64KB | larger socket (no fly-leads) |

## One ROM Fire 28 (rev B) — fire-28-b

*28-pin chips (native)*

| Chip | ROM size | Image size | Fit |
|:---|---:|---:|:---|
| 2764 | 8KB | 32KB | native |
| 27C64 | 8KB | 32KB | native |
| 27LC64 | 8KB | 32KB | native |
| 28C64 | 8KB | 32KB | native |
| 23128 | 16KB | 32KB | native |
| 27128 | 16KB | 32KB | native |
| 27C128 | 16KB | 32KB | native |
| 27LC128 | 16KB | 32KB | native |
| 23256 | 32KB | 64KB | native |
| 27256 | 32KB | 64KB | native |
| 27C256 | 32KB | 64KB | native |
| 27LC256 | 32KB | 64KB | native |
| 27SF256 | 32KB | 64KB | native |
| 28C256 | 32KB | 32KB | native |
| 23QL384 | 48KB | 128KB | native |
| 23512 | 64KB | 64KB | native |
| 23QL512 | 64KB | 128KB | native |
| 27512 | 64KB | 64KB | native |
| 27C512 | 64KB | 64KB | native |
| 27LC512 | 64KB | 64KB | native |
| 27SF512 | 64KB | 64KB | native |
| 231024 | 128KB | 256KB | native |
| 23C1000 | 128KB | 256KB | native |
| 23C1000A | 128KB | 256KB | native |
| MX23C1000 | 128KB | 256KB | native |
| TC531000 | 128KB | 256KB | native |

*24-pin chips (with overhang)*

| Chip | ROM size | Image size | Fit |
|:---|---:|---:|:---|
| 2704 | 512B | 4KB | overhang |
| 2708 | 1KB | 16KB | overhang |
| 2716 | 2KB | 32KB | overhang |
| 28C16 | 2KB | 32KB | overhang |
| 2732 | 4KB | 32KB | overhang |
| 27C32 | 4KB | 32KB | overhang |
| 2364 | 8KB | 128KB | overhang |
| 4764 | 8KB | 128KB | overhang |
| MCM68364 | 8KB | 128KB | overhang |
| MCM68764 | 8KB | 128KB | overhang |
| MCM68A364 | 8KB | 128KB | overhang |
| MCM68A764 | 8KB | 128KB | overhang |
| MK36000 | 8KB | 128KB | overhang |
| MM52164 | 8KB | 128KB | overhang |

*32-pin chips (with fly-leads)*

| Chip | ROM size | Image size | Fit |
|:---|---:|---:|:---|
| 28C512 | 64KB | 64KB | larger socket (no fly-leads) |

## One ROM Fire 28 (rev C) — fire-28-c

*28-pin chips (native)*

| Chip | ROM size | Image size | Fit |
|:---|---:|---:|:---|
| 2764 | 8KB | 32KB | native |
| 27C64 | 8KB | 32KB | native |
| 27LC64 | 8KB | 32KB | native |
| 28C64 | 8KB | 32KB | native |
| 23128 | 16KB | 32KB | native |
| 27128 | 16KB | 32KB | native |
| 27C128 | 16KB | 32KB | native |
| 27LC128 | 16KB | 32KB | native |
| 23256 | 32KB | 64KB | native |
| 27256 | 32KB | 64KB | native |
| 27C256 | 32KB | 64KB | native |
| 27LC256 | 32KB | 64KB | native |
| 27SF256 | 32KB | 64KB | native |
| 28C256 | 32KB | 32KB | native |
| 23QL384 | 48KB | 256KB | native |
| 23512 | 64KB | 64KB | native |
| 23QL512 | 64KB | 256KB | native |
| 27512 | 64KB | 64KB | native |
| 27C512 | 64KB | 64KB | native |
| 27LC512 | 64KB | 64KB | native |
| 27SF512 | 64KB | 64KB | native |
| 231024 | 128KB | 128KB | native |
| 23C1000 | 128KB | 128KB | native |
| 23C1000A | 128KB | 128KB | native |
| MX23C1000 | 128KB | 128KB | native |
| TC531000 | 128KB | 128KB | native |

*24-pin chips (with overhang)*

| Chip | ROM size | Image size | Fit |
|:---|---:|---:|:---|
| 2704 | 512B | 4KB | overhang |
| HM7641 | 512B | 4KB | overhang |
| 2708 | 1KB | 8KB | overhang |
| 2716 | 2KB | 32KB | overhang |
| 28C16 | 2KB | 32KB | overhang |
| 2732 | 4KB | 32KB | overhang |
| 27C32 | 4KB | 32KB | overhang |
| 2364 | 8KB | 256KB | overhang |
| 4764 | 8KB | 256KB | overhang |
| MCM68364 | 8KB | 256KB | overhang |
| MCM68764 | 8KB | 256KB | overhang |
| MCM68A364 | 8KB | 256KB | overhang |
| MCM68A764 | 8KB | 256KB | overhang |
| MK36000 | 8KB | 256KB | overhang |
| MM52164 | 8KB | 256KB | overhang |

*32-pin chips (with fly-leads)*

| Chip | ROM size | Image size | Fit |
|:---|---:|---:|:---|
| 28C512 | 64KB | 64KB | larger socket (no fly-leads) |
| 23C1010 | 128KB | 128KB | fly-lead to X1 |
| 27C010 | 128KB | 128KB | fly-lead to X1 |
| 27C1000A | 128KB | 128KB | fly-lead to X1 |
| 27C1001 | 128KB | 128KB | fly-lead to X1 |
| 29F010 | 128KB | 128KB | fly-lead to X1 |
| 39SF010 | 128KB | 128KB | fly-lead to X1 |
| SST39SF010 | 128KB | 128KB | fly-lead to X1 |

## One ROM Fire 28 (rev D) — fire-28-d

*28-pin chips (native)*

| Chip | ROM size | Image size | Fit |
|:---|---:|---:|:---|
| 2764 | 8KB | 32KB | native |
| 27C64 | 8KB | 32KB | native |
| 27LC64 | 8KB | 32KB | native |
| 28C64 | 8KB | 32KB | native |
| 23128 | 16KB | 32KB | native |
| 27128 | 16KB | 32KB | native |
| 27C128 | 16KB | 32KB | native |
| 27LC128 | 16KB | 32KB | native |
| 23256 | 32KB | 64KB | native |
| 27256 | 32KB | 64KB | native |
| 27C256 | 32KB | 64KB | native |
| 27LC256 | 32KB | 64KB | native |
| 27SF256 | 32KB | 64KB | native |
| 28C256 | 32KB | 32KB | native |
| 23QL384 | 48KB | 256KB | native |
| 23512 | 64KB | 64KB | native |
| 23QL512 | 64KB | 256KB | native |
| 27512 | 64KB | 64KB | native |
| 27C512 | 64KB | 64KB | native |
| 27LC512 | 64KB | 64KB | native |
| 27SF512 | 64KB | 64KB | native |
| 231024 | 128KB | 128KB | native |
| 23C1000 | 128KB | 128KB | native |
| 23C1000A | 128KB | 128KB | native |
| MX23C1000 | 128KB | 128KB | native |
| TC531000 | 128KB | 128KB | native |

*24-pin chips (with overhang)*

| Chip | ROM size | Image size | Fit |
|:---|---:|---:|:---|
| 2704 | 512B | 4KB | overhang |
| HM7641 | 512B | 4KB | overhang |
| 2708 | 1KB | 8KB | overhang |
| 2716 | 2KB | 32KB | overhang |
| 28C16 | 2KB | 32KB | overhang |
| 2732 | 4KB | 32KB | overhang |
| 27C32 | 4KB | 32KB | overhang |
| 2364 | 8KB | 256KB | overhang |
| 4764 | 8KB | 256KB | overhang |
| MCM68364 | 8KB | 256KB | overhang |
| MCM68764 | 8KB | 256KB | overhang |
| MCM68A364 | 8KB | 256KB | overhang |
| MCM68A764 | 8KB | 256KB | overhang |
| MK36000 | 8KB | 256KB | overhang |
| MM52164 | 8KB | 256KB | overhang |

*32-pin chips (with fly-leads)*

| Chip | ROM size | Image size | Fit |
|:---|---:|---:|:---|
| 28C512 | 64KB | 64KB | larger socket (no fly-leads) |
| 23C1010 | 128KB | 128KB | fly-lead to X1 |
| 27C010 | 128KB | 128KB | fly-lead to X1 |
| 27C1000A | 128KB | 128KB | fly-lead to X1 |
| 27C1001 | 128KB | 128KB | fly-lead to X1 |
| 29F010 | 128KB | 128KB | fly-lead to X1 |
| 39SF010 | 128KB | 128KB | fly-lead to X1 |
| SST39SF010 | 128KB | 128KB | fly-lead to X1 |

## One ROM Fire 32 (rev A) — fire-32-a

*32-pin chips (native)*

| Chip | ROM size | Image size | Fit |
|:---|---:|---:|:---|
| 28C512 | 64KB | 256KB | native |
| 23C1010 | 128KB | 512KB | native |
| 27C010 | 128KB | 512KB | native |
| 27C100 | 128KB | 512KB | native |
| 27C1000 | 128KB | 512KB | native |
| 27C1000A | 128KB | 512KB | native |
| 27C1001 | 128KB | 512KB | native |
| 27C301 | 128KB | 512KB | native |
| 29F010 | 128KB | 512KB | native |
| 39SF010 | 128KB | 512KB | native |
| SST39SF010 | 128KB | 512KB | native |
| 27C020 | 256KB | 512KB | native |
| 27C2001 | 256KB | 512KB | native |
| 29F020 | 256KB | 512KB | native |
| 39SF020 | 256KB | 512KB | native |
| SST39SF020 | 256KB | 512KB | native |
| 27C040 | 512KB | 512KB | native |
| 27C4001 | 512KB | 512KB | native |
| 27C080 | 1MB | 512KB | native |
| 27C801 | 1MB | 512KB | native |

*28-pin chips (with overhang)*

| Chip | ROM size | Image size | Fit |
|:---|---:|---:|:---|
| 2764 | 8KB | 256KB | overhang |
| 27C64 | 8KB | 256KB | overhang |
| 27LC64 | 8KB | 256KB | overhang |
| 28C64 | 8KB | 256KB | overhang |
| 27128 | 16KB | 256KB | overhang |
| 27C128 | 16KB | 256KB | overhang |
| 27LC128 | 16KB | 256KB | overhang |
| 23256 | 32KB | 256KB | overhang |
| 27256 | 32KB | 256KB | overhang |
| 27C256 | 32KB | 256KB | overhang |
| 27LC256 | 32KB | 256KB | overhang |
| 27SF256 | 32KB | 256KB | overhang |
| 28C256 | 32KB | 256KB | overhang |
| 23512 | 64KB | 256KB | overhang |
| 27512 | 64KB | 256KB | overhang |
| 27C512 | 64KB | 256KB | overhang |
| 27LC512 | 64KB | 256KB | overhang |
| 27SF512 | 64KB | 256KB | overhang |
| 231024 | 128KB | 512KB | overhang |
| 23C1000 | 128KB | 512KB | overhang |
| 23C1000A | 128KB | 512KB | overhang |
| MX23C1000 | 128KB | 512KB | overhang |
| TC531000 | 128KB | 512KB | overhang |

*24-pin chips (with overhang)*

| Chip | ROM size | Image size | Fit |
|:---|---:|---:|:---|
| 2704 | 512B | 32KB | overhang |
| HM7641 | 512B | 32KB | overhang |
| 2708 | 1KB | 64KB | overhang |
| 2716 | 2KB | 256KB | overhang |
| 28C16 | 2KB | 256KB | overhang |
| 2732 | 4KB | 256KB | overhang |
| 27C32 | 4KB | 256KB | overhang |

## One ROM Fire 32 (rev B) — fire-32-b

*32-pin chips (native)*

| Chip | ROM size | Image size | Fit |
|:---|---:|---:|:---|
| 28C512 | 64KB | 64KB | native |
| 23C1001 | 128KB | 128KB | native |
| 23C1010 | 128KB | 128KB | native |
| 27C010 | 128KB | 128KB | native |
| 27C100 | 128KB | 256KB | native |
| 27C1000 | 128KB | 256KB | native |
| 27C1000A | 128KB | 128KB | native |
| 27C1001 | 128KB | 128KB | native |
| 27C301 | 128KB | 256KB | native |
| 29F010 | 128KB | 128KB | native |
| 39SF010 | 128KB | 128KB | native |
| D23C1001 | 128KB | 128KB | native |
| SST39SF010 | 128KB | 128KB | native |
| 27C020 | 256KB | 256KB | native |
| 27C2001 | 256KB | 256KB | native |
| 29F020 | 256KB | 256KB | native |
| 39SF020 | 256KB | 256KB | native |
| SST39SF020 | 256KB | 256KB | native |
| 27C040 | 512KB | 512KB | native |
| 27C4001 | 512KB | 512KB | native |
| 29F040 | 512KB | 512KB | native |
| 39SF040 | 512KB | 512KB | native |
| SST39SF040 | 512KB | 512KB | native |
| 27C080 | 1MB | 512KB | native |
| 27C801 | 1MB | 512KB | native |

*28-pin chips (with overhang)*

| Chip | ROM size | Image size | Fit |
|:---|---:|---:|:---|
| 2764 | 8KB | 32KB | overhang |
| 27C64 | 8KB | 32KB | overhang |
| 27LC64 | 8KB | 32KB | overhang |
| 28C64 | 8KB | 32KB | overhang |
| 27128 | 16KB | 32KB | overhang |
| 27C128 | 16KB | 32KB | overhang |
| 27LC128 | 16KB | 32KB | overhang |
| 23256 | 32KB | 32KB | overhang |
| 27256 | 32KB | 32KB | overhang |
| 27C256 | 32KB | 32KB | overhang |
| 27LC256 | 32KB | 32KB | overhang |
| 27SF256 | 32KB | 32KB | overhang |
| 28C256 | 32KB | 64KB | overhang |
| 23512 | 64KB | 64KB | overhang |
| 27512 | 64KB | 64KB | overhang |
| 27C512 | 64KB | 64KB | overhang |
| 27LC512 | 64KB | 64KB | overhang |
| 27SF512 | 64KB | 64KB | overhang |
| 231024 | 128KB | 256KB | overhang |
| 23C1000 | 128KB | 256KB | overhang |
| 23C1000A | 128KB | 256KB | overhang |
| MX23C1000 | 128KB | 256KB | overhang |
| TC531000 | 128KB | 256KB | overhang |

*24-pin chips (with overhang)*

| Chip | ROM size | Image size | Fit |
|:---|---:|---:|:---|
| 2704 | 512B | 4KB | overhang |
| 2708 | 1KB | 8KB | overhang |
| 2716 | 2KB | 32KB | overhang |
| 28C16 | 2KB | 32KB | overhang |
| 2732 | 4KB | 32KB | overhang |
| 27C32 | 4KB | 32KB | overhang |

## One ROM Fire 40 (rev A) — fire-40-a

| Chip | ROM size | Image size | Fit |
|:---|---:|---:|:---|
| 27C200 | 256KB | 512KB | native |
| HN62402 | 256KB | 512KB | native |
| 23C4100 | 512KB | 512KB | native |
| 27C400 | 512KB | 512KB | native |
| 27C4100 | 512KB | 512KB | native |
| AT27C400 | 512KB | 512KB | native |
| HN62404 | 512KB | 512KB | native |
| HN62424 | 512KB | 512KB | native |
| M27C400 | 512KB | 512KB | native |
| MB834200 | 512KB | 512KB | native |
| MX23C4100 | 512KB | 512KB | native |
| MX27C4100 | 512KB | 512KB | native |
| TCS534200 | 512KB | 512KB | native |

## One ROM Fire 40 (rev B) — fire-40-b

| Chip | ROM size | Image size | Fit |
|:---|---:|---:|:---|
| 27C200 | 256KB | 256KB | native |
| HN62402 | 256KB | 256KB | native |
| 23C4100 | 512KB | 512KB | native |
| 27C400 | 512KB | 512KB | native |
| 27C4100 | 512KB | 512KB | native |
| AT27C400 | 512KB | 512KB | native |
| HN62404 | 512KB | 512KB | native |
| HN62424 | 512KB | 512KB | native |
| M27C400 | 512KB | 512KB | native |
| MB834200 | 512KB | 512KB | native |
| MX23C4100 | 512KB | 512KB | native |
| MX27C4100 | 512KB | 512KB | native |
| TCS534200 | 512KB | 512KB | native |

