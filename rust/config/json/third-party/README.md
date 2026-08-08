# Third-Party Hardware Configs

This directory contains unofficial SDRR hardware configuration files that have been contributed by the community.

[`sdrr-gen`](/rust/gen/README.md) looks here for hardware configuration files, as well as the directory above, and [`user/`](../user/) and use them to generate the firmware for your hardware if selected.

This directory is in the project [`.gitignore`](/.gitignore) file, so files added here will not be committed to the repository by default.

Ensure any hardware configuration files are given lower-case names, and use dashes (-), not underscores (_).
