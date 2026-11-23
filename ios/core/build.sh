#!/bin/sh
# Xcode pre-build step: the Rust core for the platform being built, into $BUILT_PRODUCTS_DIR (linked as -ltermix_core).
set -e
# rustup's toolchain has the iOS targets (a Homebrew rustc earlier in PATH has none).
export PATH="$HOME/.cargo/bin:$PATH"
# Xcode's SDKROOT is the iOS SDK: build scripts and proc macros compiled for the Mac would link against it.
# Without it, cc/rustc ask xcrun for the right SDK per target.
unset SDKROOT
case "$PLATFORM_NAME" in
  iphonesimulator) target=aarch64-apple-ios-sim ;;
  *) target=aarch64-apple-ios ;;
esac
profile=debug
if [ "$CONFIGURATION" = Release ]; then profile=release; fi
cd "$(dirname "$0")"
cargo build --target "$target" $([ $profile = release ] && echo --release)
mkdir -p "$BUILT_PRODUCTS_DIR"
cp "target/$target/$profile/libtermix_core.a" "$BUILT_PRODUCTS_DIR/"
