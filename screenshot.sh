#!/bin/bash
export PATH="$HOME/.cargo/bin:$PATH"
cd "$(dirname "$0")" || exit 1
folder="$HOME/Desktop/Baking Powder Screenshots"
mkdir -p "$folder" || exit 1
n=1
while [ -e "$folder/Baking Powder $n.png" ]; do n=$((n + 1)); done
file="$folder/Baking Powder $n.png"
cargo run --quiet --target-dir "$HOME/Library/Caches/baking-powder-desktop" -- --screenshot "${1:-1440x900@2}" "$file" || exit 1
echo "$file"
open "$file"
