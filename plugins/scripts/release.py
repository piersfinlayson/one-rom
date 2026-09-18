#!/usr/bin/env python3

# Script to release One ROM plugins to the images repository.
#
# Discovers all staged plugins in the input (dist/) directory, parses their
# binary headers, verifies integrity, and updates the release manifests in
# the output (images repo) directory.
#
# Usage:
#   python scripts/release.py --input-dir dist --output-dir <path_to_images_repo>

import argparse
import hashlib
import json
import shutil
import struct
import sys
from pathlib import Path
from urllib.error import HTTPError
from urllib.request import urlopen

# URLs
IMAGES_BASE_URL         = "https://images.onerom.org/plugins"
TOP_MANIFEST_URL        = f"{IMAGES_BASE_URL}/plugins.json"

# Plugin header constants
PLUGIN_MAGIC            = 0x2041524F
KNOWN_API_VERSIONS      = {1}
PLUGIN_HEADER_SIZE      = 256

# Plugin type enum mapping
PLUGIN_TYPE_MAP = {
    0: "system",
    1: "user",
    2: "pio",
}

# Header struct layout (little-endian)
# magic(u32), api_version(u32), major(u16), minor(u16), patch(u16), build(u16),
# entry(u32), plugin_type(u8), sam_usage(u8), overrides1(u8), properties1(u8),
# min_fw_major(u16), min_fw_minor(u16), min_fw_patch(u16)
HEADER_STRUCT           = struct.Struct("<IIHHHHIBBBBHHHxx")

# File names
META_FILENAME           = "plugin-meta.json"
DIST_FILENAME           = "plugin.bin"
PLUGIN_FILENAME         = "plugin.bin"
PLUGIN_MANIFEST         = "releases.json"
TOP_MANIFEST            = "plugins.json"

# Manifest schema versions
MANIFEST_SCHEMA_VERSION = 1
TOP_MANIFEST_SCHEMA_VERSION = 1

# Key order of a written per-plugin manifest, so one fetched without a newer
# field gains it in place rather than at the end
MANIFEST_KEY_ORDER = (
    "version", "display_name", "description", "author", "source", "license",
    "latest", "releases",
)


def error(msg):
    print(f"ERROR: {msg}", file=sys.stderr)
    sys.exit(1)


def info(msg):
    print(msg)


def calculate_sha256(filepath):
    sha256 = hashlib.sha256()
    with open(filepath, "rb") as f:
        for block in iter(lambda: f.read(4096), b""):
            sha256.update(block)
    return sha256.hexdigest()


def fetch_json(url):
    """Fetch JSON from a URL. Returns None if 404, exits on other errors."""
    try:
        with urlopen(url) as response:
            return json.loads(response.read().decode("utf-8"))
    except HTTPError as e:
        if e.code == 404:
            return None
        error(f"HTTP error fetching {url}: {e}")
    except Exception as e:
        error(f"Could not fetch {url}: {e}")


def parse_header(bin_path):
    """Parse and validate the plugin binary header."""
    with open(bin_path, "rb") as f:
        data = f.read(PLUGIN_HEADER_SIZE)

    if len(data) < PLUGIN_HEADER_SIZE:
        error(f"Binary too small to contain a valid header: {bin_path}")

    fields = HEADER_STRUCT.unpack_from(data)
    (magic, api_version, major, minor, patch, build,
     entry, plugin_type_raw, sam_usage, overrides1, properties1,
     min_fw_major, min_fw_minor, min_fw_patch) = fields

    if magic != PLUGIN_MAGIC:
        error(f"Invalid magic 0x{magic:08X} in {bin_path} (expected 0x{PLUGIN_MAGIC:08X})")

    if api_version not in KNOWN_API_VERSIONS:
        error(f"Unknown API version {api_version} in {bin_path} (known: {sorted(KNOWN_API_VERSIONS)})")

    if plugin_type_raw not in PLUGIN_TYPE_MAP:
        error(f"Unknown plugin type {plugin_type_raw} in {bin_path}")

    return {
        "api_version":   api_version,
        "major":         major,
        "minor":         minor,
        "patch":         patch,
        "plugin_type":   PLUGIN_TYPE_MAP[plugin_type_raw],
        "min_fw_major":  min_fw_major,
        "min_fw_minor":  min_fw_minor,
        "min_fw_patch":  min_fw_patch,
    }


