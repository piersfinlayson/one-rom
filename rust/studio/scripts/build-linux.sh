#!/bin/sh
set -e

# Builds the One ROM Studio deb packages for Linux (x86_64 and arm64), on
# either architecture.
#
# zig links both against the glibc version in ci/linux-min-glibc-version, so
# the packages install on that version or later whatever the build machine
# runs.
#
# Pre-requisites:
# - Rust:
#
# ```sh
#   curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs | sh
# ```

# Check we're running on Linux
if [ "$(uname -s)" != "Linux" ]; then
    echo "Error: This script must be run on Linux" >&2
    exit 1
fi

# Parse arguments
CLEAN=true
DEPS=true

for arg in "$@"; do
    case "$arg" in
        noclean)
            echo "!!! WARNING: Not cleaning cargo artifacts" >&2
            CLEAN=false
            ;;
        nodeps)
            echo "!!! WARNING: Skipping dependencies installation step" >&2
            DEPS=false
            ;;
        *)
            echo "Error: Unknown argument '$arg'" >&2
            echo "Usage: $0 [nosign] [noclean] [nodeps]" >&2
            exit 1
            ;;
    esac
done

#
# Setup
#

# Install required packages
if [ "$DEPS" = true ]; then
    echo "Installing dependencies..."

    # Clean up any Ubuntu repo files from previous runs
    sudo rm -f /etc/apt/sources.list.d/ubuntu-ports.list
    sudo rm -f /etc/apt/sources.list.d/ubuntu-amd64.sources
    sudo rm -f /etc/apt/sources.list.d/ubuntu-archive.list

    sudo apt update && sudo apt install -y curl dpkg-dev python3 xz-utils

    # Detect OS and host architecture
    if [ -f /etc/os-release ]; then
        . /etc/os-release
        OS_ID="$ID"
    else
        echo "ERROR: Cannot detect OS" >&2
        exit 1
    fi
    HOST_ARCH=$(dpkg --print-architecture)
    echo "Detected OS: $OS_ID, host architecture: $HOST_ARCH"

    # cargo-deb works out each package's libc6 dependency with dpkg-shlibdeps,
    # which reads the libc6 installed for that package's architecture.
    if [ "$OS_ID" = "debian" ]; then
        # On Debian, native repos support both architectures
        sudo dpkg --add-architecture arm64
        sudo dpkg --add-architecture amd64
        sudo apt update && sudo apt install -y libc6:arm64 libc6:amd64

    elif [ "$OS_ID" = "ubuntu" ]; then
        # On Ubuntu, architectures need different repos
        CODENAME=$(lsb_release -sc)

        # Restrict existing repos to native architecture before adding foreign arch
        if [ -f /etc/apt/sources.list.d/ubuntu.sources ]; then
            sudo cp /etc/apt/sources.list.d/ubuntu.sources /tmp/ubuntu.sources.backup
            sudo sed -i '/^Architectures:/d' /etc/apt/sources.list.d/ubuntu.sources
            sudo sed -i "/^Types:/a Architectures: ${HOST_ARCH}" /etc/apt/sources.list.d/ubuntu.sources
        fi

        if [ "$HOST_ARCH" = "amd64" ]; then
            # On x86_64 host: add arm64 packages from ports.ubuntu.com
            sudo dpkg --add-architecture arm64
            echo "Configuring ports.ubuntu.com for arm64 packages"
            echo "deb [arch=arm64] http://ports.ubuntu.com/ubuntu-ports ${CODENAME} main universe" | sudo tee /etc/apt/sources.list.d/ubuntu-ports.list
            echo "deb [arch=arm64] http://ports.ubuntu.com/ubuntu-ports ${CODENAME}-updates main universe" | sudo tee -a /etc/apt/sources.list.d/ubuntu-ports.list
            echo "deb [arch=arm64] http://ports.ubuntu.com/ubuntu-ports ${CODENAME}-security main universe" | sudo tee -a /etc/apt/sources.list.d/ubuntu-ports.list
            sudo apt update && sudo apt install -y libc6:arm64

        elif [ "$HOST_ARCH" = "arm64" ]; then
            # On arm64 host: add amd64 packages from archive.ubuntu.com
            sudo dpkg --add-architecture amd64
            echo "Configuring archive.ubuntu.com for amd64 packages"
            echo "deb [arch=amd64] http://archive.ubuntu.com/ubuntu ${CODENAME} main universe" | sudo tee /etc/apt/sources.list.d/ubuntu-archive.list
            echo "deb [arch=amd64] http://archive.ubuntu.com/ubuntu ${CODENAME}-updates main universe" | sudo tee -a /etc/apt/sources.list.d/ubuntu-archive.list
            echo "deb [arch=amd64] http://security.ubuntu.com/ubuntu ${CODENAME}-security main universe" | sudo tee -a /etc/apt/sources.list.d/ubuntu-archive.list
            sudo apt update && sudo apt install -y libc6:amd64
        fi

    else
        echo "ERROR: Unsupported OS: $OS_ID" >&2
        exit 1
    fi

    # Install the Rust targets
    rustup target add x86_64-unknown-linux-gnu
    rustup target add aarch64-unknown-linux-gnu

    # Install cargo-deb if not already installed
    # Forced to ensure it builds against glibc version on this system
    cargo install cargo-deb --locked --force
else
    echo "Skipping dependencies installation step."
fi

# Runs with nodeps too, since the build requires zig on PATH.  It installs
# nothing when the pinned versions are already present.
ZIG_BIN="$(../../ci/install-zig.sh)"
export PATH="${ZIG_BIN}:${PATH}"
# cargo-zigbuild uses a Python ziglang package ahead of zig on PATH
export CARGO_ZIGBUILD_ZIG_PATH="${ZIG_BIN}/zig"

MIN_GLIBC="$(tr -d '[:space:]' < ../../ci/linux-min-glibc-version)"

#
# Clean previous builds
#

if [ "$CLEAN" = true ]; then
    echo "Cleaning previous build artifacts..."
    cargo clean --target x86_64-unknown-linux-gnu
    cargo clean --target aarch64-unknown-linux-gnu
    rm -fr dist/*.deb
else
    echo "Skipping cleaning of previous build artifacts."
fi

mkdir -p dist

#
# Build and package
#

for TARGET in x86_64-unknown-linux-gnu aarch64-unknown-linux-gnu; do
    echo "Building for target: $TARGET, glibc $MIN_GLIBC"
    cargo zigbuild --bin onerom-studio --release --target "$TARGET.$MIN_GLIBC"

    echo "Packaging deb for target: $TARGET"
    cargo deb -v --no-build --target "$TARGET" --output dist/
done

echo "Build complete."
