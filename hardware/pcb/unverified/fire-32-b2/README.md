# Fire 32 Rev B2

**Unverified**

A variant of the 32 pin version.  Adds native support for the SST39SF040, while retaining all existing ROM support.

**Has components on both sides of the board.**

Changes from fire-32-a:
- Adds a neo-pixel LED, GPIO 44, alongside the status LED at GPIO 45.
- Address and CS pins have been laid out differently to allow SST39SF040 to be served without using a shim.  Alo allows more efficient use of flash for most image types.

KiCad and fabrication files are not yet provided for this revision.

## Contents

- [Schematic](./fire-32-b2-schematic.pdf)
