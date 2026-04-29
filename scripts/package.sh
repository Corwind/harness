#!/usr/bin/env bash
# Assemble the shippable Harness.app: build the Rust sidecar, build the Swift
# bundle, embed the sidecar in Contents/MacOS/, and place the result at
# dist/Harness.app.
#
# Usage:
#   scripts/package.sh              # Debug-configured app, debug-bundled sidecar layout
#   scripts/package.sh --release    # Release configuration end-to-end
#
# Outputs `dist/Harness.app` and prints its absolute path on success.

set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
REPO_ROOT="$(cd "${SCRIPT_DIR}/.." && pwd)"

CONFIGURATION="Debug"
BUILD_APP_FLAGS=()
for arg in "$@"; do
    case "${arg}" in
        --release) CONFIGURATION="Release"; BUILD_APP_FLAGS+=("--release") ;;
        --debug)   CONFIGURATION="Debug" ;;
        *) echo "unknown argument: ${arg}" >&2; exit 2 ;;
    esac
done

DIST_DIR="${REPO_ROOT}/dist"
DIST_APP="${DIST_DIR}/Harness.app"
SIDECAR_BIN="${REPO_ROOT}/backend/target/release/harness-server"

# 1. Build the Rust sidecar (release; the .app always ships the optimised
#    backend even when the Swift side is Debug).
"${SCRIPT_DIR}/build-backend.sh"

# 2. Build the Swift app bundle. Capture the produced .app path from the
#    last line of stdout.
BUILD_LOG="$(mktemp)"
trap 'rm -f "${BUILD_LOG}"' EXIT
if (( ${#BUILD_APP_FLAGS[@]} > 0 )); then
    "${SCRIPT_DIR}/build-app.sh" "${BUILD_APP_FLAGS[@]}" | tee "${BUILD_LOG}"
else
    "${SCRIPT_DIR}/build-app.sh" | tee "${BUILD_LOG}"
fi
APP_SRC="$(tail -n 1 "${BUILD_LOG}")"
if [[ ! -d "${APP_SRC}" ]]; then
    echo "FAIL: build-app.sh produced unexpected output: ${APP_SRC}" >&2
    exit 1
fi

# 3. Stage the .app under dist/ — copy rather than symlink so the result is
#    self-contained and trivially shippable.
mkdir -p "${DIST_DIR}"
rm -rf "${DIST_APP}"
echo "==> staging ${APP_SRC} -> ${DIST_APP}"
cp -R "${APP_SRC}" "${DIST_APP}"

# 4. Embed the sidecar binary in Contents/MacOS/ next to the Swift main
#    executable so resolveProvider() in HarnessApp.swift finds it.
DEST_BIN="${DIST_APP}/Contents/MacOS/harness-server"
echo "==> embedding harness-server into ${DEST_BIN}"
cp "${SIDECAR_BIN}" "${DEST_BIN}"
chmod +x "${DEST_BIN}"

# 5. Sanity: ensure both binaries are present and executable.
for required in "${DIST_APP}/Contents/MacOS/Harness" "${DEST_BIN}"; do
    if [[ ! -x "${required}" ]]; then
        echo "FAIL: ${required} is missing or not executable after staging" >&2
        exit 1
    fi
done

# 6. codesign --display is informational; do not fail the package if the
#    bundle is unsigned, that is by design for local dev. scripts/sign.sh
#    handles signing when an identity is available.
if command -v codesign >/dev/null 2>&1; then
    echo "==> codesign --display (informational)"
    codesign --display --verbose=2 "${DIST_APP}" 2>&1 | sed 's/^/    /' || true
fi

printf 'OK  Harness.app staged at %s\n' "${DIST_APP}"
printf '%s\n' "${DIST_APP}"
