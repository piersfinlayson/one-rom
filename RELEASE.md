# Releasing New Version of One ROM

## Update Version Number

To update the version:

- Add the new version to [CHANGELOG.md](CHANGELOG.md), and note key changes.
- Update the firmware version in [Makefile](/Makefile).
- Bump the version (as needed) of any crate that changed and will be
  re-published, in its `Cargo.toml`. The crates are independently versioned; the
  publishable ones are:
  - [config](/rust/config/Cargo.toml)
  - [database](/rust/database/Cargo.toml)
  - [gen](/rust/gen/Cargo.toml)
  - [fw-parser](/rust/fw-parser/Cargo.toml)
  - [fw](/rust/fw/Cargo.toml)
  - [protocol](/rust/protocol/Cargo.toml)
  - [lab](/rust/lab/Cargo.toml)
- If the firmware metadata/image format version has changed, update the
  `MAX_VERSION_*` consts in [rust/fw-parser/src/lib.rs](/rust/fw-parser/src/lib.rs).

## Release Process

Ensure all changes are committed, including the [version number updates](#update-version-number).

```bash
git pull
git push
```

Locally run the following tests:

```bash
ci/test-emu.sh
ci/build.sh ci
ci/build.sh release v<x.y.z>
```

---

Publish `onerom-database` to crates.io:

```bash
cd rust
cargo publish --dry-run -p onerom-database
cargo publish -p onerom-database
```

Update link to `onerom-database` in [protocol/Cargo.toml](/rust/protocol/Cargo.toml) to use the crates.io version.

---

Publish `onerom-protocol` to crates.io:

```bash
cd rust
cargo publish --dry-run -p onerom-protocol
cargo publish -p onerom-protocol
```

Update links to `onerom-database` and `onerom-protocol` in [lab/Cargo.toml](/rust/lab/Cargo.toml) to use the crates.io versions.

---

Publish `onerom-config` to crates.io:

```bash
cd rust
cargo publish --dry-run -p onerom-config
cargo publish -p onerom-config
```

Update links to and `onerom-config` in others to use the crates.io versions.

---

Publish `onerom-gen` to crates.io:

```bash
cd rust
cargo publish --dry-run -p onerom-gen
cargo publish -p onerom-gen
```

Update links to and `onerom-gen` in others to use the crates.io versions.

---

Publish the new version of `onerom-fw-parser` to crates.io:

```bash
cd rust
cargo publish --dry-run -p onerom-fw-parser
cargo publish -p onerom-fw-parser
```

---

Publish `onerom-fw` to crates.io:

```bash
cd rust
cargo publish --dry-run -p onerom-fw
cargo publish -p onerom-fw
```

Update links to and `onerom-fw` in others to use the crates.io versions.

---

If on a branch, submit a pull request and merge it into main.

Tag the version in git:

```bash
git tag -s -a v<x.y.z> -m "Release v<x.y.z>"
git push origin v<x.y.z>
```

## WASM/Site/Images updates

- Copy `onerom-config/schema.json` to `one-rom-images/configs/schema.json` and commit/push.
- Update `onerom-wasm` to the new onerom-fw-parser/onerom-gen version if required and release.
  - Make sure to bump the wasm development version releasing
  - Check release appears in wasm/releases, and new dev version on homepage
- Update `one-rom-site` to use the new `onerom-wasm` version, test and release
  - Ensure can read/write firmware correctly using web programmer
  - Ensure the new firmware version appears in the web programmer's Custom
    (build-your-own) flow - it lists versions from
    `images.onerom.org/releases.json`
  - Note: the web programmer's Pre-built flow lists versions from
    `one-rom/releases/releases.json`, which is frozen at v0.6.x. From v0.7.0 no
    per-board pre-built images are produced, so v0.7.0+ will not appear there -
    this is expected.
- Update releases in `one-rom-images`
  - From v0.7.0 there is a single base firmware for all Fire boards (no Ice)
  - Within `one-rom` `main` branch run `ci/build-images.sh x.y.z ../one-rom-images`
  - Paste the new release manifest fragment (from `/tmp/releases.json`) into
    `one-rom-images/releases.json` and update `latest`
  - Ensure the image exists at `one-rom-images/vx.y.z/fire/rp2350/firmware.bin`
  - Commit and push changes to `one-rom-images` repo
  - Test using Studio
