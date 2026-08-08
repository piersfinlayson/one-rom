# One ROM Build Container

This directory contains the files to build a One ROM build container.

It supports Linux, macOS, and Windows hosts (x86_64 and arm64). Docker Desktop automatically provides the Linux VM on macOS and Windows.

Dependencies are installed in the container, so you do not need to install them on your host system.

The container is a complete One ROM build environment: use it to build the firmware, build the host tooling (CLI, Studio, Lab), and run the tooling.

From v0.7.0 the firmware is a single base image covering all Fire boards (Ice is no longer supported). Building a firmware for a specific board and ROM configuration is a tooling step - use the CLI, as shown below.

Example usage - build the base firmware:

```bash
mkdir ./output
docker run --rm -v $(pwd)/output:/home/build/output \
    --name onerom-build ghcr.io/piersfinlayson/onerom-build:latest \
    sh -c './clone.sh && \
            cd one-rom && \
            make && \
            ../copy-fw.sh'
```

The firmware is now available in `./output/` on your host:

```bash
$ ls -l ./output
total 1568
-rw-r--r--  1 pdf  wheel  262144 Jan 31 18:16 onerom-rp235x.bin
-rwxr-xr-x  1 pdf  wheel  398168 Jan 31 18:16 onerom-rp235x.elf
-rw-r--r--  1 pdf  wheel  524288 Jan 31 18:16 onerom-rp235x.uf2
```

## Building One ROM

To build One ROM using the build container, you have two options:

* Use the container in the foreground, running the build commands directly.
* Use the container in the background, running each command via `docker exec`.

### Foreground

This example:

* creates a `./output/` directory on your host for the built firmware
* clones the One ROM repo
* builds the base One ROM firmware
* copies the built firmware to the mounted output directory on your host
* exits the container (which is then auto-deleted).

```bash
mkdir -p ./output
docker run -it --rm -v $(pwd)/output:/home/build/output --hostname onerom-build --name onerom-build ghcr.io/piersfinlayson/onerom-build:latest bash
./clone.sh
cd one-rom
make
../copy-fw.sh
exit
```

You can now retrieve the built firmware from your host at `./output/`.

The first time you run `make` it will take a while to build all of the Rust toolchain components.  Subsequent builds using the same container will be much faster.

### Background

This example is similar to the foreground example, but runs the container in the background and uses `docker exec` to run commands inside the container.

```bash
mkdir -p ./output
docker run -d -v $(pwd)/output:/home/build/output --name onerom-build ghcr.io/piersfinlayson/onerom-build:latest
docker exec -it onerom-build ./clone.sh
docker exec -it onerom-build sh -c 'cd one-rom && make && ../copy-fw.sh'
```

You can now retrieve the built firmware from your host at `./output/`.

When finished, stop and remove the container:

```bash
docker stop onerom-build
docker rm onerom-build
```

### Building a Configured Firmware

The steps above build the base firmware. To build a flashable firmware for a specific board with a ROM configuration, build and run the CLI - this is a tooling step, not a firmware compile:

```bash
cd rust/cli
cargo build --release
./target/release/onerom firmware build \
    --config-file ../../onerom-config/vic20-pal.json \
    --board fire-24-e \
    --out /home/build/output/firmware.bin
```

The CLI can do much more (inspecting, programming, and managing a connected One ROM); run `onerom --help` for the full command set.

## Programming One ROM

The easiest ways of programming One ROM using the firmware built in this container are:

* [One ROM Studio](https://onerom.org/studio)
* [One ROM Web](https://onerom.org/web)

Both tools allow you to select locally built firmware files to program to your One ROM.

However, it is possible to use `probe-rs` and `picotool` from within the container by giving the container access to the required hardware devices.  Use `--privileged` or `--device` flags to give the container access to USB devices.

## Building the Container

### Local Use

To build the `onerom-build` container for local use only, from the repo root run:

```bash
ci/docker/build.sh
```

The script takes an optional version argument, and also tags the container `latest`.

### Release Version

To build and push a multi-architecture version of the `onerom-build` container to the GitHub Container Registry, use buildx.

#### Setting up buildx - Optional

The script that follows sets up a buildx configuration called `onerom-multiarch` automatically, if it doesn't already exist.  This uses qemu locally to emulate non-native architectures.

However, if you'd like to use another system for building a non-native architecture (i.e. a remote host that is natively that architecture) you can do that like this:
- configure `LOCAL_ARCH` with the _local_ architecure (e.g. `linux/arm64`, when run on an Apple Silicon Mac, as `arm64` is natively supported by docker)
- configure `REMOTE_ARCH` with the _remote_ architecture (e.g. `linux/amd64`, when using a remote `x86_64` linux host)
- configure `SSH_CREDS` with your SSH details to the remote host:

```bash
export LOCAL_ARCH=linux/arm64
export REMOTE_ARCH=linux/amd64
export SSH_CREDS=user@host
docker buildx create --name onerom-multiarch --platform $LOCAL_ARCH --use
docker buildx create --append --name onerom-multiarch --platform $REMOTE_ARCH ssh://$SSH_CREDS
```

If you are natively using a `x86_64` linux host, and remotely building on an `arm64` host like a Rasperrby Pi, switch over the values of `LOCAL_ARCH` and `REMOTE_ARCH` above.

Once created, inspect the builder to ensure both platforms are listed:

```bash
docker buildx inspect onerom-multiarch
```

#### Building and Pushing

To build and push the container, run:

```bash
ci/docker/build-release.sh <version>
```

This uses docker buildx to build multi-architecture images for `x86_64` and `arm64`, and pushes them to GitHub Container Registry.  As well as `version`, it also tags the image as `latest`.

#### GitHub Container Registry Credentials

To push to GitHub Container Registry, you need to be authenticated.  The easiest way to do this is to create a personal access token (classic type) with Write and Read Package permissions.

Then, for macOS, ensure that `~/.docker/config.json` looks like this

```json
{
        "auths": {
                "ghcr.io": {}
        },
        "credsStore": "osxkeychain",
        "currentContext": "desktop-linux"
}
```

Unlock your keychain like this:

```bash
security unlock-keychain ~/Library/Keychains/login.keychain-db
```

And login like this:

```bash
docker login ghcr.io -u username
```

When prompted for a password, provide your personal access token.

Your credentials should now have been stored in your macOS keychain, and should be retrieved automatically by docker when pushing to GitHub Container Registry.

## Building Other Tools

As well as the core One ROM firmware, this container can be used to build other tools, such as One ROM Lab.  See their respective READMEs for more information.

## Building One ROM Studio

### Linux Version

You can build the linux version of One ROM Studio using this container.  To build for your host's architecture:

```bash
cd rust/studio
cargo build --release
```

### Windows Version

It is possible to build a GNU Windows version of One ROM Studio using this container, by specifying the appropriate target.  For example:

```bash
cd rust/studio
cargo build --release --target x86_64-pc-windows-gnu
```

This _does not_ create the Windows version of Studio which is available to download at https://onerom.org/studio, which is built on Windows using the MSVC target, and also creates the installer.  Building this on linux is extremely involved and not recommended, as it requires the Windows SDK.

The Windows `arm64` version is also difficult to build on linux and not recommended.

### macOS Version

You cannot build the macOS version of One ROM Studio using this container, as it requires Xcode and macOS SDKs.

### Release Builds

For production releases a triple setup of Windows, macOS, and Linux (virtual) machines is required, as well as credentials for signing on Windows and macOS.

See `/rust/studio/README.md` for more information.

