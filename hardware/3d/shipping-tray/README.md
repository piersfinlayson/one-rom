# One ROM Shipping Tray

A base plate and top plate that hold cased One ROMs upright for shipping. Each One ROM stands cap down in the base plate and the top plate fits over the other end of the case. A cable tie through each pair of tabs holds the plates together.

The plates fit every One ROM case with header pins. All One ROMs in one tray must be the same size.

To generate the plates for 10 One ROMs:

```sh
./make-tray.sh 10
```

or to choose the grid, in columns by rows:

```sh
./make-tray.sh 10 2x5
```

This writes `tray-10-base.stl` and `tray-10-top.stl` to the current directory. The script requires [OpenSCAD](https://openscad.org/). Without a grid, the most compact grid that fits an Original Prusa MK3 bed is used. One plate holds up to 48.

Print both plates with the pockets facing up. Turn the top plate over to fit it. If it doesn't fit, turn it round.
