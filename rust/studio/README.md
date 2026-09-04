# onerom-studio

A GUI front-end for interacting with One ROM and managing firmware images.

## Releasing

All instructions assume you are in the `rust/studio` directory.  Steps 1-3 can be run in parallel.

Steps:
1. Build the schema(s):

    ```bash
    cargo run --release --bin gen-schema
    cargo run --release --bin gen-manifest
    git diff manifest/app-schema.json ../../../one-rom-images/studio/app-schema.json
    # If differences
    cp manifest/app-schema.json ../../../one-rom-images/studio/app-schema.json
    ```

2. Use the `build-release.sh` script to build Studio for all platforms and architectures.  This requires ssh access to build machines for each platform and clones the current main branch to build from.  To set up the Windows machine, see [Setting up a Windows build machine](/docs/SETUP-WINDOWS-BUILD-MACHINE.md):

    ```bash
    scripts/build-release.sh pin=WIN_SIGNING_PIN
    ```

    Artifacts are placed in the `dist/` directory.

3. Run the release script to upload the files and update the manifest - assumes you have the images.onerom.org github repo at ../../../one-rom-images:

    ```bash
    scripts/release.py --input-dir dist --output-dir ../../../one-rom-images
    ```

4. Commit one-rom-images changes and push:

    ```bash
    cd ../../../one-rom-images
    git add .
    git commit -m "Update One ROM Studio releases"
    git push
    ```

5. Tag the current commit with the version and push:

    ```bash
    git tag studio-vX.Y.Z
    git push origin studio-vX.Y.Z
    ```

6. Check new releases appear at https://onerom.org/studio/

7. Update the Studio manifest `one-rom-images/studio.json` with the latest version.

## Updating Mac Icons

1. From an up to date Apple Silicon Mac, open `assets/onerom-liquid-glass.icon`.

2. Modify the icon as needed using the Icon Composer application.

3. Regenerate `assets/onerom-liquid-glass-icons/Assets.car` using:

    ```bash
    scripts/create-mac-icon-ac.sh
    ```

4. Commit and push both `assets/onerom-liquid-glass.icon` and `assets/onerom-liquid-glass-icons/Assets.car`.

## Creating a new Code Signing Certificate

See https://piers.rocks/2025/10/30/certum-open-source-code-sign.html

## CI

`.github/workflows/build-studio.yml` contains the CI instructions for building Studio.  However, it does not sign, so do not use the produced installers for release.

This CI runs on all pushes to the `rust/studio/`.

## 

## Dependencies

Assuming building Windows target on a Debian-based Linux distribution.

For building linux targets:

```bash
sudo apt install libudev-dev
```

Install the Rust Windows targets:

```bash
rustup target add x86_64-pc-windows-gnu
```

Install ming-w64 for Windows builds:

```bash
sudo apt install mingw-w64
```

To run th ARM64 Linux build you really need to be building on the platform (or using CI), as libudev is a pain to build as part of the cross compilation.

## Packaging Implementation

### Windows

Uses `cargo-packager` - see the following in `Cargo.toml`:

- `[package.metadata.packager]`
- `[package.metadata.packager.nsis]`

Code signing is done using `signtool.exe` called from `scripts/build-windows.ps1`.

### macOS

Uses `cargo-bundle` to create the macOS app bundle - see the following in `Cargo.toml`:

- `[package.metadata.bundle]`

`scripts/build-mac.sh` does code signing and notarization using `codesign` and `xcrun notarytool`.

`scripts/create-dmg.py` is used by `build-mac.sh` to create DMG installer.

### Linux

Uses `cargo-packager` to create a `.deb` package - see the following in `Cargo.toml`:

- `[package.metadata.packager]`
- `[package.metadata.packager.linux]`
- `[package.metadata.packager.deb]`

`scripts/build-linux.sh` builds the package, including adding maintainer scripts to handle libudev rules installation.
