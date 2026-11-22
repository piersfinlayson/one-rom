#!/usr/bin/env python3

# Script to generate One ROM CLI release manifest
#
# This script processes a set of release files for One ROM CLI,
# extracts version and target information, verifies consistency,
# calculates SHA256 checksums, and updates the release manifest.
#
# Usage:
#   python release.py --input-dir <path_to_release_files> --output-dir <output_repo_root>

import argparse
import hashlib
import json
import os
import re
import shutil
import sys
from pathlib import Path
from urllib.request import urlopen

# Magic values
MANIFEST_URL = "https://images.onerom.org/cli/releases.json"
EXPECTED_TARGETS = {
    "x86_64-pc-windows-msvc",
    "aarch64-pc-windows-msvc",
    "universal-apple-darwin",
    "x86_64-unknown-linux-gnu",
    "aarch64-unknown-linux-gnu",
    "arm-unknown-linux-gnueabihf",
}

def error(msg):
    print(f"ERROR: {msg}", file=sys.stderr)
    sys.exit(1)

def extract_version(filename):
    """Extract semantic version from filename."""
    match = re.search(r'(\d+\.\d+\.\d+)', filename)
    if not match:
        error(f"Could not extract version from filename: {filename}")
    return match.group(1)

def identify_target(filename):
    """Map filename to Rust target triple."""
    lower = filename.lower()

    if lower.endswith('.zip'):
        if 'mac' in lower:
            return 'universal-apple-darwin'
        elif 'win' in lower:
            if 'arm64' in lower:
                return 'aarch64-pc-windows-msvc'
            elif 'x86_64' in lower:
                return 'x86_64-pc-windows-msvc'
            else:
                error(f"Could not determine architecture for Windows file: {filename}")
        else:
            error(f"Could not determine platform for zip file: {filename}")
    elif lower.endswith('.deb'):
        if 'aarch64' in lower or 'arm64' in lower:
            return 'aarch64-unknown-linux-gnu'
        elif 'armhf' in lower:
            return 'arm-unknown-linux-gnueabihf'
        elif 'amd64' in lower or 'x86_64' in lower:
            return 'x86_64-unknown-linux-gnu'
        else:
            error(f"Could not determine architecture for Linux file: {filename}")
    else:
        error(f"Unknown file type: {filename}")

def calculate_sha256(filepath):
    """Calculate SHA256 checksum of file."""
    sha256_hash = hashlib.sha256()
    with open(filepath, "rb") as f:
        for byte_block in iter(lambda: f.read(4096), b""):
            sha256_hash.update(byte_block)
    return sha256_hash.hexdigest()

def fetch_manifest():
    """Fetch existing manifest from remote URL."""
    try:
        with urlopen(MANIFEST_URL) as response:
            return json.loads(response.read().decode('utf-8'))
    except Exception as e:
        error(f"Could not fetch manifest from {MANIFEST_URL}: {e}")

def main():
    parser = argparse.ArgumentParser(description='Generate One ROM CLI release manifest')
    parser.add_argument('--input-dir', required=True, help='Directory containing release files')
    parser.add_argument('--output-dir', required=True, help='Output directory (repo root)')
    args = parser.parse_args()

    input_dir = Path(args.input_dir)
    output_dir = Path(args.output_dir)

    if not input_dir.exists():
        error(f"Input directory does not exist: {input_dir}")

    if not output_dir.exists():
        error(f"Output directory does not exist: {output_dir}")

    # Get all files in input directory
    files = [f for f in input_dir.iterdir() if f.is_file()]
    if not files:
        error(f"No files found in input directory: {input_dir}")

    # Extract versions and identify targets
    file_info = {}
    versions = set()

    for filepath in files:
        filename = filepath.name
        version = extract_version(filename)
        target = identify_target(filename)
        versions.add(version)

        if target in file_info:
            error(f"Duplicate target found: {target}")

        file_info[target] = {
            'filename': filename,
            'filepath': filepath,
            'version': version
        }

    # Verify single version
    if len(versions) != 1:
        error(f"Inconsistent versions found: {versions}")

    version = versions.pop()
    print(f"Version: {version}")

    # Verify all targets present
    found_targets = set(file_info.keys())
    if found_targets != EXPECTED_TARGETS:
        missing = EXPECTED_TARGETS - found_targets
        extra = found_targets - EXPECTED_TARGETS
        if missing:
            error(f"Missing targets: {missing}")
        if extra:
            error(f"Unexpected targets: {extra}")

    # Fetch existing manifest
    print(f"Fetching manifest from {MANIFEST_URL}")
    manifest = fetch_manifest()

    # Check if version already exists
    for release in manifest.get('releases', []):
        if release['version'] == version:
            error(f"Version {version} already exists in manifest")

    # Create output directory structure
    version_dir = output_dir / 'cli' / f'v{version}'
    version_dir.mkdir(parents=True, exist_ok=True)

    # Copy files and build platform entries
    platforms = []
    for target in sorted(EXPECTED_TARGETS):
        info = file_info[target]
        filepath = info['filepath']
        filename = info['filename']

        # Copy file
        dest = version_dir / filename
        print(f"Copying {filename} to {dest}")
        shutil.copy2(filepath, dest)

        # Calculate checksum
        sha256 = calculate_sha256(dest)
        print(f"  SHA256: {sha256}")

        platforms.append({
            'target': target,
            'filename': filename,
            'sha256': sha256
        })

    # Add new release to manifest
    new_release = {
        'version': version,
        'path': f'v{version}',
        'platforms': platforms
    }

    manifest['releases'].insert(0, new_release)

    # Update latest for all targets
    for target in EXPECTED_TARGETS:
        manifest['latest'][target] = version

    # Write updated manifest
    manifest_path = output_dir / 'cli' / 'releases.json'
    manifest_path.parent.mkdir(parents=True, exist_ok=True)

    print(f"Writing manifest to {manifest_path}")
    with open(manifest_path, 'w') as f:
        json.dump(manifest, f, indent=2)

    print(f"Success! Release {version} added to manifest.")

if __name__ == '__main__':
    main()