def read_meta(meta_path):
    """Read and validate plugin-meta.json."""
    with open(meta_path) as f:
        meta = json.load(f)

    for field in ("name", "display_name", "description", "author", "source", "license"):
        if field not in meta:
            error(f"Missing field '{field}' in {meta_path}")

    return meta


def discover_plugins(input_dir):
    """Discover staged plugins by finding plugin-meta.json files in dist/."""
    plugins = []
    for meta_path in sorted(input_dir.rglob(META_FILENAME)):
        plugin_dir = meta_path.parent
        type_dir   = plugin_dir.parent
        plugins.append({
            "meta_path":   meta_path,
            "bin_path":    plugin_dir / DIST_FILENAME,
            "plugin_dir":  plugin_dir,
            "path_type":   type_dir.name,
            "path_name":   plugin_dir.name,
        })
    return plugins


def load_or_create_plugin_manifest(url, display_name, description):
    """Fetch existing per-plugin manifest, or create a fresh one."""
    manifest = fetch_json(url)
    if manifest is None:
        info(f"  No existing manifest found at {url} - creating new")
        return {
            "version":      MANIFEST_SCHEMA_VERSION,
            "display_name": display_name,
            "description":  description,
            "latest":       None,
            "releases":     [],
        }
    return manifest


def load_or_create_top_manifest(url):
    """Fetch existing top-level plugins.json, or create a fresh one."""
    manifest = fetch_json(url)
    if manifest is None:
        info(f"No existing top manifest found at {url} - creating new")
        return {
            "version": TOP_MANIFEST_SCHEMA_VERSION,
            "plugins": [],
        }
    return manifest


