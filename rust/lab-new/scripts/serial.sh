#!/bin/bash
# Connect to a One ROM reader board's serial console.
#
# Usage: serial.sh [serial_port]

set -e

# Function to print usage
print_usage() {
    echo "Usage: $0 [serial_port]"
    echo ""
    echo "  serial_port  <full serial port path>"
    echo ""
    echo "Examples:"
    echo "  $0"
    echo "  $0 /dev/cu.usbmodem1103"
}   

if [ $# -gt 1 ]; then
    print_usage
    exit 1
fi

SRIAL_PORT="$1"
if [ -z "$SRIAL_PORT" ]; then
    echo "Error: Serial port not specified."
    print_usage
    exit 1
fi

python3 -m serial.tools.miniterm "${1}" 115200