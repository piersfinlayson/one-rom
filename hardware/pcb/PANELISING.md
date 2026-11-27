# PANELISING PCBs

One ROM can be panelised for efficient manufacturing at scale.

You have two options for panelisation with JLC:
- Have JLC panelise a single type of One ROM.
- Build a custom panel with multiple different One ROM types.

All of the below assumes you are panelising USB-C version of One ROM.


## JLC Panelisation

To panelise using JLC:
- Upload the gerbers for a single piece (found the in `fab` folder of the design you are using) to JLC as normal
- Choose yor PCB colour
- Under PCB quantity, select the number of **panels** you want (e.g. 5)
- Under panlisation select "Panel by JLCPCB" or similar.  Then select
    - The number of columns (e.g. 5)
    - The number of rows (e.g. 4)
    - Edge rails (recommend left and right)
    As part of this check that the lines between the PCBs are marked "V-CUT".
- This example would give 100 One ROMs (5 columns x 4 rows x 5 panels = 100)
- Ensure the finish is one of HASL (Lead), HASL or ENIG - not OSP.
- Select assembly as normal, making sure you select top _and_ bottom for boards that require it, which is most of them.
- When asked whether your BOM is for a single PCB or all PCBs, select "single piece pls help me duplicate" or similar.  JLC then multiply the part quantities in your BOM by the number of One ROMs.
- Ensure _all_ parts are selected.
- Ensure the parts placement and rotation is correct for the single PCB - JLC will then duplicate the placement for all PCBs in the panel.
- Place your order.

When the panels arrive, separate the panels into individual PCBs by snapping them apart along the V-CUT lines, program, test and enjoy!

## Custom Panelisation

Use [`scripts/panelise.py`](scripts/panelise.py).  Each panel row is a single One ROM type.  All One ROM types within a panel must be identical widths.

For example for a 5 row, 8 column panel with 3 x fire-28-c4 rows and 2 x fire-28-d3 rows:

```
cd hardware/pcb/unverified
python3 ../scripts/panelise.py --cols 8 \
    --row fire-28-c4 --row fire-28-c4 --row fire-28-c4 \
    --row fire-28-d3 --row fire-28-d3 \
    --out /tmp/panel --name fire-28-c4-d3-panel
```

The outputs within `/tmp/panel/` are:
- `gerbers/`
- `fab/` for the the BOM and POS files

The script outputs the precise panel size - check that JLC detects this, or enter it manually.

To order:
- Upload the panel's gerbers zip and select the number of panels
- Select:
  - Panel by customer
  - The number of different designs
  - Tooling holes, edge rails and fiducials added by customer
  - Finish as above (not OSP)
- Select Standard PCBA and top _and_ bottom assembly if needed
- Ensure all parts are selected to be placed
- Check placement and rotation on every PCB in the panel, on both the bottom and top
