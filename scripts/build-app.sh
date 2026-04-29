#!/usr/bin/env bash
# Build the macOS Harness.app bundle.
#
# Two paths exist:
#   1. xcodebuild — the canonical production route. Generates the .xcodeproj
#      via xcodegen, then runs xcodebuild against it. Produces a fully Apple-
#      blessed bundle layout, asset catalogs, etc. Required for codesigning,
#      notarisation, and any artefact that ships to users.
#   2. swiftc fallback — used only when xcodebuild can't run on this host
#      (broken Xcode plug-ins, missing CoreSimulator framework, no Xcode at
#      all). Compiles every Swift source under macos/Sources/Harness with a
#      direct swiftc invocation and hand-assembles the .app skeleton. Good
#      enough for local dev, headless agents, and XCUITest harnesses;
#      cannot replace xcodebuild for shipping.
#
# Usage:
#   scripts/build-app.sh                 # Debug, prefer xcodebuild
#   scripts/build-app.sh --release       # Release, prefer xcodebuild
#   FORCE_SWIFTC=1     scripts/build-app.sh   # skip xcodebuild
#   FORCE_XCODEBUILD=1 scripts/build-app.sh   # never fall back; fail if xcodebuild does
#
# Environment overrides:
#   XCODEGEN  — path to xcodegen (default: xcodegen on PATH)
#
# Outputs the absolute path of the produced .app on stdout's last line so
# downstream scripts can capture it via tail -n 1.

set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
REPO_ROOT="$(cd "${SCRIPT_DIR}/.." && pwd)"
MACOS_DIR="${REPO_ROOT}/macos"

CONFIGURATION="Debug"
for arg in "$@"; do
    case "${arg}" in
        --release) CONFIGURATION="Release" ;;
        --debug)   CONFIGURATION="Debug" ;;
        *) echo "unknown argument: ${arg}" >&2; exit 2 ;;
    esac
done

FORCE_SWIFTC="${FORCE_SWIFTC:-0}"
FORCE_XCODEBUILD="${FORCE_XCODEBUILD:-0}"
if [[ "${FORCE_SWIFTC}" == "1" && "${FORCE_XCODEBUILD}" == "1" ]]; then
    echo "FAIL: FORCE_SWIFTC and FORCE_XCODEBUILD cannot both be set" >&2
    exit 2
fi

# -----------------------------------------------------------------------------
# Path 1 — xcodebuild
# -----------------------------------------------------------------------------
try_xcodebuild() {
    local xcodegen_bin="${XCODEGEN:-xcodegen}"
    if ! command -v "${xcodegen_bin}" >/dev/null 2>&1; then
        echo "    xcodegen not found ('${xcodegen_bin}')" >&2
        return 1
    fi
    if ! command -v xcodebuild >/dev/null 2>&1; then
        echo "    xcodebuild not found" >&2
        return 1
    fi

    echo "==> xcodegen generate" >&2
    ( cd "${MACOS_DIR}" && "${xcodegen_bin}" generate --quiet )

    local derived="${MACOS_DIR}/build"
    echo "==> xcodebuild -scheme Harness -configuration ${CONFIGURATION}" >&2
    if ! xcodebuild \
        -project "${MACOS_DIR}/Harness.xcodeproj" \
        -scheme Harness \
        -configuration "${CONFIGURATION}" \
        -derivedDataPath "${derived}" \
        CODE_SIGN_IDENTITY="" \
        CODE_SIGNING_REQUIRED=NO \
        CODE_SIGNING_ALLOWED=NO \
        build \
        >&2; then
        return 1
    fi

    local app="${derived}/Build/Products/${CONFIGURATION}/Harness.app"
    if [[ ! -d "${app}" ]]; then
        echo "    xcodebuild succeeded but ${app} is missing" >&2
        return 1
    fi
    echo "OK  built Harness.app via xcodebuild (${CONFIGURATION})" >&2
    printf '%s\n' "${app}"
}

