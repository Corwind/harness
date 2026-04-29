#!/usr/bin/env bash
# Codesign Harness.app and its embedded sidecar with the hardened runtime.
#
# Usage:
#   HARNESS_SIGN_IDENTITY="Developer ID Application: ..." \
#   HARNESS_ENTITLEMENTS=macos/Sources/Harness/Resources/Harness.entitlements \
#       scripts/sign.sh [path/to/Harness.app]
#
# Defaults:
#   HARNESS_SIGN_IDENTITY  → "Apple Development"
#   HARNESS_ENTITLEMENTS   → macos/Sources/Harness/Resources/Harness.entitlements
#   bundle path            → dist/Harness.app
#
# Notes:
#   * --options runtime turns on the hardened runtime, which is required by
#     notarisation.
#   * --deep applies signatures to nested binaries (the harness-server
#     sidecar in Contents/MacOS/). We sign the sidecar first explicitly so
#     the embedded entitlements line up before the outer signature is
#     produced; this matches Apple's recommended layered approach.
#   * For local-dev unsigned builds, set HARNESS_SIGN_IDENTITY=- (the ad-hoc
#     identity). codesign accepts it but the hardened runtime cannot be
#     stapled with notarisation.

set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
REPO_ROOT="$(cd "${SCRIPT_DIR}/.." && pwd)"

APP_PATH="${1:-${REPO_ROOT}/dist/Harness.app}"
IDENTITY="${HARNESS_SIGN_IDENTITY:-Apple Development}"
ENTITLEMENTS="${HARNESS_ENTITLEMENTS:-${REPO_ROOT}/macos/Sources/Harness/Resources/Harness.entitlements}"

if [[ ! -d "${APP_PATH}" ]]; then
    echo "FAIL: bundle not found at ${APP_PATH}" >&2
    echo "      run scripts/package.sh first" >&2
    exit 2
fi

if [[ ! -f "${ENTITLEMENTS}" ]]; then
    echo "FAIL: entitlements file not found at ${ENTITLEMENTS}" >&2
    exit 2
fi

if ! command -v codesign >/dev/null 2>&1; then
    echo "codesign not found — install Xcode command-line tools" >&2
    exit 1
fi

SIDECAR="${APP_PATH}/Contents/MacOS/harness-server"
MAIN_EXE="${APP_PATH}/Contents/MacOS/Harness"

echo "==> signing ${SIDECAR}"
codesign \
    --force \
    --options runtime \
    --timestamp \
    --entitlements "${ENTITLEMENTS}" \
    --sign "${IDENTITY}" \
    "${SIDECAR}"

echo "==> signing ${MAIN_EXE}"
codesign \
    --force \
    --options runtime \
    --timestamp \
    --entitlements "${ENTITLEMENTS}" \
    --sign "${IDENTITY}" \
    "${MAIN_EXE}"

echo "==> deep-signing bundle ${APP_PATH}"
codesign \
    --force \
    --options runtime \
    --timestamp \
    --entitlements "${ENTITLEMENTS}" \
    --sign "${IDENTITY}" \
    --deep \
    "${APP_PATH}"

echo "==> codesign --verify --deep --strict"
codesign --verify --deep --strict --verbose=2 "${APP_PATH}"

echo "==> codesign --display"
codesign --display --verbose=2 "${APP_PATH}" 2>&1 | sed 's/^/    /'

printf 'OK  %s signed with identity "%s"\n' "${APP_PATH}" "${IDENTITY}"
