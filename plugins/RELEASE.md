# Releasing plugins

The following script discovers all plugins with a `plugin-meta.json` file, and builds them.  It then copies the built plugin binaries to the `dist` directory.

```bash
scripts/build-release-all.sh
```

Alternatively, to just release a single plugin:

```bash
scripts/build-release.sh system/usb
```

## Building Third-party plugins

To build a third-party plugin against this version of the firmware:
- Copy the plugin directory into `plugins/<type>/<name>`.
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

Tag the release with the new version number, and push the tag.  For example:

```bash
git tag plugin-system-usb-v0.1.0
git push origin plugin-system-usb-v0.1.0
```