#!/usr/bin/env bash

# Used to create a stock, empty firmware image for One ROM, for a specific
# hardware variant

set -e

usage() {
    echo "Usage: $0 [-d] [-l]"
}

help() {
    echo "This script creates a stock, empty firmware image for"
    echo "One ROM, for a specific hardware variant."
}

if [ $1 == "--help" ] || [ $1 == "-h" ]; then
    usage
    echo ""
    help
    exit 0
fi

# Parse the optional flags
BOOT_LOGGING=0
DEBUG_LOGGING=0
while getopts "dl" opt; do
  case $opt in
    d)
      DEBUG_LOGGING=1
      ;;
    l)
      BOOT_LOGGING=1
      ;;
    \?)
      echo "Invalid option: -$OPTARG" >&2
      usage
      exit 1
      ;;
  esac
done

echo "BOOT_LOGGING=$BOOT_LOGGING DEBUG_LOGGING=$DEBUG_LOGGING make"
BOOT_LOGGING=$BOOT_LOGGING DEBUG_LOGGING=$DEBUG_LOGGING make
