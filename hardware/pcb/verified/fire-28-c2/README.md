# Fire 28 Rev C2

A 4 layer revision of fire-28-c.  Uses a flat (inline) USB-C connector.

This 4 layer design has less noise and avoids possible USB packet corruption while serving ROMs.

**Has components on both sides of the board.**

Changes from fire-28-c:
- 4 layer PCB, with two internal ground planes.
- D+ and D- rerouted further from D0-D7, with 5 mil rather than 4 mil tracks.
- C14 moved.
- RGB LED moved slightly.
- QSPI_SS and D0-D6 tracks moved slightly.
