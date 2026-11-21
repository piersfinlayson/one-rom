#!/bin/bash

# Builds One ROM Studio for macOS, Linux, and Windows serially on separate
# build machines.
#
# Assumes SSH key-based authentication is set up for the build machines using
# the host's username.
#
# Runs clean builds, and signs on the appropriate platforms - again, assumes
# signing credentials are already set up on the build machines.
#
# There is a single manual step - the macOS keychain must be unlocked.  This is
# done just before the macOS build starts, and the keychain is locked again
# afterwards.  This happens early in the process, so the remainder of the build
# can proceed unattended.
#
# This script runs each platform build in sequence, for simplicity, and because
# it is currently run on a single host, with platform specific VMs (so
# parallisation provides little benefit).
#
# Usage:
#   ./build-release.sh pin=XXXX
# where XXXX is the Windows code signing smartcard PIN.

set -e  # Exit on any error during setup phase

# Build machine hostnames
MACOS_HOST="macmini"
LINUX_HOST="ubuntu-25-10-arm64"
WINDOWS_HOST="windows-11-arm64"

REPO_URL="https://github.com/piersfinlayson/one-rom.git"
BUILD_DIR="builds/one-rom-build"
STUDIO_DIR="$BUILD_DIR/rust/studio"

# Check for required Windows signing PIN
if [ -z "$1" ]; then
  echo "Error: Windows signing PIN required"
  echo "Usage: $0 pin=XXXX"
exit 1
fi
if [[ ! "$1" =~ ^pin=[0-9]+$ ]]; then
  echo "Error: Invalid PIN format. Must be pin=XXXX where XXXX is digits only"
  echo "Usage: $0 pin=XXXX"
  exit 1
fi
WINDOWS_PIN="$1"

echo "=== Testing SSH connectivity ==="
echo "Testing connection to $MACOS_HOST..."
ssh -o ConnectTimeout=5 "$MACOS_HOST" exit

echo "Testing connection to $LINUX_HOST..."
ssh -o ConnectTimeout=5 "$LINUX_HOST" exit

echo "Testing connection to $WINDOWS_HOST..."
ssh -o ConnectTimeout=5 "$WINDOWS_HOST" exit

echo "All hosts reachable"

echo ""
echo "=== Cleaning local dist directory ==="
rm -f dist/*

echo ""
echo "=== Setting up builds on Unix hosts ==="
for host in "$MACOS_HOST" "$LINUX_HOST"; do
  echo ""
  echo "Setting up $host..."
  echo ""
  ssh "$host" bash << 'EOF'
set -e
if [ ! -d builds/one-rom-build ]; then
  echo "Cloning repository..."
  mkdir -p builds
  cd builds
  git clone https://github.com/piersfinlayson/one-rom.git one-rom-build
  cd ..
fi
cd builds/one-rom-build
echo "Updating repository..."
git checkout main
git fetch origin
git reset --hard origin/main
cd rust/studio
echo "Cleaning dist directory..."
rm -f dist/*
EOF
done

echo ""
echo "=== Setting up build on $WINDOWS_HOST ==="
echo ""
# Create temporary PowerShell script
cat > /tmp/windows-setup.ps1 << 'EOF'
$ErrorActionPreference = "Stop"
if (!(Test-Path builds\one-rom-build)) {
  Write-Host "Cloning repository..."
  New-Item -ItemType Directory -Force -Path builds | Out-Null
  cd builds
  git clone https://github.com/piersfinlayson/one-rom.git one-rom-build
  cd ..
}
cd builds\one-rom-build
Write-Host "Updating repository..."
git checkout main
git fetch origin
git reset --hard origin/main
cd rust\studio
Write-Host "Cleaning dist directory..."
if (Test-Path dist) {
  cmd /c rmdir /s /q dist
  New-Item -ItemType Directory -Path dist | Out-Null
}
EOF

# Copy to Windows, execute, and clean up
scp /tmp/windows-setup.ps1 "$WINDOWS_HOST:setup.ps1"
ssh "$WINDOWS_HOST" "powershell -ExecutionPolicy Bypass -File setup.ps1"
ssh "$WINDOWS_HOST" "del setup.ps1"
rm /tmp/windows-setup.ps1

echo ""
echo "=== Starting macOS build... ==="
echo "Unlocking macOS keychain..."
ssh -t "$MACOS_HOST" "security unlock-keychain ~/Library/Keychains/login.keychain-db"
echo "Continuing macOS build..."
start=$(date +%s)
set +e
ssh "$MACOS_HOST" "zsh -l -c 'cd $STUDIO_DIR && scripts/build-mac.sh'" > /tmp/studio-build-mac.log 2>&1
mac_status=$?
set -e
end=$(date +%s)
echo "Locking macOS keychain..."
ssh "$MACOS_HOST" "security lock-keychain ~/Library/Keychains/login.keychain-db"
echo "macOS build completed (status: $mac_status) in $((end - start)) seconds"
if [ $mac_status -ne 0 ]; then
  echo "macOS build log:"
  cat /tmp/studio-build-mac.log
fi

echo ""
echo "=== Starting Linux build... ==="
start=$(date +%s)
set +e
ssh "$LINUX_HOST" "bash -l -c 'cd $STUDIO_DIR && scripts/build-linux.sh'" > /tmp/studio-build-linux.log 2>&1
linux_status=$?
set -e
end=$(date +%s)
echo "Linux build completed (status: $linux_status) in $((end - start)) seconds"
if [ $linux_status -ne 0 ]; then
  echo "Linux build log:"
  cat /tmp/studio-build-linux.log
fi

echo ""
echo "=== Starting Windows build... ==="
start=$(date +%s)
set +e
ssh "$WINDOWS_HOST" ". 'C:\Program Files\Microsoft Visual Studio\18\Community\Common7\Tools\Launch-VsDevShell.ps1'; cd $STUDIO_DIR; .\scripts\build-win.ps1 $WINDOWS_PIN" > /tmp/studio-build-win.log 2>&1
win_status=$?
set -e
end=$(date +%s)
echo "Windows build completed (status: $win_status) in $((end - start)) seconds"
if [ $win_status -ne 0 ]; then
    echo "Windows build log:"
    cat /tmp/studio-build-win.log
fi

failed=0
for platform in "macOS:$mac_status" "Linux:$linux_status" "Windows:$win_status"; do
  if [ "${platform#*:}" -ne 0 ]; then
    echo "Error: ${platform%%:*} build failed" >&2
    failed=1
  fi
done
[ $failed -eq 0 ] || exit 1

echo ""
echo "=== Collecting build artifacts ==="

echo "Copying macOS artifacts..."
scp "$MACOS_HOST:$STUDIO_DIR/dist/*.dmg" dist/

echo "Copying Linux artifacts..."
scp "$LINUX_HOST:$STUDIO_DIR/dist/*.deb" dist/

echo "Copying Windows artifacts..."
scp "$WINDOWS_HOST:$STUDIO_DIR/dist/*.exe" dist/

echo ""
echo "=== Build complete ==="
echo "Artifacts in dist/:"
ls -lh dist/