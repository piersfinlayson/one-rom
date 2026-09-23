# One ROM I2C Control Protocol (ORICP)

Copyright (C) 2026 Piers Finlayson <piers@piers.rocks>

Version: 0.1.0 IN DEVELOPMENT

** Very much a work in progress **

This specification may be freely implemented without restriction.

---

## Introduction

> Section rewritten.  Review.

The One ROM I2C Control Protocol (ORICP) lets a computer or microcontroller
control a One ROM over I2C while the One ROM serves a ROM to the system it is
fitted in. The host is the I2C master. The One ROM is an I2C slave.

ORICP builds on top of the [ROM Bus Control Protocol
(RBCP)](https://github.com/piersfinlayson/rom-bus-control-protocol), reusing RBCP's:
- groups
- command
- argument bytes
- same response formats.

ORICP replaces the transport layer with I2C, adding an additional transport to control
One ROM, alongside USB and the ROM Bus.

## Summary

> Rewritten to be clear and less verbose.

- The device is an I2C slave at a 7-bit address, at 100 kHz or 400 kHz.
- A command is an I2C write consisting of:
  - register 0x00
  - RBCP frame of
    - GROUP
    - CMD
    - argument bytes.
  The write's stop condition ends the frame.
- The result is read through an 8-bit register pointer from a 240-byte
  back-channel consisting of:
  - the RBCP 8-byte sttus header
  - the command's response data.
- The device processes a command once the write completes.  The host polls
  the header for status, as in RBCP. The device holds SCL low for at most
  2 ms.
- On a bus of its own the device answers at once. On a bus it shares with
  other traffic it stays silent until the host knocks.
- The command set is RBCP 0.1.2, groups 0x01 to 0x06 and 0xAA unchanged.
  Group 0x00 carries NOP alone.

> I simply don't know why 2ms is in here, and whether it belongs in that bullet. Explain.

---

## Relationship to RBCP

> Rewritten.  We didn't need a motivation.  It's obvious.

ORICP re-uses the command RBCP framing and commands, replacing the ROM Bus
transport with I2C.

Some RBCP commands are omitted due to I2C's properties:
- Every transaction is addressed to the device, so an I2C compliant doesn't require a
  knock.
- The device owns the response memory space, no back-channel is required.
- I2C always uses command-response mode - so entering/exiting that mode is not required.

> The last bullet here was very vague so I have no idea if I captured it.

---

## Terminology

> Rewritten.

**Host:** The I2C master that sends ORICP commands. This is the host referred
to by RBCP command definitions.

**Device:** One ROM, an I2C slave executing ORICP commands.

**System:** The computer whose ROM socket the device occupies. RBCP calls this
computer the host. ORICP calls it the system because it may be different to
the host (the I2C master).

**Transaction:** One I2C start to stop addressed to the device. A register
write, a repeated start and a read together are one transaction.

**Register pointer:** The byte after the address in a write. It selects what
the transaction's remaining bytes are, and where a following read begins.

**Command frame:** The GROUP, CMD and argument bytes of one RBCP command, as
RBCP defines them.

> left, but needs correcting.

**Back-channel region:** 240 bytes the device owns, read at register pointers
0x00 to 0xEF. The device writes responses there.

**Response header:** The first 8 bytes of the back-channel region. Contains
the last command, token, progress and response fields.

**Response data section:** The 232 bytes of the back-channel region from
offset 8. Contains command-specific response data.

> I like the ID block being at the end

**Identity block:** 16 bytes read at register pointers 0xF0 to 0xFF, saying
what the device is and which ORICP version it implements.

**Token, Progress, Response:** As RBCP defines them. Their values are fixed by
ORICP, unlike RBCP where the host defines them.

**Probe:** A write transaction with no register pointer, comprising:
- start
- the device's address with the write bit
- stop.

**Knock:** Four probes in quick succession. On a shared bus the device answers
nothing until it has heard one.

> What is the change of a knock being detected accidently using IEC?  I worry with random data it's high and we actually need something more complex.  Analyse.

> Shared bus undefined.  I didn't feel capable of rewriting this one.

**Session:** On a shared bus, the interval from a knock until the device stops
answering. On a bus of its own the device is always in session.

---

## Versioning and Compatibility

ORICP uses semantic versioning.  During 0.x.y a minor increment may break
compatibility, and a host written against 0.Y.z interoperates with a device
implementing 0.Y.w where w >= z.

> You are type ORICP 0.1.* to RBCP 0.1.2?

This document names one RBCP version, 0.1.2. A device implementing ORICP
0.1.x implements that version of RBCP over I2C. The device reports the RBCP
version is response to GET_PROTOCOL_VERSION and its ORICP version in the [identity
block](#identity-block). A host should read both, and not issue a command whose
RBCP Since value exceeds the version the device reports.

---

## Physical Medium

> Why would you take about device build in a protocol spec?

> Rewritten

ORICP runs on an I2C bus.

- **SDA and SCL** are open-drain. The bus supplies the pull-ups. The device
  does not supply pull-ups.
- **Bus voltage** is 3.3 V or 5 V. Care should be taken to ensure that non-5V
  tolerant pins are not used to receive 5V signals.
- **Address:** the device answers its single 7-bit address. The default ORICP
  address is 0x4F, but may use a different address. The configuration mechanism
  for a device's address is outside the scope of this document.
- **Speed:** I2C standard mode, 100 kHz, and fast mode, 400 kHz.
- **Clock stretching:** the device holds SCL low while it prepares the bytes a
  read asks for, and for at most 2 ms within any transaction. A host allows at
  least that. A transaction the host's own timeout ends is a failed
  transaction, and the host repeats it.

> Again with the device build.  See what I wrote above.  Make this follow that.

Which of the device's pads carry SDA and SCL is a property of the device build
and is documented with the device, not here.

All multi-byte values are little-endian.

---

## Register Map

> I think each (header, response data section and command frame) should say RBCP in front.

> This needs redoing with what we've agreed.

The register pointer selects one of three things.

```
pointer   0x00      0x08                              0xEF  0xF0        0xFF
          +---------+---------------------------------------+-------------+
read      | header  |     response data section (232)       |  identity   |
          |  (8)    |                                       |    (16)     |
          +---------+---------------------------------------+-------------+
write     | command |                 no effect                           |
          |  frame  |                                                     |
          +---------+-----------------------------------------------------+
```

A write at pointer 0x00 carries a command frame. A write at any other pointer
is accepted and has no effect. A read returns bytes from the pointer onward.

---

## Transactions

### Sending a command

One write transaction. The device's address, the register pointer 0x00, then
the command frame. The stop condition ends the frame.

```
 S | ADDR+W | A | 0x00 | A | GROUP | A | CMD | A | A0 | A | ... | An | A | P

 S  start        A  acknowledge, from the device        P  stop
```

The frame carries exactly the argument bytes RBCP defines for its GROUP and
CMD. A frame of any other length is refused: the device runs the [command
processing sequence](#command-processing-sequence) with a response of failed
and executes nothing. The same applies to a GROUP or CMD the device does not
implement. Every byte of the write is consumed either way.

A frame that arrives while the device is processing a command is discarded
without touching the header. A host issues a command only when the header
shows the last one complete.

### Reading the back-channel

A write of the register pointer, a repeated start, then a read. The device
returns the byte at the pointer, then the following bytes, wrapping from 0xFF
to 0x00. The host ends the read with a not-acknowledge and a stop.

```
 S | ADDR+W | A | PTR | A | Sr | ADDR+R | A | D[PTR] | A | D[PTR+1] | A | ... | D[n] | N | P

 Sr  repeated start        N  not-acknowledge, from the host
```

A read with no register pointer, start then the device's address with the read
bit, begins at 0x00.

The response data section is stable from the moment progress reads complete
until the next command frame arrives. A host reads it then.

### Probe

A write of the address alone.

```
 S | ADDR+W | A | P        acknowledged: the device is in session

 S | ADDR+W | N | P        not acknowledged: absent, or listening on a shared bus
```

---

## Response Header

The first 8 bytes of the back-channel region, read at pointer 0x00.

| Offset | Size | Field | Description |
|--------|------|-------|-------------|
| 0 | 2 | Last Command | The GROUP and CMD bytes of the most recently received command. |
| 2 | 2 | Token | Incremented by exactly 1 on receipt of every command, LSB first, wrapping from 0xFFFF to 0x0000. |
| 4 | 1 | Progress | 0x01 when the device has finished processing the last command, complete. 0xFE while it is processing, pending. |
| 5 | 1 | Response | 0x01 where the last completed command succeeded, status-OK. 0xFE where it failed. |
| 6 | 2 | Reserved | Zero. |

The fields, their meaning and the token's atomicity are RBCP's. ORICP fixes
the complete and status-OK values, since the region holds no image data the
host must avoid. The pending and failed values are their bitwise inverses, as
in RBCP.

When the device starts, the header reads as last command 0x0000, token 0,
progress complete and response status-OK, so a host may issue a command at
once.

### Command Processing Sequence

On receipt of a command frame the device performs, in order:

1. Set progress = pending
2. Increment token, LSB first
3. Update last command
4. Process the command
5. Set response = status-OK or failed
6. Set progress = complete

The device processes one command at a time.

### Host Polling Sequence

To issue a command the host:

1. Reads the token LSB
2. Sends the command frame
3. Polls the token LSB until it differs from the value read
4. Polls progress until it reads complete
5. Reads response
6. Reads any command-specific response data

Steps 3 to 6 are the same read at pointer 0x00, extended to the data the host
wants. A single read of 8 bytes serves steps 3 to 5 together.

---

## Bus Modes

A device is built for one of two bus modes. The [identity
block](#identity-block) reports which.

### Plain

The bus carries I2C traffic alone. The device acknowledges its address from
the moment it starts and every transaction is answered. A probe succeeds at
any time. The knock is unnecessary and harmless.

### Shared

The bus also carries traffic that is not I2C, such as the serial bus of the
system the device is fitted in, whose lines a host may drive as SDA and SCL. An
I2C slave that acknowledged its address would answer that traffic whenever
eight bits of it happened to match, and hang the bus by holding a line low.
In shared mode the device acknowledges nothing outside a session.

A host opens a session with a knock: four probes, each starting within 10 ms
of the previous one ending. None of the four is acknowledged. The device
acknowledges the next transaction addressed to it, and every one after that,
for the length of the session. A host that probes repeatedly until it is
acknowledged has knocked.

```
  S ADDR+W N P   S ADDR+W N P   S ADDR+W N P   S ADDR+W N P   S ADDR+W A P ...
  |<--- 10 ms -->|<--- 10 ms -->|<--- 10 ms -->|             answered from here
```

A session ends when the device receives RBCP_RESET, or when 500 ms pass with
no transaction addressed to it. The device then returns to listening and the
next session needs a knock.

The host is responsible for the bus being free of other traffic for the whole
of a session. Traffic from a third party during a session corrupts the
transaction it lands in, which the host sees as a not-acknowledge or a failed
command and repeats after RBCP_RESET and a fresh knock.

---

## Identity Block

Sixteen bytes read at pointer 0xF0. The device answers them in every session,
before and after any command.

| Offset | Size | Field | Description |
|--------|------|-------|-------------|
| 0 | 4 | magic | The ASCII bytes `O`, `R`, `I`, `C`. |
| 4 | 1 | major | ORICP major version. |
| 5 | 1 | minor | ORICP minor version. |
| 6 | 1 | patch | ORICP patch version. |
| 7 | 1 | flags | Bit 0: the device is in shared bus mode. Bits 1 to 7 zero. |
| 8 | 2 | region_size | Size of the back-channel region in bytes, 240. |
| 10 | 6 | Reserved | Zero. |

A host scanning a bus recognises a One ROM by the magic and reads the rest
before it issues a command.

---

## Command Set

Commands are RBCP's. This section says which RBCP definitions apply and how
RBCP's terms map onto ORICP. It defines no command of its own.

| GROUP | Name | ORICP |
|-------|------|-------|
| 0x00 | Control | NOP alone. |
| 0x01 | Read | As RBCP. |
| 0x02 | Modify | As RBCP. |
| 0x03 | NV Storage | As RBCP. |
| 0x04 | Pipes | As RBCP. |
| 0x05 | Auxiliary I/O | As RBCP. |
| 0x06 | LEDs | As RBCP. |
| 0xAA | Reset | As RBCP. On a shared bus RBCP_RESET also ends the session. |

Where RBCP says a command is valid in command-response mode only, it is valid
in ORICP. ORICP has one mode, and it is that one.

The commands RBCP defines for entering, leaving and restoring after
command-response mode have no meaning on I2C and the device does not implement
them: ENTER_CMD_RESP, EXIT_CMD_RESP_ACK, EXIT_CMD_RESP_SILENT, SWITCH_AND_EXIT,
LOAD_AND_EXIT and EXIT_CMD_RESP_RESTORE. A host uses SWITCH_SLOT and LOAD_SLOT
in their place. SET_AUX_AND_EXIT and SET_AUX_SWITCH_EXIT are not implemented
for the same reason, and a host issues SET_AUX and SWITCH_SLOT as two commands.
A frame naming any of these fails as an unknown command does.

RBCP's argument rules apply unchanged, including that 0xAA is invalid in a
final argument. The response formats and value tables are RBCP's, read from
the response data section at offset 8 of the back-channel region. The region
is 240 bytes, so a command RBCP defines as failing where the response data
section is too small fails at 232 bytes: SLOT_PEEK, NV_PEEK and PIPE_READ with
a count of zero, which means 256, fail, and GET_FLASH_SLOT_INFO_ALL returns
what fits and reports the rest as RBCP describes.

RBCP_RESET resets the device's ORICP state and updates no header field. The
host then reads the header before issuing a further command.

---

## Coexistence with RBCP

A device may carry RBCP on its ROM bus and ORICP on I2C at the same time. The
two are separate sessions, each with its own back-channel and its own state,
and a command on one may execute while a command on the other does. Both act
on the device's slots, pins, LEDs and pipes through the device, which applies
each change in the order it receives it. A pipe is read by one session at a
time: PIPE_READ on a pipe the other session is reading fails, and writes to a
pipe from both sessions interleave. RBCP_RESET on one session leaves the other
as it was.

---

## Example — Loading and Switching a Kernal

This illustrates an ORICP session in which a host loads a ROM image into a
RAM slot and switches the system to it. It is illustrative rather than
normative.

1. **Knock**, on a shared bus: probe until acknowledged.
2. **Identify.** Read 16 bytes at 0xF0. Check the magic and the ORICP version.
3. **Check the command set.** Issue GET_PROTOCOL_VERSION. Read the RBCP
   version from the response data section.
4. **Find a slot.** Issue GET_RAM_SLOT_INFO_ALL. Choose a RAM slot other than
   the active one.
5. **Load the image.** Either issue LOAD_SLOT naming a flash slot that already
   holds it, or issue SLOT_POKE once per byte, polling the header after each.
6. **Switch.** Issue SWITCH_SLOT naming the RAM slot. The system now reads the
   new image.
7. **Restart the system**, where the image is one the system must boot into.
   Where a device pin is wired to the system's reset line, issue SET_AUX to
   pulse it. Otherwise the host resets the system by its own means.
8. **End the session** with RBCP_RESET.

Each command follows the [host polling sequence](#host-polling-sequence).

---

## Attribution

ORICP is a transport for the [ROM Bus Control
Protocol](https://github.com/piersfinlayson/rom-bus-control-protocol), whose
command set it carries. The use case that shaped it, a drive emulator loading a
kernal into a One ROM over I2C, came from [Jaime
Idolpx](https://github.com/idolpx).
