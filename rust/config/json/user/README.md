# User Hardware Configs

Use this directory for your own hardware configs.  Given them lower-case names, and use dashes (-), not underscores (_).

[`sdrr-gen`](/rust/gen/README.md) looks here for hardware configuration files, as well as the directory above, and [`third-party/`](../third-party/) and use them to generate the firmware for your hardware if selected.

This directory is in the project [`.gitignore`](/.gitignore) file, so files added here will not be committed to the repository by default.
