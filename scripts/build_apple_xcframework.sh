#!/usr/bin/env bash
set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
ROOT_DIR="$(cd "${SCRIPT_DIR}/.." && pwd)"

echo "=== mm-dlp Apple Universal & iOS XCFramework Builder ==="

TARGET_MACOS_ARM64="aarch64-apple-darwin"
TARGET_MACOS_X86="x86_64-apple-darwin"
TARGET_IOS_ARM64="aarch64-apple-ios"
TARGET_IOS_SIM_ARM64="aarch64-apple-ios-sim"
TARGET_IOS_SIM_X86="x86_64-apple-ios"

cd "${ROOT_DIR}"

SWIFT_OUT="${ROOT_DIR}/bindings/swift/Sources/MmDlpCore"
FRAMEWORK_OUT="${ROOT_DIR}/bindings/swift/Frameworks"
mkdir -p "${SWIFT_OUT}"
mkdir -p "${FRAMEWORK_OUT}"

echo "Generating Swift bindings via uniffi-bindgen..."
cargo run -p mm-dlp-cli --bin uniffi-bindgen -- generate "${ROOT_DIR}/mm-dlp-core/src/mm-dlp.udl" --language swift --out-dir "${SWIFT_OUT}" || true

if [[ "$(uname -s)" == "Darwin" ]]; then
    echo "Host is macOS. Proceeding with target builds and XCFramework packaging..."

    # Ensure rust targets are installed
    rustup target add "${TARGET_MACOS_ARM64}" "${TARGET_MACOS_X86}" "${TARGET_IOS_ARM64}" "${TARGET_IOS_SIM_ARM64}" "${TARGET_IOS_SIM_X86}" || true

    echo "Compiling macOS targets..."
    cargo build --release -p mm-dlp-core --target "${TARGET_MACOS_ARM64}"
    cargo build --release -p mm-dlp-core --target "${TARGET_MACOS_X86}"

    mkdir -p "${ROOT_DIR}/target/universal-macos"
    lipo -create \
        "${ROOT_DIR}/target/${TARGET_MACOS_ARM64}/release/libmm_dlp_core.a" \
        "${ROOT_DIR}/target/${TARGET_MACOS_X86}/release/libmm_dlp_core.a" \
        -output "${ROOT_DIR}/target/universal-macos/libmm_dlp_core.a"

    echo "Compiling iOS Device and Simulator targets..."
    cargo build --release -p mm-dlp-core --target "${TARGET_IOS_ARM64}"
    cargo build --release -p mm-dlp-core --target "${TARGET_IOS_SIM_ARM64}"
    cargo build --release -p mm-dlp-core --target "${TARGET_IOS_SIM_X86}"

    mkdir -p "${ROOT_DIR}/target/universal-iossim"
    lipo -create \
        "${ROOT_DIR}/target/${TARGET_IOS_SIM_ARM64}/release/libmm_dlp_core.a" \
        "${ROOT_DIR}/target/${TARGET_IOS_SIM_X86}/release/libmm_dlp_core.a" \
        -output "${ROOT_DIR}/target/universal-iossim/libmm_dlp_core.a"

    HEADER_DIR="${SWIFT_OUT}"
    rm -rf "${FRAMEWORK_OUT}/MmDlpCore.xcframework"

    xcodebuild -create-xcframework \
        -library "${ROOT_DIR}/target/universal-macos/libmm_dlp_core.a" -headers "${HEADER_DIR}" \
        -library "${ROOT_DIR}/target/${TARGET_IOS_ARM64}/release/libmm_dlp_core.a" -headers "${HEADER_DIR}" \
        -library "${ROOT_DIR}/target/universal-iossim/libmm_dlp_core.a" -headers "${HEADER_DIR}" \
        -output "${FRAMEWORK_OUT}/MmDlpCore.xcframework"

    echo "=== Successfully built MmDlpCore.xcframework ==="
else
    echo "Current host is $(uname -s). Cross-building instructions:"
    echo "Run this script on a macOS runner or CI agent with Xcode installed."
fi
