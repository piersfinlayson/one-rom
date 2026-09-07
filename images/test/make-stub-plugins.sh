#!/usr/bin/env bash
# Regenerate the stub plugin images onerom-config/test/24-plugins-23xx.json uses.
#
# They exist so a test config can declare plugin slots, which is what makes the
# firmware's slot numbering differ from the flash slot numbers RBCP reports.
# Nothing runs the code in them, so each is a plugin header and no body - the
# generator pads it out to fill a slot.
#
# The entry point is 0xFFFFFFFF, outside the region the firmware expects a
# plugin to be linked into, so check_plugin_valid refuses to launch either of
# these and they are harmless on real hardware. Slot counts come from the
# config rather than the header, so they are what a device with two plugins
# reports either way.
#
# Header layout: ora_plugin_header_t in firmware/ora/api.h.
set -e
dir=$(cd "$(dirname "$0")" && pwd)

python3 - "$dir" <<'EOF'
import struct
import sys

dir = sys.argv[1]

for name, plugin_type in (("system", 0), ("user", 1)):
    header = struct.pack(
        "<4sIHHHHIBBBBHHH",
        b"ORA ",       # magic
        1,             # api_version
        0, 1, 0, 0,    # plugin major/minor/patch/build
        0xFFFFFFFF,    # entry
        plugin_type,
        0,             # sam_usage
        0,             # overrides1
        0,             # properties1
        0, 7, 0,       # minimum firmware version
    )
    header += b"\x00" * (256 - len(header))
    path = f"{dir}/stub-{name}-plugin.bin"
    with open(path, "wb") as f:
        f.write(header)
    print(f"Wrote {path}")
EOF
