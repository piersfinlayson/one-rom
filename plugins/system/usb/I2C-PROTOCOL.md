# One ROM I2C Control Protocol (ORICP)

Copyright (C) 2026 Piers Finlayson <piers@piers.rocks>

Version: 0.1.0 IN DEVELOPMENT

**Very much a work in progress**

This specification may be freely implemented without restriction.

---

## Introduction

The One ROM I2C Control Protocol (ORICP) lets a computer or microcontroller
control a One ROM over I2C while the One ROM serves a ROM to the system it is
fitted in. The host is the I2C master. The One ROM is an I2C slave.

ORICP builds on top of the [ROM Bus Control Protocol
(RBCP)](https://github.com/piersfinlayson/rom-bus-control-protocol), reusing RBCP's:
- groups
- commands
- argument bytes
- response formats.

ORICP replaces RBCP's ROM bus transport with I2C, adding a transport to control
One ROM alongside USB and the ROM bus.

ORICP also provides [PICOBOOT and picobootx](#picobootx-device) over I2C.

## Summary

- The device is an I2C slave at a 7-bit address, at 100 kHz or 400 kHz.
- The register pointer selects a virtual device on that address:
  - the RBCP device, at 0x00 to 0xDF
  - the picoboot(x) device, at 0xE0 to 0xEF.
- An RBCP command is an I2C write consisting of:
  - register 0x00
  - RBCP command frame of
    - GROUP
    - CMD
    - argument bytes.
- The write's stop condition ends the frame.
- The result is read through an 8-bit register pointer from a back-channel
  region consisting of:
  - the 8-byte RBCP response header
  - the RBCP response data section.
- The device processes a command once the write completes. The host polls
  the header for status, as in RBCP.
- The RBCP device runs RBCP's commands. Those that enter or leave
  command-response mode are omitted, except RBCP_RESET.
- A picoboot(x) command is a 32-byte PICOBOOT command packet written at 0xE0.
  The host reads or writes the command's data at 0xE1 and reads its status at
  0xE2.
- The device may hold SCL low for at most 2 ms, or longer while it writes its
  flash.
- In Normal mode the device acknowledges its address at once. In Knock mode it
  stays silent until the host knocks.

---

## Relationship to RBCP

Three RBCP mechanisms are omitted due to I2C's properties:
- Each I2C transaction is addressed to one device, so RBCP's knock isn't
  required. [Knock mode](#knock) uses a different knock.
- The device owns the back-channel region, so the host doesn't configure it
  with ENTER_CMD_RESP.
- ORICP always uses command-response mode, so entering and leaving that mode
  isn't required.

---

## Terminology

**Host:** The I2C master that sends ORICP commands. This is the host referred
to by RBCP command definitions.

**Device:** One ROM, an I2C slave executing ORICP commands.

**System:** The computer whose ROM socket the device occupies. RBCP calls this
computer the host. ORICP calls it the system because it may be different to
the host (the I2C master).

**Transaction:** One I2C sequence from start to stop. It is one of:
- a write
- a read
- a write, a repeated start and a read.

**Register pointer:** The byte after the address in a write. It selects what
the transaction's remaining bytes are, and where a following read begins.

**Virtual device:** The part of the device at one range of register pointers.

**RBCP device:** The virtual device at register pointers 0x00 to 0xDF. It runs
RBCP commands.

**picoboot(x) device:** The virtual device at register pointers 0xE0 to 0xEF.
It runs PICOBOOT and picobootx commands. See [picoboot(x)
Device](#picobootx-device).

**RBCP command frame:** The GROUP, CMD and argument bytes of one RBCP command,
as RBCP's [Command
Framing](https://github.com/piersfinlayson/rom-bus-control-protocol/blob/main/spec/rbcp.md#command-framing)
defines them.

**Back-channel region:** The response to the last RBCP command, read from
register pointer 0x00.

**RBCP response header:** The first 8 bytes of the back-channel region.
Contains the following fields:
- last command
- token
- progress
- response.

**RBCP response data section:** The back-channel region from offset 8.
Contains command-specific response data.

**Command packet:** The 32 bytes a host writes to start a picoboot(x) command.

**IN data, OUT data:** The data of a picoboot(x) command. The host reads IN
data and writes OUT data.

**Command status:** 16 bytes containing the progress and result of the last
picoboot(x) command.

**Identity block:** 16 bytes read at register pointers 0xF0 to 0xFF. Contains
the magic, the ORICP version, the bus mode, whether the picoboot(x) device is
present, the region size and the idle timeout.

**Token, Progress, Response:** As RBCP defines them. The progress and response
values are fixed by ORICP, unlike RBCP where the host defines them.

**Normal mode:** The mode for a bus used only for I2C. The device acknowledges
its address at any time.

**Knock mode:** The mode for a bus also used for other traffic. The device
doesn't acknowledge any transaction outside a session.

**Probe:** A write transaction without a register pointer, comprising:
- start
- a 7-bit address with the write bit
- stop.

**Knock:** Five probes that open a session in Knock mode. See
[Knock](#knock).

**Session:** In Knock mode, the interval from a knock until the device stops
acknowledging. In Normal mode the device is always in session.

---

## Versioning and Compatibility

ORICP uses semantic versioning. During 0.x.y a minor increment may break
compatibility, and a host written against 0.Y.z interoperates with a device
implementing 0.Y.w where w >= z.

ORICP and RBCP are versioned separately:
- The ORICP version covers the I2C transport. The device reports it in the
  [identity block](#identity-block).
- The RBCP version covers the commands. The device reports it in response to
  GET_PROTOCOL_VERSION. RBCP's own versioning rules apply.

A device implementing ORICP 0.1 implements RBCP 0.1.3 or a later 0.1 version.
[Relationship to RBCP](#relationship-to-rbcp) omits commands by a rule rather
than a list, so the omissions also cover commands a later RBCP version adds.
Every other command runs as RBCP defines it. The stop condition ends every
frame, so a command the device doesn't implement fails without affecting the
next.

---

## Physical Medium

ORICP runs on an I2C bus.

- **SDA and SCL** are open-drain. The bus supplies the pull-ups. The device
  does not supply pull-ups. The device pins used for SDA and SCL are outside
  the scope of this document.
- **Bus voltage** is 3.3 V or 5 V. Care should be taken to ensure that non-5V
  tolerant pins are not used to receive 5V signals.
- **Address:** the device has a single 7-bit address. The default ORICP
  address is 0x4F, but a device may use another. The configuration mechanism
  for a device's address is outside the scope of this document.
- **Speed:** I2C standard mode, 100 kHz, and fast mode, 400 kHz.
- **Clock stretching:** the device holds SCL low for at most 2 ms within any
  transaction. While the device writes its flash, for example during an NV
  commit or a picoboot(x) flash page write, the hold lasts until the write
  ends.
  A host allows at least 2 ms. A transaction the host's own timeout ends is a
  failed transaction, and the host repeats it unless it was at pointer 0xE1
  (see [Data](#data)).

All multi-byte values are little-endian.

---

## Register Map

The register pointer selects what a transaction reads or writes.

```
pointer   0x00      0x08                   0xDF 0xE0          0xEF 0xF0     0xFF
          +---------+--------------------------+------------------+-------------+
read      | RBCP    | RBCP response data       | picoboot(x)      |  identity   |
          | response| section, continuing past | device           |  block      |
          | header  | 0xDF                     |                  |             |
          +---------+--------------------------+------------------+-------------+
write     | 0x00: RBCP command frame.          | picoboot(x)      |  ignored    |
          | 0x01 to 0xDF: ignored.             | device           |             |
          +------------------------------------+------------------+-------------+
```

Writes:
- Two or more bytes after pointer 0x00 are an RBCP command frame.
- Bytes after pointers 0xE0 to 0xEF go to the [picoboot(x)
  device](#picobootx-device).
- Any other bytes after the pointer are ignored.

Reads:
- A read starts at the register pointer last written. The pointer is 0x00
  when the device starts.
- A read starting at 0x00 to 0xDF returns the back-channel region from that
  offset to the end of the region, which may be past 0xDF.
- A read starting at 0xE0 to 0xEF returns bytes from the [picoboot(x)
  device](#picobootx-device).
- A read starting at 0xF0 to 0xFF returns the identity block from that offset.
- A read returns 0x00 for each byte past the end of the back-channel region,
  the command status or the identity block.

---

## Transactions

### Sending an RBCP command

Sending an RBCP command is done with a single write transaction consisting of:
- the device's address
- the register pointer 0x00
- the RBCP command frame.

The stop condition ends the frame.

```
 S | ADDR+W | A | 0x00 | A | GROUP | A | CMD | A | A0 | A | ... | An | A | P

 S  start        A  acknowledge, from the device        P  stop
```

A frame fails if either:
- the number of argument bytes differs from the number RBCP defines for the
  GROUP and CMD
- the device doesn't implement the GROUP or CMD.

A failed frame isn't executed. The device still runs the [command processing
sequence](#command-processing-sequence) with response = failed.

The device acknowledges every byte of a write.

While the device is processing an RBCP command it discards any frame, including
RBCP_RESET. A discarded frame doesn't change the header.

A host waits for progress to read complete before sending the next RBCP
command.

### Reading

Reading is done with a transaction consisting of:
- a write of the register pointer
- a repeated start
- a read of the bytes from the register pointer onwards, as the [register
  map](#register-map) defines
- a not-acknowledge and a stop from the host.

```
 S | ADDR+W | A | PTR | A | Sr | ADDR+R | A | D[PTR] | A | D[PTR+1] | A | ... | D[n] | N | P

 Sr  repeated start        N  not-acknowledge, from the host
```

A stop and a start in place of the repeated start work the same.

The RBCP response data section doesn't change from when progress reads
complete until the next command frame arrives. SLOT_PEEK and NV_PEEK data may
instead come from the slot or NV storage at the time of the read.

### Probe

A write containing only an address.

```
 S | ADDR+W | A | P        acknowledged: the device is in session

 S | ADDR+W | N | P        not acknowledged: the device is absent or not in session
```

---

## RBCP Response Header

The first 8 bytes of the back-channel region, read at pointer 0x00.

| Offset | Size | Field | Description |
|--------|------|-------|-------------|
| 0 | 2 | Last Command | The GROUP and CMD bytes of the most recently received command. |
| 2 | 2 | Token | Incremented by exactly 1 on receipt of every command, LSB first, wrapping from 0xFFFF to 0x0000. |
| 4 | 1 | Progress | Complete, 0x01, when the device has finished processing the last command. Pending, 0xFE, while it is processing. |
| 5 | 1 | Response | Status-OK, 0x01, where the last completed command succeeded. Failed, 0xFE, where it failed. |
| 6 | 2 | Reserved | Zero. |

RBCP defines:
- the fields and their meanings
- token atomicity
- pending and failed as the bitwise inverses of complete and status-OK.

Complete and status-OK are fixed because the header isn't part of a ROM image.

When the device starts, the header contains last command 0x0000, token 0,
progress complete and response status-OK. A host may issue a command at once.

### Command Processing Sequence

On receipt of an RBCP command frame the device performs, in order:

1. Set progress = pending
2. Increment token, LSB first
3. Update last command
4. Process the command
5. Set response = status-OK or failed
6. Set progress = complete

The device processes one RBCP command at a time.

### Host Polling Sequence

To issue an RBCP command the host:

1. Reads the token LSB
2. Sends the RBCP command frame
3. Polls the token LSB until it differs from the value read
4. Polls progress until it reads complete
5. Reads response
6. Reads any command-specific response data

Steps 3 to 6 are the same read at pointer 0x00, extended to the data
required. A single read of 8 bytes serves steps 3 to 5 together.

---

## Bus Modes

A device uses Normal or Knock bus mode. The bus mode is in the [identity
block](#identity-block). A session covers both virtual devices.

### Normal

The bus is used only for I2C. The device acknowledges its address from the
moment it starts and ignores a knock.

### Knock

The bus is also used for other traffic. For example, it may be an IEC serial
bus with two of its lines used as SCL and SDA. That traffic can match the
device's address by chance. Outside a session the device doesn't drive SDA or
SCL.

A host opens a session with a knock:
- The knock is five probes to 0x4F, 0x52, 0x49, 0x43 and 0x50 in that order.
  These are the ASCII codes of `ORICP`.
- Each probe starts within 10 ms of the previous one ending.
- Any other transaction between the probes restarts the knock.
- A probe to 0x4F always starts a new knock.
- A probe counts whether or not the device acknowledges it.

After the fifth probe the device acknowledges every transaction addressed to
it until the session ends.

```
 S 0x4F+W N P  S 0x52+W N P  S 0x49+W N P  S 0x43+W N P  S 0x50+W N P  S ADDR+W A P
              ^             ^             ^             ^              ^
              each gap under 10 ms                                     session open
```

A session ends when one of these occurs:
- the device runs an RBCP_RESET from the host
- a transaction to any other address starts
- the idle timeout passes without a transaction addressed to the device.

The [identity block](#identity-block) reports the idle timeout.

After a session ends, the device doesn't acknowledge any transaction until the
next knock.

The host keeps the bus free of other traffic during a session. Other traffic
can:
- end the session
- make a transaction fail with a not-acknowledge or a failed response
- corrupt the bytes of a transaction without it failing.

After a failure the host issues another knock then reads the header or the
command status to check whether the last command ran.

---

## Identity Block

Sixteen bytes read at pointer 0xF0. They are the same in every session,
before and after any command.

| Offset | Size | Field | Description |
|--------|------|-------|-------------|
| 0 | 4 | magic | The ASCII bytes `O`, `R`, `I`, `C`. |
| 4 | 1 | major | ORICP major version. |
| 5 | 1 | minor | ORICP minor version. |
| 6 | 1 | patch | ORICP patch version. |
| 7 | 1 | flags | Bit 0: set in Knock mode, clear in Normal mode. Bit 1: set where the picoboot(x) device is present. Bits 2 to 7 reserved. |
| 8 | 2 | region_size | Size of the back-channel region in bytes, at least 64. |
| 10 | 1 | idle_timeout | The session idle timeout in Knock mode, 1 to 255, in units of 10 ms. Zero in Normal mode. |
| 11 | 5 | Reserved | Zero. |

Reserved bits and bytes are zero. A host ignores them.

A host identifies a One ROM by the magic and reads the rest before it issues
a command.

---

## Command Set

ORICP doesn't define any commands of its own. The RBCP device runs these RBCP
commands:

| GROUP | Name | ORICP at RBCP 0.1.3 |
|-------|------|---------------------|
| 0x00 | Control | NOP only. |
| 0x01 | Read | As RBCP. |
| 0x02 | Modify | As RBCP. |
| 0x03 | NV Storage | As RBCP. |
| 0x04 | Pipes | As RBCP. |
| 0x05 | Auxiliary I/O | As RBCP, without SET_AUX_AND_EXIT and SET_AUX_SWITCH_EXIT. |
| 0x06 | LEDs | As RBCP. |
| 0xAA | Reset | RBCP_RESET, as below. |

RBCP commands limited to command-response mode are valid in ORICP.

The following are unchanged from RBCP:
- argument rules, including 0xAA being invalid in a final argument
- response formats and value tables.

Where an RBCP command depends on the back-channel region size, the size is
region_size from the identity block. The RBCP response data section is 8
bytes smaller.

RBCP_RESET:
- discards an NV write transaction in progress over I2C
- doesn't run the [command processing sequence](#command-processing-sequence)
  or change the header
- ends the session in Knock mode.

A host doesn't poll for the token to change after RBCP_RESET.

---

## picoboot(x) Device

The picoboot(x) device runs the same commands as the [USB plugin](README.md):
- PICOBOOT commands, defined in section 5.6 of the [RP2350
  datasheet](https://datasheets.raspberrypi.com/rp2350/rp2350-datasheet.pdf),
  *USB PICOBOOT interface*
- [picobootx](https://github.com/piersfinlayson/picobootx) commands, which
  use their own dMagic.

| Pointer | Write | Read |
|---------|-------|------|
| 0xE0 | Command packet | 0x00 |
| 0xE1 | OUT data | IN data |
| 0xE2 | Ignored | Command status |
| 0xE3 | Reset | 0x00 |
| 0xE4 to 0xEF | Reserved, ignored | Reserved, 0x00 |

Where bit 1 of flags in the identity block is clear, the picoboot(x) device is
absent. Reads at 0xE0 to 0xEF then return 0x00 and writes to them are ignored.

The RBCP device and the picoboot(x) device have separate state. An RBCP
command may run while a picoboot(x) command does.

### Command Packet

A write of 32 bytes after pointer 0xE0 is a command packet, laid out as RP2350
datasheet section 5.6.4 defines. A write of any other number of bytes after
0xE0 is ignored.

```
 S | ADDR+W | A | 0xE0 | A | P0 | A | P1 | A | ... | P31 | A | P
```

The device starts the command once the write completes.

While a picoboot(x) command is in progress the device discards any command
packet. A discarded packet doesn't change the command status. A host waits for
the command to complete, or resets the picoboot(x) device, before writing the
next command packet.

### Data

Where a command's dTransferLength isn't zero, the host reads or writes that
many bytes at pointer 0xE1. Bit 7 of bCmdId sets the direction:
- set: IN data
- clear: OUT data.

```
 S | ADDR+W | A | 0xE1 | A | D0 | A | D1 | A | ... | Dn | A | P               OUT data

 S | ADDR+W | A | 0xE1 | A | Sr | ADDR+R | A | D0 | A | ... | Dn | N | P      IN data
```

Every byte after pointer 0xE1 is a data byte. The pointer doesn't advance. The
host may split the data across transactions of any length.

Reads at 0xE1 return 0x00 and writes to it are ignored, except while the
command in progress has data left to transfer in that direction.

When a command writes flash, the device holds SCL low on the last OUT data
byte of each page until the page is written.

A failed transaction at 0xE1 may have transferred some of its bytes, so the
host doesn't repeat it. The host resets the picoboot(x) device and sends the
command again.

### Command Status

A read at pointer 0xE2 returns the 16-byte command status, laid out as
GET_COMMAND_STATUS returns it over USB (RP2350 datasheet section 5.6.5.2).

- dToken and bCmdId are from the last command packet.
- bInProgress is 1 while the command is in progress and 0 once it completes
  or fails.
- dStatusCode is the result once bInProgress is 0.

When the device starts, the command status is zero.

A command completes:
- without data, once the device has run it
- with OUT data, once the device has written the last byte
- with IN data, once the host has read the last byte.

Over USB a zero-length packet acknowledges completion. Over I2C the command
status is the only indication of completion.

Over USB a failed command stalls the endpoints. Over I2C nothing stalls, and
a host may write the next command packet without a reset.

### Host Sequence

To issue a picoboot(x) command the host:

1. Writes the command packet at 0xE0
2. Reads any IN data or writes any OUT data at 0xE1
3. Polls the command status at 0xE2 until dToken matches the command packet's
   and bInProgress reads 0
4. Reads dStatusCode

IN data is valid only where dStatusCode reads OK. A host may read the command
status at any step, for example to check for a failure before transferring
data.

### Reset

A write at pointer 0xE3 resets the picoboot(x) device as INTERFACE_RESET does
over USB (RP2350 datasheet section 5.6.5.1). Bytes after the pointer are
ignored.

A reset doesn't affect the RBCP device or a Knock mode session.

---

## Coexistence with Other Transports

A device may implement RBCP on its ROM bus, picoboot(x) on USB and ORICP on
I2C simultaneously.

- Each transport has separate state, including any back-channel region or
  command status.
- A command over one transport may execute while a command over another
  does, except picoboot(x) commands over USB and I2C. The device has one flash
  write buffer for both, so only one runs at a time.
- A picoboot(x) command sent over USB or I2C while one is in progress over the
  other fails with dStatusCode INVALID_STATE (9).
- An NV commit over I2C and a picoboot(x) flash write don't overlap. The one
  received second waits until the first ends. While it waits, progress reads
  pending for an NV commit and bInProgress reads 1 for a flash write.
- Commands over every transport act on the same slots, NV storage, pins, LEDs
  and pipes.
- The device applies changes in the order it receives them.
- A reset over one transport doesn't affect any other.
- While an NV write transaction begun over one transport is in progress,
  commands over any other transport that start or use an NV write transaction
  fail.
- PIPE_WRITE commands over different transports interleave and the bytes of
  each stay together.
- IN bytes from a pipe go to the transport of the first PIPE_READ on it until
  the device restarts. PIPE_READ on that pipe over any other transport fails.

---

## Example — Loading and Switching a Kernal

This example isn't normative.

1. In Knock mode, issue a knock.
2. Read 16 bytes at 0xF0. Check the magic and the ORICP version.
3. Issue GET_PROTOCOL_VERSION. Read the RBCP version from the RBCP response
   data section.
4. Issue GET_RAM_SLOT_INFO_ALL. Choose a RAM slot other than the active one.
5. Load the image into that RAM slot with LOAD_SLOT from a flash slot
   containing it or with SLOT_POKE once per byte.
6. Hold the system in reset. Use SET_AUX where a device pin is wired to the
   system's reset line.
7. Issue SWITCH_SLOT for the RAM slot.
8. Release the reset the same way. The system boots the new image.
9. In Knock mode, end the session with RBCP_RESET.

Each command except RBCP_RESET follows the [host polling
sequence](#host-polling-sequence).

---

## Attribution

ORICP is an I2C transport for the [ROM Bus Control
Protocol](https://github.com/piersfinlayson/rom-bus-control-protocol). The use
case that shaped it, a drive emulator loading a kernal into a One ROM over I2C,
came from [Jaime Idolpx](https://github.com/idolpx).
