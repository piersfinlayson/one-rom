# Image Selection

This article describes stock image switching, which allows different images, or sets of images, to be selected __at boot time__ using the image select jumpers.  For multi-ROM sets and bank switchinging see [Multiple ROM Sets](/docs/MULTI-ROM-SETS.md).

As above, up to 16 ROM images can be stored on One ROM (although the number varies by ROM type and One ROM pin count) and selected at boot time using the jumpers on the PCB.  The One ROM image select jumpers are labelled om the underside `1`, `2`, `4` and `8`, or `A`, `B`, `C` and `D`.  The jumper being open is a `0` and closed is a `1`.

Do not confuse the image select jumpers with the BOOTSEL/BOOT0 jumper, labelled `B0` or `B`.  This is provided to [recover a One ROM a programmer cannot find](/docs/CLI-MANUAL.md#recovering-a-one-rom-the-cli-cannot-find).

The images with the One ROM firmware are numbered from 0 onwards, so, to select image 5 (the 6th image), you would close jumpers `1` and `4`, or `A` and `C`.

If you select a higher image number than there are images installed, One ROM will wrap back to the first image and keep counting.  So, if you only have 4 images installed (as numbers 0-3), a jumper setting of 4 will select image 0, a jumper setting of 5 will select image 1, etc.

You must reboot the One ROM device after changing the jumper settings, to cause One ROM to load the new image and settings, and start serving it.  You may need to physically power your retro system off and on again to do this - a reset may not be enough if power is not removed.

If you are developing a ROM image and reflashing it, it is recommended to use the same image index on a reflash, so when the One ROM device resets after reflashing, the correct image will be reloaded automatically.  This avoids the need for a host device power cycle (although a host device reset may be required, depending on what type of ROM you are providing).
