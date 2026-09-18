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
  - [metadata](/rust/metadata/Cargo.toml)
  - [app](/rust/app/Cargo.toml)
  - [cli](/rust/cli/Cargo.toml)
- If the firmware metadata/image format version has changed, update the
  `MAX_VERSION_*` consts in [rust/fw-parser/src/lib.rs](/rust/fw-parser/src/lib.rs).

## Release Process

Ensure all changes are committed, including the [version number updates](#update-version-number).

```bash
git pull
git push
```

Locally run the following.  `ci/test-emu.sh` is run by CI on every push, so it
can be left out here:

```bash
ci/test-emu.sh
ci/build.sh ci
ci/build.sh release v<x.y.z>
```

---

Publish the crates whose version moved this cycle.  The CHANGELOG's "To
publish" list says which.

`onerom-config` publishes first, on its own, with `--no-verify`.  Its build
script writes into `src/` and `docs/CHIP-TYPES.md`, which the verification build
rejects.  Add `--dry-run` to rehearse:

```bash
cargo publish --manifest-path rust/Cargo.toml --no-verify -p onerom-config
```

The rest publish together, verified.  Name each crate that moved:

```bash
cargo publish --manifest-path rust/Cargo.toml --dry-run -p <crate> -p <crate>
```

```bash
cargo publish --manifest-path rust/Cargo.toml -p <crate> -p <crate>
```

The CLI **binary** releases on its own cycle, following
[rust/cli/README.md](/rust/cli/README.md).  The CLI manual PDF is published by
that release rather than this one, since the manual moves with the CLI version.

---

If on a branch, submit a pull request and merge it into main.

## Plugins

Build and release any plugins whose version changed this cycle, following
[plugins/RELEASE.md](/plugins/RELEASE.md).  Build them individually rather than
with `build-release-all.sh` unless every plugin is being released, since that
script stages every plugin carrying a `plugin-meta.json`.

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
    `one-rom-images/releases.json`.  The script leaves `latest` alone - see
    [The `latest` fields](#the-latest-fields)
  - Ensure the image exists at `one-rom-images/vx.y.z/fire/rp2350/firmware.bin`
  - Commit and push changes to `one-rom-images` repo
  - Test using Studio
- Update the documentation PDFs in `one-rom-images`
  - Install the documentation toolchain once with `ci/install-doc-tools.sh`, and
    put the directory it prints on `PATH`
  - Within `one-rom` `main` branch run
    `ci/build-docs.sh ../one-rom-images --source firmware`
  - `--source firmware` builds only the documents on the firmware's release
    cycle - the chip compatibility and chip type references.  The CLI manual is
    published by the CLI release instead, since it moves with the onerom-cli
    crate.  Without it the whole set is built, which would republish the manual
    at a version that had not moved
  - The script stages each document under `one-rom-images/docs/<slug>/v<x.y.z>/`
    and merges the release into that document's `releases.json`, keeping every
    past edition so a reader on an older firmware can still fetch the one
    matching their build.  It leaves `latest` alone - see
    [The `latest` fields](#the-latest-fields)
  - Commit and push changes to `one-rom-images` repo

## The `latest` fields

Several manifests in `one-rom-images` carry a `latest`.  Three scripts move it
as part of staging:

| Manifest | Moved by |
| --- | --- |
| `plugins/<type>/<name>/releases.json` | `plugins/scripts/release.py` |
| `cli/releases.json` | `rust/cli/scripts/release.py` |
| `studio/releases.json` | `rust/studio/scripts/release.py` |

These five are hand edits, made once everything is published and serving:

| Manifest | Field | Moves to |
| --- | --- | --- |
| `releases.json` | `latest` | the firmware version |
| `docs/chip-types/releases.json` | `latest` | the firmware version |
| `docs/compatibility/releases.json` | `latest` | the firmware version |
| `docs/cli-manual/releases.json` | `latest` | the onerom-cli version |
| `studio.json` | `latest_app_version` | the Studio version |

`studio.json` also carries a `revision`, which increments whenever anything in
that file changes.

`ci/build-images.sh` and `ci/build-docs.sh` stage their files and leave `latest`
as it stands, so the release can be assembled and pushed before it becomes the
version a client fetches.
