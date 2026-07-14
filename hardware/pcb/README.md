# PCB Files

Contains One ROM PCB design files.  These are organised into two directories:

- [`verified/`](verified/README.md) - Verified designs that have been tested and work.
- [`unverified/`](unverified/README.md) - Unverified designs that have not been tested.

Before using any of the designs, review the hardware [LICENSE](/LICENSE.md#cern-ohl-w-20-license) and [NOTICE](/hardware/NOTICE) files.

## Recommended Revisions

Last Updated 4th July 2026.

The following table lists the recommended verified designs.  Each design has been manufactured and assembled using JLCPCB's PCB assembly service, and verified to work correctly.

| Model | MCU | Pins | USB | SWD | Image Select Pins | X Pins | RGB | PCB |
|-------|-----|------|-----|-----|-------------------|--------|-----|-------|
| Fire  | RP2350A | 24 | Micro-B | ✓ | 4 | 2 | - | [fire-24-d](./verified/fire-24-d/README.md) |
| Fire  | RP2354A | 24 | C | ✓ | 4 | 2 | - | [fire-24-e](./verified/fire-24-e/README.md) |
| Fire  | RP2354A | 24 | C | ✓ | 4 | 2 | ✓ | [fire-24-f](./verified/fire-24-f/README.md) |
| Fire  | RP2350A or RP2354A | 28 | C | ✓ | 2 | n/a | - | [fire-28-a4](./verified/fire-28-a4/README.md) |
| Fire  | RP2350A or RP2354A | 28 | C (upright) | ✓ | 2 | n/a | - | [fire-28-b](./verified/fire-28-b/README.md) |
| Fire  | RP2354B | 28 | C | ✓ | 2 | 2 | ✓ | [fire-28-c](./verified/fire-28-c/README.md) |
| Fire  | RP2350B or RP2354B | 32 | C | ✓ | 4 | n/a | - | [fire-32-a](./verified/fire-32-a/README.md) |
| Fire  | RP2350B or RP2354B | 32 | C | ✓ | 4 | n/a | ✓ | [fire-32-b2](./verified/fire-32-b2/README.md) |
| Fire  | RP2350B or RP2354B | 40 | C | ✓ | 4 | n/a | - | [fire-40-a](./verified/fire-40-a/README.md) |
| Fire  | RP2350B or RP2354B | 40 | C | ✓ | 4 | n/a | ✓ | [fire-40-b](./verified/fire-40-b/README.md) |

**One ROM 28C, 32 and 40 require components on the top and bottom of the PCB**

There may be later, unverified, revisions not in the list above.  These may have improvements over the recommended revisions, but they have not been verified to work.  If you want to use one of these later revisions, read the relevant README before ordering.

## Alternative Components

Sometimes your PCB manufacturer/assembler may be out of stock of a particular component.  This section lists some reasonable alternatives, using LCSC/JLC part numbers.

| Original Component | Original LCSC/JLC Part Number | Alternative Component | Alternative LCSC/JLC Part Number | Verified? | Notes |
|-|-|-|-|-|-|
| Diodes Inc AP2112K-3.3TRG1 | C51118 | Tech Public AP2112K-3.3TRG1 | C23380830 | Yes | Jelly bean part, same specs |
| Uniroyal 0201WMF1002TEE | C473048 | Yageo RC0201FR-0710KL | C106225 | No | Jelly bean part, same specs |
| Raspberry Pi RP2350A | C42411118 | Raspberry Pi RP2354A | C41378174 | Yes | If using RP2354A, do not populate the external flash chip, as the RP2354A has internal flash. |
| Raspberry Pi RP2350B | C42415655 | Raspberry Pi RP2354B | C39843328 | Yes | If using RP2354B, do not populate the external flash chip, as the RP2354B has internal flash. |

The RP2354A can be replaced with the RP2350A _if the design supports external flash and it is populated_.
- Some designs, such as fire-24-e, do not support external flash, so the RP2354A cannot be replaced with the RP2350A.
- Some designs, such as fire-40-a, have hardware support for both external flash plus an RP2354B, so two lots of flash.  Fabricating this option requires the correct 0R resistors to be populated/not populated.  See the board schematic for more details.

In some cases, alternative BOM/POS files are already provided for some alternative components.  In other cases, you may need to edit the BOM/POS files yourself to substitute the alternative component.  _Always_ double check component values, part numbers and placement before ordering.

## Notes on Fabrication

All recommended designs have been manufactured and assembled using JLCPCB's PCB assembly service.  The gerbers and BOM/POS files in each revision's `fab/` directory are compatible with JLCPCB's requirements.

Some variants offer multiple sets of BOM/POS files or different combinations of components.  Read the relevant revision's README for details.

You need to exercise care when submitting the order to ensure the correct options are selected.  In particular:

- Select economic assembly, not standard assembly, for cost reasons.
- Use standard 1.6mm PCB thickness.  You probably want HASL with lead finish for cost reasons.
- You have to select the desired PCB colour as part of the PCB ordering process.  Red is recommended for Fire and blue for Ice.
- As of late 2025 you no longer need to remove JLC's order ID from the PCBs, but it is worth checking this is still the case when you place your order.
- After uploading the BOM and position files for assembly, you need to ensure all parts are available and checked.
- You must ensure JLC is showing the correct orientation for all components.  There are silkscreen markings for pin 1 on all ICs and polarized components - ensure this matches the pink dot of the component in the viewer.  Also ensure the USB connector is the correct orientation.

Occasionally, some chosen parts are out of stock on JLCPCB.  One ROM has been designed with extremely common parts where possible, which JLC tend to hold large stocks of.  However, if a part is unavailable you may need to select an alternative part.  Ensure the alternative part has the same footprint and electrical characteristics (e.g. capacitance, voltage rating, etc) as the original part.  If in doubt, query within github discussions for advice. 

JLC may raise an issue with you after submitting your order.  Common issues include:
- JLC may ask to add side rails with stamp holes, to help with the PCB assembly process.  While One ROM has been successfully fabbed without these, if JLC recommends them it is best to accept this change, as otherwise they may not warrant the assembled boards.  The side rails can be snapped off using the "mouse bites" (stamp holes) after assembly.  JLC may or may not accept replacing stamp holes with v-score - as v-score is not available on their economic service.

If in doubt, query within github discussions for advice.

Assembly errors:
- Ice boards have rarely, if ever, experienced assembly errors.
- The first few batches of Fire boards experienced some assembly issues, particularly with respect to the RP2350 MCU solder pad fillets.  More recent boards and orders have been without issue.  It is unclear whether differences in the later PCBs or improvements in JLC's assembly process are responsible for this improvement.  If you do experience assembly issues, raise with JLC support.  It would also be interesting to hear on github discussions what issues you experienced, so others can be aware.
