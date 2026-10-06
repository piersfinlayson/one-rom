#!/usr/bin/env python3

# Script to add a One ROM Lab release to Lab's release manifest
#
# This script downloads every file of the lab-vX.Y.Z GitHub release, checks each
# image with this tree's `onerom firmware inspect`, calculates SHA256 checksums,
# and adds the release to lab/releases.json in the images repository.  The
# downloaded files stay in rust/lab/dist, so one can be flashed before the
# manifest is pushed.
#
# It doesn't tag, push or publish anything.
#
# Usage:
#   python release.py --version <x.y.z> --output-dir <output_repo_root>
#
# --base-url replaces GitHub as the place the lab-vX.Y.Z directory is fetched
# from, for testing against a local server.  The manifest's `path` for the
# release is the URL the files were fetched from.

import argparse
import hashlib
import json
import os
import re
import shutil
import subprocess
import sys
import zipfile
from pathlib import Path
from urllib.request import urlopen

# Magic values
BASE_URL = "https://github.com/piersfinlayson/one-rom/releases/download"
MANIFEST_VERSION = 1
NO_BOARD = "no-board"
TIMEOUT_SECS = 60

# A VID:PID nothing uses.  The CLI looks for One ROMs at startup, and this keeps
# it from opening any of this machine's devices.
NO_DEVICE_VID_PID = "0000:0000"

SCRIPT_DIR = Path(__file__).resolve().parent
LAB_DIR = SCRIPT_DIR.parent
ROOT = LAB_DIR.parent.parent
DIST_DIR = LAB_DIR / 'dist'
BOARDS_DIR = ROOT / 'rust' / 'config' / 'json'

def error(msg):
    print(f"ERROR: {msg}", file=sys.stderr)
    sys.exit(1)

def parse_version(version):
    """Split an X.Y.Z version into a tuple that sorts in version order."""
    match = re.fullmatch(r'(\d+)\.(\d+)\.(\d+)', version)
    if not match:
        error(f"Not an X.Y.Z version: {version}")
    return tuple(int(part) for part in match.groups())

def fire_boards():
    """Every Fire board, from rust/config/json, as ci/build-images.sh finds them."""
    boards = sorted(path.stem for path in BOARDS_DIR.glob('fire-*.json'))
    if not boards:
        error(f"No Fire boards found in {BOARDS_DIR}")
    return boards

def calculate_sha256(filepath):
    """Calculate SHA256 checksum of file."""
    sha256_hash = hashlib.sha256()
    with open(filepath, "rb") as f:
        for byte_block in iter(lambda: f.read(4096), b""):
            sha256_hash.update(byte_block)
    return sha256_hash.hexdigest()

def run(args, cwd):
    """Run a command, stopping on failure."""
    try:
        return subprocess.run(args, cwd=cwd, check=True, stdout=subprocess.PIPE, text=True)
    except (OSError, subprocess.CalledProcessError) as e:
        error(f"{' '.join(args)} failed: {e}")

def build_onerom():
    """Build this tree's CLI and return its path.

    Built from rust/, so the workspace's toolchain and host target build it
    rather than Lab's.
    """
    rust_dir = ROOT / 'rust'
    print("Building this tree's onerom CLI...")
    run(['cargo', 'build', '-p', 'onerom-cli'], rust_dir)
    metadata = run(['cargo', 'metadata', '--no-deps', '--format-version', '1'], rust_dir)
    return Path(json.loads(metadata.stdout)['target_directory']) / 'debug' / 'onerom'

def download(url, dest):
    """Download url to dest.  An interrupted download leaves only a .partial file."""
    partial = dest.with_name(dest.name + '.partial')
    try:
        with urlopen(url, timeout=TIMEOUT_SECS) as response, open(partial, 'wb') as f:
            shutil.copyfileobj(response, f)
    except Exception as e:
        partial.unlink(missing_ok=True)
        error(f"Could not download {url}: {e}")
    os.replace(partial, dest)

