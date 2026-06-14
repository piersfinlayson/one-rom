# onerom-fw-emulator

This is a crate that provides the One ROM firmware as a library for running on hosts, emulating the underlying hardware, including PIOs and DMAs.

Sample invocation from this crate's directory:

```bash
BASE_DIR=$(realpath ../..) CONFIG=onerom-config/test-0.json BOARD=fire-24-a cargo test
```