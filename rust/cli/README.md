# One ROM CLI

This directory contains the source code and build scripts for the One ROM command-line interface (CLI) tool.

The CLI is a Rust application that provides a cross-platform CLI to manage One ROMs, including those with the USB plugin to provide management of One ROM while running (serving bytes).

For instructions on using the CLI, see [The CLI manual](/docs/CLI-MANUAL.md).

## Building

With the One ROM [build environment](/INSTALL.md) installed, from this directory:

```bash
cargo build --release
```

## Releasing

1. Build the release artifacts for all platforms:

    ```bash
    scripts/build-release.sh pin=WIN_SIGNING_PIN
    ```

    Artifacts are placed in the `dist/` directory.

2. Update the CLI release manifest and copy the artifacts to the One ROM images repo.  Assuming ../../../one-rom-images exists:

    ```bash
    scripts/release.py --input-dir dist --output-dir ../../../one-rom-images
    ```

3. Commit the changes to the one-rom-images repo and push:
    ```bash
    cd ../../../one-rom-images
    git add .
    git commit -m "Update One ROM CLI releases"
    git push
    ```

4. Tag the current commit with the version and push:

    ```bash
    git tag cli-vX.Y.Z
    git push origin cli-vX.Y.Z
    ```

5. Check new releases appear at https://onerom.org/cli/

6. Consider whether the [CLI Manual](/docs/CLI-MANUAL.md) needs updating for the new release.