def check_image(onerom, filepath, version, board):
    """Check an image with `onerom firmware inspect`, for Lab's version and its board."""
    result = subprocess.run(
        [str(onerom), 'firmware', 'inspect', '--firmware', str(filepath),
         '--vid-pid', NO_DEVICE_VID_PID],
        capture_output=True, text=True)
    if result.returncode != 0 or result.stderr:
        error(f"onerom firmware inspect failed on {filepath.name}:\n{result.stderr}")

    fields = {}
    for line in result.stdout.splitlines():
        name, sep, value = line.partition(':')
        if sep:
            fields[name] = value.strip()

    expected = {
        'Firmware': 'One ROM Lab',
        'Version': version,
        'Board': board if board != NO_BOARD else '(not set)',
    }
    for name, value in expected.items():
        if fields.get(name) != value:
            error(f"{filepath.name} isn't One ROM Lab {version} for {expected['Board']}:\n"
                  f"{result.stdout}")
    print(f"  Checked: One ROM Lab {version}, board {expected['Board']}")

def check_elf_zip(filepath, elf_names):
    """Check the ELF zip opens and holds exactly the expected ELFs."""
    try:
        with zipfile.ZipFile(filepath) as zf:
            corrupt = zf.testzip()
            names = sorted(zf.namelist())
    except zipfile.BadZipFile as e:
        error(f"{filepath.name} isn't a valid zip: {e}")
    if corrupt is not None:
        error(f"{filepath.name} has a corrupt member: {corrupt}")
    if names != sorted(elf_names):
        error(f"{filepath.name} holds {names}, expected {sorted(elf_names)}")
    print(f"  Checked: {len(names)} ELFs")

def load_manifest(manifest_path):
    """Read lab/releases.json, or start a new one."""
    if not manifest_path.exists():
        print(f"No manifest at {manifest_path} - creating it")
        return {'version': MANIFEST_VERSION, 'latest': {}, 'releases': []}
    with open(manifest_path) as f:
        return json.load(f)

def main():
    parser = argparse.ArgumentParser(description='Add a One ROM Lab release to its release manifest')
    parser.add_argument('--version', required=True, help='Lab version, e.g. 0.4.0')
    parser.add_argument('--output-dir', required=True, help='Output directory (images repo root)')
    parser.add_argument('--base-url', default=BASE_URL,
                        help=f"URL holding the lab-vX.Y.Z release directory (default {BASE_URL})")
    args = parser.parse_args()

    version = args.version
    version_key = parse_version(version)
    output_dir = Path(args.output_dir)
    release_url = f"{args.base_url.rstrip('/')}/lab-v{version}"

    if not output_dir.exists():
        error(f"Output directory does not exist: {output_dir}")

    # Refuse a listed version before downloading anything
    manifest_path = output_dir / 'lab' / 'releases.json'
    manifest = load_manifest(manifest_path)
    for release in manifest['releases']:
        if release['version'] == version:
            error(f"Version {version} already exists in {manifest_path}")

    # One image per Fire board, one without a board, and the ELF zip
    names = [NO_BOARD] + fire_boards()
    images = {name: f"onerom-lab-{name}-{version}.bin" for name in names}
    elf_names = [f"onerom-lab-{name}-{version}.elf" for name in names]
    elf_zip = f"onerom-lab-elf-{version}.zip"

    onerom = build_onerom()

    DIST_DIR.mkdir(parents=True, exist_ok=True)
    print(f"Downloading from {release_url} to {DIST_DIR}")
    for filename in list(images.values()) + [elf_zip]:
        print(f"Downloading {filename}")
        download(f"{release_url}/{filename}", DIST_DIR / filename)

    # Check and hash each image
    boards = []
    for name, filename in images.items():
        filepath = DIST_DIR / filename
        print(f"Processing {filename}")
        check_image(onerom, filepath, version, name)
        sha256 = calculate_sha256(filepath)
        print(f"  SHA256: {sha256}")
        boards.append({
            'name': name,
            'filename': filename,
            'sha256': sha256
        })

    print(f"Processing {elf_zip}")
    check_elf_zip(DIST_DIR / elf_zip, elf_names)

    # Add the release, keeping releases newest first
    new_release = {
        'version': version,
        'path': release_url,
        'boards': boards
    }
    releases = manifest['releases']
    index = next((i for i, release in enumerate(releases)
                  if parse_version(release['version']) < version_key), len(releases))
    releases.insert(index, new_release)

    # Update latest for every board added
    for name in images:
        manifest['latest'][name] = version

    # Write updated manifest
    manifest_path.parent.mkdir(parents=True, exist_ok=True)
    print(f"Writing manifest to {manifest_path}")
    with open(manifest_path, 'w') as f:
        json.dump(manifest, f, indent=2)

    print(f"Success! Release {version} added to manifest.")
    print(f"The downloaded files are in {DIST_DIR}")

if __name__ == '__main__':
    main()
