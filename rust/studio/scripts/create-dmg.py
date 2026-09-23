#!/usr/bin/env python3
"""
Build macOS DMG with proper layout using dmgbuild.

Takes the .app bundle built by cargo bundle, then:
1. Creates a multi-resolution ICNS file from PNG icons
2. Uses dmgbuild to create the DMG with proper layout, background, and icon positions
3. Sets custom icon on the DMG using fileicon

Dependencies:
  pip install dmgbuild toml
  brew install fileicon

macOS tools used (built-in):
  - iconutil (creating ICNS files)
"""

import subprocess
import sys
import os
import shutil
import tempfile
import plistlib
import argparse
from pathlib import Path

try:
    import toml
except ImportError:
    print("Error: toml not installed. Run: pip install toml", file=sys.stderr)
    sys.exit(1)

try:
    import dmgbuild
except ImportError:
    print("Error: dmgbuild not installed. Run: pip install dmgbuild", file=sys.stderr)
    sys.exit(1)


# Configuration constants
VOLUME_NAME = "One ROM Studio"

# DMG settings
DMG_BACKGROUND = "assets/onerom-dmg.png"
DMG_WINDOW_X = 200
DMG_WINDOW_Y = 200
DMG_WINDOW_WIDTH = 540
DMG_WINDOW_HEIGHT = 380
DMG_APP_ICON_X = 140
DMG_APP_ICON_Y = 160
DMG_APPLICATIONS_ICON_X = 400
DMG_APPLICATIONS_ICON_Y = 160
DMG_ICON_SIZE = 128
DMG_TEXT_SIZE = 16
DMG_FORMAT = "UDBZ"  # Compressed

# License
LICENSE_FILE = "LICENSE.txt"
LICENSE_LANGUAGE = "en_US"

# Icon files for creating ICNS
# Maps size in pixels to file path
# These will be used to create a proper multi-resolution ICNS file
ICON_FILES = {
    16: "assets/onerom-16x16.png",
    32: "assets/onerom-32x32.png",
    64: "assets/onerom-64x64.png",
    128: "assets/onerom-128x128.png",
    256: "assets/onerom-256x256.png",
    512: "assets/onerom-512x512.png",
    1024: "assets/onerom-1024x1024.png",
}

def build_dmg_with_dmgbuild(app_bundle_path, output_dmg):
    """Create final DMG using dmgbuild with proper layout."""
    print(f"Building final DMG with dmgbuild...")
    
    app_name = app_bundle_path.name
    
    # dmgbuild settings
    settings = {
        'format': DMG_FORMAT,
        'size': None,      # Auto-calculate
        
        # Files to include
        'files': [str(app_bundle_path)],
        
        # Create Applications symlink
        'symlinks': {'Applications': '/Applications'},
        
        # Icon positions (x, y from top-left)
        'icon_locations': {
            app_name: (DMG_APP_ICON_X, DMG_APP_ICON_Y),
            'Applications': (DMG_APPLICATIONS_ICON_X, DMG_APPLICATIONS_ICON_Y)
        },
        
        # Background image
        'background': DMG_BACKGROUND,
        
        # Window settings
        'window_rect': ((DMG_WINDOW_X, DMG_WINDOW_Y), (DMG_WINDOW_WIDTH, DMG_WINDOW_HEIGHT)),
        
        # Icon and text size
        'icon_size': DMG_ICON_SIZE,
        'text_size': DMG_TEXT_SIZE,
        
        # License agreement
        'license': {
            'default-language': LICENSE_LANGUAGE,
            'licenses': {LICENSE_LANGUAGE: LICENSE_FILE}
        }
    }
    
    # Build the DMG
    dmgbuild.build_dmg(output_dmg, VOLUME_NAME, settings=settings)
    print(f"✓ Created {output_dmg}")

def create_icns_from_pngs(output_icns_path):
    """
    Create a proper ICNS file from the PNG icon files.
    Uses iconutil which is built into macOS and works headlessly.
    Returns the path to the created ICNS file.
    
    Exits with error if any required icon files are missing or ICNS creation fails.
    """
    print(f"Creating ICNS file from PNG icons...")
    
    # Verify all required icon files exist
    missing_icons = []
    for size, path in ICON_FILES.items():
        if not Path(path).exists():
            missing_icons.append(f"{size}x{size}: {path}")
    
    if missing_icons:
        print(f"Error: Required icon files are missing:", file=sys.stderr)
        for missing in missing_icons:
            print(f"  {missing}", file=sys.stderr)
        sys.exit(1)
    
    # Create a temporary iconset directory
    iconset_dir = tempfile.mkdtemp(suffix=".iconset")
    
    try:
        # Mapping of iconset filenames to source sizes
        # Format: (iconset_filename, source_size)
        iconset_mapping = [
            ('icon_16x16.png', 16),
            ('icon_16x16@2x.png', 32),
            ('icon_32x32.png', 32),
            ('icon_32x32@2x.png', 64),
            ('icon_128x128.png', 128),
            ('icon_128x128@2x.png', 256),
            ('icon_256x256.png', 256),
            ('icon_256x256@2x.png', 512),
            ('icon_512x512.png', 512),
            ('icon_512x512@2x.png', 1024),  # If available
        ]
        
        # Copy icons into iconset directory
        for iconset_filename, source_size in iconset_mapping:
            if source_size not in ICON_FILES:
                # Skip sizes we don't have (e.g., 1024x1024)
                continue
            
            source_path = ICON_FILES[source_size]
            dest_path = Path(iconset_dir) / iconset_filename
            
            shutil.copy(source_path, dest_path)
        
        # Convert iconset to ICNS using iconutil
        try:
            subprocess.run(
                ['iconutil', '-c', 'icns', iconset_dir, '-o', output_icns_path],
                check=True,
                capture_output=True,
                text=True
            )
        except subprocess.CalledProcessError as e:
            print(f"Error: iconutil failed to create ICNS:", file=sys.stderr)
            print(f"  stdout: {e.stdout}", file=sys.stderr)
            print(f"  stderr: {e.stderr}", file=sys.stderr)
            sys.exit(1)
        
        if not Path(output_icns_path).exists():
            print(f"Error: iconutil did not create ICNS file at {output_icns_path}", file=sys.stderr)
            sys.exit(1)
        
        print(f"✓ Created ICNS: {output_icns_path}")
        return output_icns_path

    except Exception as e:
        print(f"Error creating ICNS: {e}", file=sys.stderr)
        raise
        
    finally:
        # Clean up temporary iconset directory
        if os.path.exists(iconset_dir):
            shutil.rmtree(iconset_dir)

