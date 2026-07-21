#!/usr/bin/env bash
set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
ROOT_DIR="$(cd "${SCRIPT_DIR}/.." && pwd)"
MSCRAPER_ROOT="$(cd "${ROOT_DIR}/.." && pwd)"

echo "=== Building mm-dlp-core for Android targets ==="

TARGET_AARCH64="aarch64-linux-android"
TARGET_X86_64="x86_64-linux-android"

cd "${ROOT_DIR}"

if command -v cargo-ndk >/dev/null 2>&1 && [ -n "${ANDROID_NDK_HOME:-}" ]; then
    echo "Building ${TARGET_AARCH64} and ${TARGET_X86_64} with cargo-ndk..."
    cargo ndk -t ${TARGET_AARCH64} -t ${TARGET_X86_64} -o "${ROOT_DIR}/target/android_libs" build --release -p mm-dlp-core || echo "NDK compilation skipped or failed"
else
    echo "cargo-ndk or ANDROID_NDK_HOME not present; skipping cross-compilation step..."
fi

echo "=== Generating Kotlin UniFFI Bindings ==="
OUTPUT_DIR="${ROOT_DIR}/bindings/kotlin"
mkdir -p "${OUTPUT_DIR}"

cargo run -p mm-dlp-cli --bin uniffi-bindgen -- generate "${ROOT_DIR}/mm-dlp-core/src/lib.rs" --language kotlin --out-dir "${OUTPUT_DIR}" || \
cargo run -p mm-dlp-cli --bin uniffi-bindgen -- generate "${ROOT_DIR}/mm-dlp-core/src/mm-dlp.udl" --language kotlin --out-dir "${OUTPUT_DIR}" || true

echo "=== Copying artifacts to Android project ==="
ANDROID_KOTLIN_DIR="${MSCRAPER_ROOT}/app/src/main/java/com/mscraper/mmdlp"
ANDROID_JNILIBS_DIR="${MSCRAPER_ROOT}/app/src/main/jniLibs"

mkdir -p "${ANDROID_KOTLIN_DIR}"
mkdir -p "${ANDROID_JNILIBS_DIR}/arm64-v8a"
mkdir -p "${ANDROID_JNILIBS_DIR}/x86_64"

if [ -d "${OUTPUT_DIR}/com/mscraper/mmdlp" ]; then
    cp -r "${OUTPUT_DIR}/com/mscraper/mmdlp/"* "${ANDROID_KOTLIN_DIR}/" || true
elif [ -f "${OUTPUT_DIR}/mm_dlp_core.kt" ]; then
    cp "${OUTPUT_DIR}/mm_dlp_core.kt" "${ANDROID_KOTLIN_DIR}/" || true
    cp "${OUTPUT_DIR}/mm_dlp_core.kt" "${MSCRAPER_ROOT}/app/src/main/java/com/example/core/ffi/" || true
elif [ -f "${OUTPUT_DIR}/mmdlp.kt" ]; then
    cp "${OUTPUT_DIR}/mmdlp.kt" "${ANDROID_KOTLIN_DIR}/" || true
    cp "${OUTPUT_DIR}/mmdlp.kt" "${MSCRAPER_ROOT}/app/src/main/java/com/example/core/ffi/mm_dlp_core.kt" || true
fi

if [ -f "${ROOT_DIR}/target/android_libs/arm64-v8a/libmm_dlp_core.so" ]; then
    cp "${ROOT_DIR}/target/android_libs/arm64-v8a/libmm_dlp_core.so" "${ANDROID_JNILIBS_DIR}/arm64-v8a/"
fi

if [ -f "${ROOT_DIR}/target/android_libs/x86_64/libmm_dlp_core.so" ]; then
    cp "${ROOT_DIR}/target/android_libs/x86_64/libmm_dlp_core.so" "${ANDROID_JNILIBS_DIR}/x86_64/"
fi

echo "=== Binding generation & asset placement complete ==="
