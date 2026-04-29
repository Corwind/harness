#!/usr/bin/env bash
# Behavior test for the packaging pipeline (T3.3).
#
# Asserts the layout of dist/Harness.app after `make app` (or the equivalent
# scripts/package.sh run). This script is the test that drives T3.3 — it MUST
# be runnable independently and fail loudly when any expected artefact is
# missing.
#
# Usage:
#   scripts/test-app.sh                 # asserts ./dist/Harness.app
#   scripts/test-app.sh path/to.app     # asserts the given bundle
#
# Exit codes:
#   0  every assertion passed
#   1  one or more assertions failed
#   2  bad invocation (missing argument, bundle not found)

set -euo pipefail

# Resolve the repo root so the script works regardless of cwd.
SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
REPO_ROOT="$(cd "${SCRIPT_DIR}/.." && pwd)"

APP_PATH="${1:-${REPO_ROOT}/dist/Harness.app}"

if [[ ! -d "${APP_PATH}" ]]; then
    echo "FAIL: bundle not found at ${APP_PATH}" >&2
    echo "      run 'make app' first" >&2
    exit 2
fi

fail_count=0
pass_count=0

check_path() {
    local kind="$1"  # "file" or "dir" or "exec"
    local path="$2"
    local label="$3"
    case "${kind}" in
        file)
            if [[ -f "${path}" ]]; then
                printf '  ok  %s\n' "${label}"
                pass_count=$((pass_count + 1))
            else
                printf 'FAIL  %s (expected file at %s)\n' "${label}" "${path}" >&2
                fail_count=$((fail_count + 1))
            fi
            ;;
        dir)
            if [[ -d "${path}" ]]; then
                printf '  ok  %s\n' "${label}"
                pass_count=$((pass_count + 1))
            else
                printf 'FAIL  %s (expected directory at %s)\n' "${label}" "${path}" >&2
                fail_count=$((fail_count + 1))
            fi
            ;;
        exec)
            if [[ -x "${path}" && -f "${path}" ]]; then
                printf '  ok  %s\n' "${label}"
                pass_count=$((pass_count + 1))
            else
                printf 'FAIL  %s (expected executable file at %s)\n' "${label}" "${path}" >&2
                fail_count=$((fail_count + 1))
            fi
            ;;
    esac
}

echo "Verifying bundle layout at ${APP_PATH}"

check_path dir  "${APP_PATH}/Contents"                  "Contents/"
check_path file "${APP_PATH}/Contents/Info.plist"       "Contents/Info.plist"
check_path dir  "${APP_PATH}/Contents/MacOS"            "Contents/MacOS/"
check_path exec "${APP_PATH}/Contents/MacOS/Harness"    "Contents/MacOS/Harness (Swift app binary)"
check_path exec "${APP_PATH}/Contents/MacOS/harness-server" \
                                                        "Contents/MacOS/harness-server (Rust sidecar)"
check_path dir  "${APP_PATH}/Contents/Resources"        "Contents/Resources/"

# Info.plist sanity: bundle identifier must be set and the executable name
# must point at the Swift binary that exists in MacOS/.
if [[ -f "${APP_PATH}/Contents/Info.plist" ]]; then
    bundle_id="$(/usr/libexec/PlistBuddy -c 'Print :CFBundleIdentifier' "${APP_PATH}/Contents/Info.plist" 2>/dev/null || echo '')"
    bundle_exec="$(/usr/libexec/PlistBuddy -c 'Print :CFBundleExecutable' "${APP_PATH}/Contents/Info.plist" 2>/dev/null || echo '')"

    if [[ -n "${bundle_id}" ]]; then
        printf '  ok  Info.plist CFBundleIdentifier = %s\n' "${bundle_id}"
        pass_count=$((pass_count + 1))
    else
        printf 'FAIL  Info.plist missing CFBundleIdentifier\n' >&2
        fail_count=$((fail_count + 1))
    fi

    if [[ "${bundle_exec}" == "Harness" ]]; then
        printf '  ok  Info.plist CFBundleExecutable = Harness\n'
        pass_count=$((pass_count + 1))
    else
        printf 'FAIL  Info.plist CFBundleExecutable = %s (expected Harness)\n' "${bundle_exec:-<unset>}" >&2
        fail_count=$((fail_count + 1))
    fi
fi

# The sidecar binary must report sensible Mach-O metadata: arm64 and a
# macOS host. We don't run it (handshake requires env), but we confirm
# it is a real Mach-O the OS will load.
if [[ -x "${APP_PATH}/Contents/MacOS/harness-server" ]]; then
    if file "${APP_PATH}/Contents/MacOS/harness-server" | grep -q 'Mach-O'; then
        printf '  ok  harness-server is a Mach-O binary\n'
        pass_count=$((pass_count + 1))
    else
        printf 'FAIL  harness-server is not a Mach-O binary\n' >&2
        fail_count=$((fail_count + 1))
    fi
fi

printf '\n%d passed, %d failed\n' "${pass_count}" "${fail_count}"

if (( fail_count > 0 )); then
    exit 1
fi
exit 0
