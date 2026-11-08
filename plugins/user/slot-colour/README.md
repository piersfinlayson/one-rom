# One ROM Slot Colour

A user plugin that lights One ROM's RGB LED using a different colour depending on
the slot (image) loaded at boot.

| Slot | Colour |
| --- | --- |
| 0 | Red |
| 1 | Green |
| 2 | Blue |
| 3 | Yellow |
| 4 | Cyan |
| 5 | Magenta |
| 6 | Orange |
| 7 | White |

The colours wrap if a slot > 7 is booted.

## Building

```bash
make
```

This creates `build/plugin_user.bin`.
