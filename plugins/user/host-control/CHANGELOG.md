# Changelog

## [0.1.2] - 2026-08-09

Report as many RAM slots as the RAM holds, rather than at most seven, and keep
those a host cannot name for the plugin's own use.

- **Potentially breaking.**  A RAM slot is exactly one ROM region, so with a
  small ROM there are now far more slots and each is far smaller than before —
  as little as 2KB for a 2316, where the old cap of seven was meant to keep a
  slot at 64KB but could not, since a slot has to be the size of the ROM being
  served.  A host that assumed a slot was large enough for some purpose of its
  own, or that there were at most seven, sees something different.
- `GET_RAM_SLOT_INFO_ALL` now reports at most 170 slots however many the
  firmware has, because every command that names a slot rejects 0xAA and a slot
  no host can name is no use to one.  Slots above that are the plugin's.

Make NV storage writable on devices whose RAM slots are too small to stage a
transaction in.  Where the plugin has slots of its own it stages there, using as
many consecutive slots as the staging buffer needs, and leaves the slot the host
named untouched.

- Previously `GET_NV_CAPABILITY` reported NV storage writable whenever the
  device had more than one RAM slot, but every `NV_POKE_BEGIN` then failed if no
  single slot could hold the 4KB staging buffer and the erase routine — which is
  every device serving a ROM smaller than that.

Route RBCP command decode through the observed (bus) address space, so command
signalling works on a 40-pin part whose least-significant address line the
device does not observe.

Stop `RBCP_RESET`, `EXIT_CMD_RESP_SILENT` and `SWITCH_AND_EXIT` writing to the
response header.  All three ran the first half of the command processing
sequence before being recognised as silent, leaving the token incremented and
progress stuck at pending.

- **Behaviour change**, bringing the plugin into line with the specification,
  which requires all three to update nothing.  A host that polled after one of
  them would previously have waited for a completion that never came.

Stop `GET_FLASH_SLOT_INFO_ALL` overwriting the truncated record's `rom_type`
when that byte is the only one present.  The null terminator is now written
only where the record carries a name.

- **Potentially breaking.**  RBCP v0.1.1 clarifies that a truncated record's
  final byte is 0x00 where a name is present — previously unspecified, and
  what the plugin already did.  A host that relied on that byte being the
  record's real byte, or on a one-byte record being terminated, sees different
  data.

Stop acting on Group 0x01 Read commands received in command mode, where the
specification makes them valid in command-response mode only.  Previously such
a command executed and wrote its answer into a back-channel region the device
had already stopped maintaining, modifying the served ROM image outside any
session.  Group 0x01 and Group 0x03 now consume the command's argument bytes
and discard it.

- **Potentially breaking** for a host that issued a Read command in command
  mode and used the result — behaviour the specification never permitted.
  Neither the RBCP 6502 reference host nor r107sl's C64 bootloader does: both
  issue every Read and NV command through the command-response polling path.

Report failure, rather than discarding silently, when `ENTER_CMD_RESP` asks for
a back-channel larger than the RAM slot.  The specification requires failure
here and silent discard for the other malformed-argument cases, and a host
tells the two apart by whether the token increments.

Versions before 0.1.2 predate this file.
