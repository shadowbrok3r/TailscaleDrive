#!/usr/bin/env bash
set -euo pipefail

ROOT="$(cd "$(dirname "$0")/.." && pwd)"
RUST="$ROOT/rust"
GEN="$RUST/generated"

SWIFT_DST="$ROOT/Sources/TailscaleDrive"
C_DST="$ROOT/Sources/BridgeFFI/include"
LIB_DST="$ROOT/RustLibs"

# 1) Build Rust for device
cargo build --manifest-path "$RUST/Cargo.toml" --target aarch64-apple-ios --release

# 2) Remove stale generated bindings from the Swift target
#    Only delete files that are produced by swift-bridge and that you copy in.
rm -f \
  "$SWIFT_DST/SwiftBridgeCore.swift" \
  "$SWIFT_DST/tailscale-drive.swift"

# If you sometimes have other generated swift files, delete them too:
find "$SWIFT_DST" -maxdepth 1 -type f -name '*.swift' \
  -print0 | while IFS= read -r -d '' f; do
    base="$(basename "$f")"
    case "$base" in
      SwiftBridgeCore.swift|tailscale-drive.swift) : ;;  # already removed above
      ContentView.swift|TailscaleDriveApp.swift|RendererHandle.swift|BridgeFFIImport.swift) : ;; # keep app files
      *) rm -f "$f" ;; # remove any other stray generated swift
    esac
  done

# 3) Copy generated Swift into the Swift target (flattened)
mkdir -p "$SWIFT_DST"

cp -f "$GEN/SwiftBridgeCore.swift" "$SWIFT_DST/SwiftBridgeCore.swift"

# Copy all other generated .swift files (flatten to basename)
find "$GEN" -type f -name '*.swift' ! -name 'SwiftBridgeCore.swift' -print0 \
  | while IFS= read -r -d '' f; do
      cp -f "$f" "$SWIFT_DST/$(basename "$f")"
    done

# 4) Ensure generated Swift can see the C declarations from the BridgeFFI module
for f in "$SWIFT_DST/SwiftBridgeCore.swift" "$SWIFT_DST/tailscale-drive.swift"; do
  if [[ -f "$f" ]] && ! head -n 5 "$f" | grep -q '^import BridgeFFI'; then
    tmp="$(mktemp)"
    printf "import BridgeFFI\n\n" > "$tmp"
    cat "$f" >> "$tmp"
    mv "$tmp" "$f"
  fi
done

# 5) Copy the static library
mkdir -p "$LIB_DST"

# Copy whatever staticlib was produced (avoid hardcoded name mismatch)
LIB="$(find "$RUST/target/aarch64-apple-ios/release" -maxdepth 1 -type f -name 'lib*.a' | head -n 1)"
if [[ -z "${LIB:-}" ]]; then
  echo "ERROR: No .a produced in $RUST/target/aarch64-apple-ios/release"
  echo "Ensure Cargo.toml has [lib] crate-type = [\"staticlib\"]"
  exit 1
fi
cp -f "$LIB" "$LIB_DST/"

# 6) Copy generated C headers for BridgeFFI
mkdir -p "$C_DST"

cp -f "$GEN/SwiftBridgeCore.h" "$C_DST/SwiftBridgeCore.h"

# If your module header path/name changes, copy by search instead of hardcoding:
HDR="$(find "$GEN" -type f -name '*.h' | grep -v 'SwiftBridgeCore.h' | head -n 1)"
if [[ -z "${HDR:-}" ]]; then
  echo "ERROR: No module .h found under $GEN"
  exit 1
fi
cp -f "$HDR" "$C_DST/tailscale-drive.h"

# Umbrella header (optional but fine to keep consistent)
cat > "$C_DST/bridging-header.h" <<'EOF'
#pragma once
#include "SwiftBridgeCore.h"
#include "tailscale-drive.h"
EOF