# -----------------------------------------------------------------------------
# Path 2 — swiftc fallback
# -----------------------------------------------------------------------------
try_swiftc() {
    if ! command -v swiftc >/dev/null 2>&1; then
        echo "    swiftc not found" >&2
        return 1
    fi

    local stage="${MACOS_DIR}/build/SwiftcFallback/${CONFIGURATION}"
    local app="${stage}/Harness.app"
    rm -rf "${stage}"
    mkdir -p "${app}/Contents/MacOS" "${app}/Contents/Resources"

    # Optimisation level mirrors the configuration: -Onone for Debug so we
    # don't pay 10x compile time per dev iteration; -O for Release so the
    # produced bundle is fit to ship.
    local opt_flag="-Onone"
    if [[ "${CONFIGURATION}" == "Release" ]]; then
        opt_flag="-O"
    fi

    echo "==> swiftc fallback (${CONFIGURATION}) — compiling macos/Sources/Harness" >&2
    local sources_file
    sources_file="$(mktemp)"
    # shellcheck disable=SC2064
    trap "rm -f '${sources_file}'" RETURN
    find "${MACOS_DIR}/Sources/Harness" -name '*.swift' -print0 > "${sources_file}"

    if ! xargs -0 swiftc \
            -disable-sandbox \
            "${opt_flag}" \
            -target arm64-apple-macos14.0 \
            -module-name Harness \
            -emit-executable \
            -o "${app}/Contents/MacOS/Harness" \
            < "${sources_file}" \
            >&2; then
        return 1
    fi

    # Render Info.plist statically. The xcodebuild path uses build-variable
    # substitution ($(EXECUTABLE_NAME), $(PRODUCT_BUNDLE_IDENTIFIER), etc.);
    # swiftc has no build system above it, so we resolve those ourselves.
    cat > "${app}/Contents/Info.plist" <<'PLIST'
<?xml version="1.0" encoding="UTF-8"?>
<!DOCTYPE plist PUBLIC "-//Apple//DTD PLIST 1.0//EN" "http://www.apple.com/DTDs/PropertyList-1.0.dtd">
<plist version="1.0">
<dict>
    <key>CFBundleName</key>
    <string>Harness</string>
    <key>CFBundleDisplayName</key>
    <string>Harness</string>
    <key>CFBundleIdentifier</key>
    <string>com.harness.Harness</string>
    <key>CFBundleVersion</key>
    <string>1</string>
    <key>CFBundleShortVersionString</key>
    <string>0.1.0</string>
    <key>CFBundlePackageType</key>
    <string>APPL</string>
    <key>CFBundleExecutable</key>
    <string>Harness</string>
    <key>CFBundleInfoDictionaryVersion</key>
    <string>6.0</string>
    <key>LSMinimumSystemVersion</key>
    <string>14.0</string>
    <key>NSHighResolutionCapable</key>
    <true/>
    <key>NSPrincipalClass</key>
    <string>NSApplication</string>
</dict>
</plist>
PLIST

    echo "OK  built Harness.app via swiftc fallback (${CONFIGURATION})" >&2
    printf '%s\n' "${app}"
}

# -----------------------------------------------------------------------------
# Dispatch
# -----------------------------------------------------------------------------
APP_PATH=""

if [[ "${FORCE_SWIFTC}" == "1" ]]; then
    echo "==> FORCE_SWIFTC=1 set; skipping xcodebuild" >&2
    if APP_PATH="$(try_swiftc)"; then
        :
    else
        echo "FAIL: swiftc fallback failed" >&2
        exit 1
    fi
else
    echo "==> attempting xcodebuild path" >&2
    if APP_PATH="$(try_xcodebuild)"; then
        :
    elif [[ "${FORCE_XCODEBUILD}" == "1" ]]; then
        echo "FAIL: xcodebuild failed and FORCE_XCODEBUILD=1 forbids fallback" >&2
        exit 1
    else
        echo "==> xcodebuild path failed; falling back to swiftc" >&2
        if APP_PATH="$(try_swiftc)"; then
            :
        else
            echo "FAIL: both xcodebuild and swiftc paths failed" >&2
            exit 1
        fi
    fi
fi

# Last line of stdout is the absolute path — callers parse it with tail -1.
printf '%s\n' "${APP_PATH}"
