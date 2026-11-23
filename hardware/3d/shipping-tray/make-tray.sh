#!/bin/sh
# Usage: make-tray.sh COUNT [COLSxROWS]
set -e

usage() { echo "usage: $0 COUNT [COLSxROWS]" >&2; exit 1; }

[ $# -ge 1 ] && [ $# -le 2 ] || usage
count=$1
case $count in ''|*[!0-9]*|0) usage ;; esac
cols=0 rows=0
if [ $# -eq 2 ]; then
    case $2 in [1-9]*x[1-9]*) ;; *) usage ;; esac
    cols=${2%x*} rows=${2#*x}
    case $cols$rows in *[!0-9]*) usage ;; esac
fi

scad=$(dirname "$0")/tray.scad
for part in base top; do
    out=tray-$count-$part.stl
    if ! log=$(openscad -o "$out" -D count="$count" -D cols="$cols" -D rows="$rows" \
            -D part="\"$part\"" "$scad" 2>&1); then
        echo "$log" | grep -E 'ERROR|Assertion' >&2
        exit 1
    fi
    echo "$out $(echo "$log" | sed -n 's/^ECHO: grid = "\(.*\)"$/\1/p')"
done
