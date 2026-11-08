#!/usr/bin/env bash
# Cross-compile CotwMod.exe for Windows from Linux.
# Needs: mingw-w64, a Rust toolchain with rust-src (uses -Zbuild-std).
set -euo pipefail
RUSTBIN="${RUSTBIN:-/usr/lib/rust-1.97/bin}"
export PATH="$RUSTBIN:$PATH" RUSTC_BOOTSTRAP=1
cargo build --release --target x86_64-pc-windows-gnu -Zbuild-std=std,panic_abort "$@"
ls -la target/x86_64-pc-windows-gnu/release/CotwMod.exe
