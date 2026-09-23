#!/bin/sh
set -e

# Builds One ROM Studio for macOS, with a single binary for both architectures
#
# Required env variables:
# - CODESIGN_IDENTITY: The codesign identity to use for signing the app.
#   This is the "name" of the certificate including "Developer ID ...".
#   Only required if not building with "nosign" option.
#
# The notarytool uses a keychain profile named "onerom-notarization" for
# notarization. This profile must be set up in advance with the appropriate
# Apple ID and app-specific password using a command line:
#
# ```sh
# xcrun notarytool store-credentials "onerom-notarization" \
#  --apple-id your@email.com \
#  --team-id XXXXXXXXXX \
#  --password xxxx-xxxx-xxxx-xxxx
# ```
#
# - Team ID matches that on the Developer ID Application certificate.
# - The app-specific password is generated in the Apple ID account settings.
#
# Args:
# - nosign: Build without code signing or notarization.
# - noclean: Skip the cargo clean step (still removes the dmg).
# - nodeps: Skip the dependencies installation step.

#
# Checks
#

# Check we're running on macOS
if [ "$(uname -s)" != "Darwin" ]; then
    echo "Error: This script must be run on macOS" >&2
    exit 1
fi

# Parse arguments
SIGN=true
CLEAN=true
DEPS=true

for arg in "$@"; do
    case "$arg" in
        nosign)
            echo "!!! WARNING: Building without code signing or notarization" >&2
            SIGN=false
            ;;
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

# Check codesign identity if signing is enabled
if [ "$SIGN" = true ] && [ -z "${CODESIGN_IDENTITY:-}" ]; then
    echo "Error: CODESIGN_IDENTITY environment variable must be set"
    exit 1
fi

#
# Variable
#

# Distribution directory - DMG goes here
DIST_DIR="dist"

# Get version from Cargo.toml
VERSION=$(grep "^version" Cargo.toml | head -1 | sed 's/.*"\(.*\)".*/\1/')

# Set DMG filename
DMG_FILE="One ROM Studio_${VERSION}.dmg"
DMG_PATH="${DIST_DIR}/${DMG_FILE}"

#
# Dependencies
#

if [ "$DEPS" = true ]; then
    echo "Installing dependencies..."
    # Install the Rust targets
    echo "Installing Rust targets..."
    rustup target add x86_64-apple-darwin
    rustup target add aarch64-apple-darwin

    # Install cargo-bundle, pinned because 0.12.0 rejects the
    # osx_minimum_system_version key in Cargo.toml
    cargo install cargo-bundle --version 0.11.0 --locked

    # Install fileicon if not already installed
    brew install fileicon

    # Install python pip packages
    python3 -m pip install --upgrade pip || true # Ensure supports --break-system-packages
    python3 -m pip install --break-system-packages -r scripts/requirements.txt
else
    echo "Skipping dependencies installation step."
fi

#
# Clean previous builds
#

echo "Cleaning previous build artifacts..."

# Do a Cargo clean
if [ "$CLEAN" = true ]; then
    echo "Running cargo clean..."
    cargo clean --target x86_64-apple-darwin
    cargo clean --target aarch64-apple-darwin
else
    echo "Skipping cargo clean step."
fi

# Delete old DMG
if [ -f "${DMG_PATH}" ]; then
    echo "Removing old dmg: ${DMG_PATH}"
    rm -f "${DMG_PATH}"
fi

#
# Build
#

echo "Building One ROM Studio version: $VERSION"
echo "Building dmg: $DMG_PATH"

# Build for Intel
PACKAGER_TARGET="x86_64-apple-darwin"
echo "Building One ROM Studio for target: $PACKAGER_TARGET"
cargo build --release --target $PACKAGER_TARGET

# Build for ARM64
PACKAGER_TARGET="aarch64-apple-darwin"
echo "Building One ROM Studio for target: $PACKAGER_TARGET"
cargo build --release --target $PACKAGER_TARGET

# Package as a .app bundle - do this using ARM64 target for now
echo "Bundling app for target: $PACKAGER_TARGET"
cargo bundle --bin onerom-studio --release --target $PACKAGER_TARGET
APP_FILE="../target/$PACKAGER_TARGET/release/bundle/osx/One ROM Studio.app"
echo "Built app file: $APP_FILE"

# Set the liquid glass icon
echo "Setting liquid glass icon..."
cp -pr assets/onerom-liquid-glass-icons/Assets.car "$APP_FILE/Contents/Resources/"
/usr/libexec/PlistBuddy -c "Add :CFBundleIconName string onerom-liquid-glass" "$APP_FILE/Contents/Info.plist"

# Replace App binary with a universal binary
lipo -create \
  ../target/x86_64-apple-darwin/release/onerom-studio \
  ../target/aarch64-apple-darwin/release/onerom-studio \
  -output "$APP_FILE/Contents/MacOS/onerom-studio"

# Check if signing is enabled
if [ "$SIGN" = true ]; then
    # Sign the app
    echo "Signing app..."
    codesign --deep --verify --options runtime --timestamp --sign "$CODESIGN_IDENTITY" "$APP_FILE"

    # Verify the signature
    echo "Verifying signature..."
    codesign -dvv "$APP_FILE" 2>&1 | grep "Authority=Developer ID Application"
    if [ $? -ne 0 ]; then
        echo "Error: App signature verification failed" >&2
        exit 1
    fi
fi

# Create the dmg
echo "Creating dmg..."
scripts/create-dmg.py \
    --app-bundle "$APP_FILE" \
    --output "${DMG_FILE}" \
    --dist-dir ${DIST_DIR}

# Check if signing is enabled
if [ "$SIGN" = true ]; then
    # Notarize the dmg
    echo "Notarizing dmg..."
    xcrun notarytool submit "$DMG_PATH" \
        --keychain-profile "onerom-notarization" \
        --wait

    # Staple the notarization ticket to the dmg
    echo "Stapling dmg..."
    xcrun stapler staple "$DMG_PATH"

    # Validate the stapled ticket
    echo "Validating stapled ticket..."
    xcrun stapler validate "$DMG_PATH"

    # Finished
    echo "Built and notarized dmg: $DMG_PATH"
else
    echo "Built dmg without notarization: $DMG_PATH"
fi
