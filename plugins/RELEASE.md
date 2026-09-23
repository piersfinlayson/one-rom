# Releasing plugins

The following script builds a plugin and copies the binary to the `dist` directory.

```bash
scripts/build-release.sh system/usb
```

## Building Third-party plugins

To build a third-party plugin against this version of the firmware:
- Copy the plugin directory into `plugins/<type>/<name>`.
- Run `make generated` at the root.
- Build it with the pinned toolchain (see `ci/arm-toolchain-version` and `ci/install-arm-toolchain.sh`).

```bash
TOOLCHAIN=$HOME/arm-gnu-toolchain/arm-toolchain/bin scripts/build-release.sh user/<name>
```

Then release it as below.

The following script places the binaries in the `one-rom-images` repo at the provided path, and updates the plugin manifests there.

```bash
scripts/release.py --input-dir dist --output-dir ../../one-rom-images
```

Now cd to the `one-rom-images` repo, and commit the changes to the plugin binaries and manifests.

```bash
cd ../../one-rom-images
git add plugins/*
git commit -m "Update plugin binaries and manifests"
git push
```

For a third-party plugin, name the commit its tag pointed to when you reply on
its issue.  A tag can be moved later.

For an in-tree plugin, tag the release in `one-rom`.  Tags are signed, so they take a message:

```bash
cd ../one-rom
git tag -s -a plugin-system-usb-v0.1.0 -m "USB system plugin v0.1.0"
git push origin plugin-system-usb-v0.1.0
```
