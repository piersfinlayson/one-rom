# One ROM RGB

A user plugin that cycles One ROM's RGB LED through the hues. It runs on
firmware v0.7.1 or later, on the models that have the LED, and it occupies the
user plugin slot.

## Firmware RGB control supersedes this plugin

From firmware v0.7.2 One ROM drives its RGB LED itself, and the CLI reaches it
with `onerom control rgb` — on, off, beacon, flame, cycle, breathe and blink,
each with a colour, a brightness and a speed, and `onerom inspect rgb` reads
back what the LED is doing. None of that needs a user plugin.

This plugin is unchanged and keeps working. A device running it behaves exactly
as it did.

**Do not run this plugin on a device you also drive with `onerom control rgb`.**
Both write the same GPIO, and nothing on the device detects that they are
fighting over it. Pick one.

The user plugin slot is the other reason to choose. This plugin takes it, so a
device using it cannot also run `host-control`. Firmware RGB control leaves the
slot free.

## Building

```bash
make
```

This creates `build/plugin_user.bin`, which can be loaded onto One ROM as a user
plugin.