def set_dmg_icon(dmg_path, icns_path):
    """
    Set a custom icon on the DMG file using fileicon.
    
    Exits with error if fileicon is not available or fails to set the icon.
    """
    print(f"Setting custom icon on DMG...")
    
    # Check if fileicon is available
    try:
        subprocess.run(
            ['which', 'fileicon'],
            check=True,
            capture_output=True
        )
    except subprocess.CalledProcessError:
        print(f"Error: fileicon not found. Install with: brew install fileicon", file=sys.stderr)
        sys.exit(1)
    
    # Set the icon
    try:
        subprocess.run(
            ['fileicon', 'set', str(dmg_path), str(icns_path)],
            check=True,
            capture_output=True,
            text=True
        )
    except subprocess.CalledProcessError as e:
        print(f"Error: fileicon failed to set icon on DMG:", file=sys.stderr)
        print(f"  stdout: {e.stdout}", file=sys.stderr)
        print(f"  stderr: {e.stderr}", file=sys.stderr)
        sys.exit(1)
    
    print(f"✓ Set custom icon on DMG")


def parse_arguments():
    """Parse and validate command-line arguments."""
    parser = argparse.ArgumentParser(
        description='Build macOS DMG with proper layout using dmgbuild',
        formatter_class=argparse.RawDescriptionHelpFormatter,
        epilog='''
Example:
  %(prog)s --app-bundle dist/One\\ ROM\\ Studio.app --output OneROMStudio_0.5.4_aarch64.dmg --dist-dir dist
        '''
    )
    
    parser.add_argument(
        '--app-bundle',
        required=True,
        help='Path to the signed .app bundle'
    )
    parser.add_argument(
        '--output',
        required=True,
        help='Output DMG filename'
    )
    parser.add_argument(
        '--dist-dir',
        required=True,
        help='Distribution directory'
    )
    
    args = parser.parse_args()

    # Validate app bundle path
    app_bundle_path = Path(args.app_bundle)
    if not app_bundle_path.exists():
        print(f"Error: App bundle not found: {args.app_bundle}", file=sys.stderr)
        sys.exit(1)
    
    if not app_bundle_path.is_dir() or not app_bundle_path.name.endswith('.app'):
        print(f"Error: Provided path is not a .app bundle: {args.app_bundle}", file=sys.stderr)
        sys.exit(1)
    
    # Validate and prepare output paths
    dist_dir = Path(args.dist_dir)
    dist_dir.mkdir(exist_ok=True)
    
    output_filename = args.output
    if not output_filename.endswith('.dmg'):
        output_filename += '.dmg'
    
    return {
        'app_bundle_path': app_bundle_path,
        'dist_dir': dist_dir,
        'output_path': dist_dir / output_filename
    }

def main():
    """Main build process."""
    # Parse arguments
    config = parse_arguments()
    
    app_bundle_path = config['app_bundle_path']
    dist_dir = config['dist_dir']
    final_dmg_path = config['output_path']
    
    # Create temporary ICNS path
    icns_path = dist_dir / "temp_icon.icns"
    
    try:
        print(f"Using .app bundle: {app_bundle_path}")
        
        # Step 1: Create ICNS file from PNG icons
        create_icns_from_pngs(icns_path)
        
        # Step 2: Build final DMG with dmgbuild
        build_dmg_with_dmgbuild(app_bundle_path, final_dmg_path)
        
        # Step 3: Set custom icon on DMG using fileicon
        set_dmg_icon(final_dmg_path, icns_path)
        
        print(f"\n✓ Successfully created: {final_dmg_path}")
        
    except Exception as e:
        print(f"\n✗ Build failed: {e}", file=sys.stderr)
        sys.exit(1)
    
    finally:
        # Cleanup
        if icns_path.exists():
            print(f"Cleaning up temporary ICNS file...")
            icns_path.unlink()


if __name__ == '__main__':
    main()