def main():
    parser = argparse.ArgumentParser(description="Release One ROM plugins to the images repository")
    parser.add_argument("--input-dir",  required=True, help="Staged dist directory (e.g. dist)")
    parser.add_argument("--overwrite",  action="store_true",
                        help="Overwrite an existing release of the same version")
    parser.add_argument("--output-dir", required=True, help="Images repository root")
    args = parser.parse_args()

    input_dir  = Path(args.input_dir)
    output_dir = Path(args.output_dir)

    if not input_dir.exists():
        error(f"Input directory does not exist: {input_dir}")
    if not output_dir.exists():
        error(f"Output directory does not exist: {output_dir}")

    plugins = discover_plugins(input_dir)
    if not plugins:
        error(f"No staged plugins found in {input_dir}")

    info(f"Found {len(plugins)} plugin(s) to release")

    # Fetch top-level manifest once
    info(f"Fetching top-level manifest from {TOP_MANIFEST_URL}")
    top_manifest = load_or_create_top_manifest(TOP_MANIFEST_URL)

    for plugin in plugins:
        meta_path  = plugin["meta_path"]
        bin_path   = plugin["bin_path"]
        path_type  = plugin["path_type"]
        path_name  = plugin["path_name"]

        info(f"\nProcessing {path_type}/{path_name}...")

        if not bin_path.exists():
            error(f"Binary not found: {bin_path}")

        # Read and validate meta
        meta = read_meta(meta_path)
        if meta["name"] != path_name:
            error(f"Plugin name '{meta['name']}' does not match directory '{path_name}'")

        # Parse and validate header
        header = parse_header(bin_path)

        # Cross-check type
        if header["plugin_type"] != path_type:
            error(
                f"Plugin type '{header['plugin_type']}' in binary does not match "
                f"directory type '{path_type}'"
            )

        version = f"{header['major']}.{header['minor']}.{header['patch']}"
        info(f"  Version:    {version}")
        info(f"  Type:       {header['plugin_type']}")
        info(f"  API:        {header['api_version']}")
        info(f"  Min FW:     {header['min_fw_major']}.{header['min_fw_minor']}.{header['min_fw_patch']}")

        # Fetch per-plugin manifest
        plugin_manifest_url = f"{IMAGES_BASE_URL}/{path_type}/{path_name}/{PLUGIN_MANIFEST}"
        info(f"  Fetching plugin manifest from {plugin_manifest_url}")
        plugin_manifest = load_or_create_plugin_manifest(
            plugin_manifest_url,
            meta["display_name"],
            meta["description"],
        )

        # source and license are fixed for the life of a plugin (see
        # CONTRIBUTIONS.md), so a change is a mistake, not an update.
        for field in ("source", "license"):
            if field in plugin_manifest and plugin_manifest[field] != meta[field]:
                error(
                    f"{field} '{meta[field]}' differs from the published "
                    f"'{plugin_manifest[field]}' for {path_type}/{path_name}"
                )

        # Check version not already released
        existing = [r for r in plugin_manifest.get("releases", []) if r["version"] == version]
        if existing:
            if not args.overwrite:
                error(
                    f"Version {version} already exists in manifest for {path_type}/{path_name} "
                    f"(use --overwrite to replace)"
                )
            info(f"  Overwriting existing release {version}")
            plugin_manifest["releases"] = [
                r for r in plugin_manifest["releases"] if r["version"] != version
            ]

        # Calculate SHA256
        sha256 = calculate_sha256(bin_path)
        info(f"  SHA256:     {sha256}")

        # Copy binary to output, removing existing version dir if overwriting
        version_dir = output_dir / "plugins" / path_type / path_name / f"v{version}"
        if version_dir.exists():
            if not args.overwrite:
                error(f"Version directory already exists: {version_dir} (use --overwrite to replace)")
            shutil.rmtree(version_dir)
            info(f"  Removed existing version directory {version_dir}")
        version_dir.mkdir(parents=True, exist_ok=True)
        dest_bin = version_dir / PLUGIN_FILENAME
        shutil.copy2(bin_path, dest_bin)
        info(f"  Copied binary to {dest_bin}")

        # Build release entry
        new_release = {
            "version":      version,
            "path":         f"v{version}",
            "filename":     PLUGIN_FILENAME,
            "sha256":       sha256,
            "api_version":  header["api_version"],
            "plugin_type":  header["plugin_type"],
            "min_fw_version": f"{header['min_fw_major']}.{header['min_fw_minor']}.{header['min_fw_patch']}",
        }

        plugin_manifest["releases"].insert(0, new_release)
        plugin_manifest["latest"] = version
        plugin_manifest["display_name"] = meta["display_name"]
        plugin_manifest["description"]  = meta["description"]
        plugin_manifest["author"]       = meta["author"]
        plugin_manifest["source"]       = meta["source"]
        plugin_manifest["license"]      = meta["license"]

        # Write per-plugin manifest
        manifest_path = output_dir / "plugins" / path_type / path_name / PLUGIN_MANIFEST
        manifest_path.parent.mkdir(parents=True, exist_ok=True)
        ordered = {k: plugin_manifest[k] for k in MANIFEST_KEY_ORDER if k in plugin_manifest}
        ordered.update({k: v for k, v in plugin_manifest.items() if k not in ordered})
        with open(manifest_path, "w") as f:
            json.dump(ordered, f, indent=2)
        info(f"  Written manifest to {manifest_path}")

        # Update top-level manifest - add plugin if not already listed
        existing_names = {p["name"] for p in top_manifest["plugins"]}
        if path_name not in existing_names:
            top_manifest["plugins"].append({
                "name": path_name,
                "type": path_type,
                "path": f"{path_type}/{path_name}",
            })
            info(f"  Added {path_type}/{path_name} to top-level manifest")

    # Write top-level manifest
    top_manifest_path = output_dir / "plugins" / TOP_MANIFEST
    top_manifest_path.parent.mkdir(parents=True, exist_ok=True)
    with open(top_manifest_path, "w") as f:
        json.dump(top_manifest, f, indent=2)
    info(f"\nWritten top-level manifest to {top_manifest_path}")
    info(f"Released {len(plugins)} plugin(s) successfully.")


if __name__ == "__main__":
    main()