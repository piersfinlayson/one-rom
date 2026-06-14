# onerom-fw-tester

This crate provides a test harness for the One ROM firmware.  It utilises `onerom-fw-emulator` to build and run the firmwar on a host system, and drives the GPIOs via `epio` to ensure that the emulated firmware correctly serves the configured ROMs.