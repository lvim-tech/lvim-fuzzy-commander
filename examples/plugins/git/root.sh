#!/bin/sh
# A `control` action: what it prints are instructions for lvc (cd / select / message / error ...).
root=$(git -C "$LVC_DIR" rev-parse --show-toplevel 2>/dev/null) || { echo "error not in a git repository"; exit 0; }
echo "cd $root"
echo "message $(git -C "$root" branch --show-current) @ $root"
