#!/usr/bin/env bash
# Build the harness-server Rust binary in release mode.
#
# Usage:
#   scripts/build-backend.sh                 # release build, stripped
#   scripts/build-backend.sh --no-strip      # skip the strip pass (debugging)
#
# Output:
#   <repo>/backend/target/release/harness-server
#
# Exits non-zero if cargo fails or the binary is missing afterwards.

set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
REPO_ROOT="$(cd "${SCRIPT_DIR}/.." && pwd)"

STRIP=1
for arg in "$@"; do
    case "${arg}" in
        --no-strip) STRIP=0 ;;
        *) echo "unknown argument: ${arg}" >&2; exit 2 ;;
    esac
done

CARGO_BIN="${CARGO:-cargo}"
if ! command -v "${CARGO_BIN}" >/dev/null 2>&1; then
    # Fall back to the standard rustup install location.
    if [[ -x "${HOME}/.cargo/bin/cargo" ]]; then
        CARGO_BIN="${HOME}/.cargo/bin/cargo"
    else
        echo "cargo not found on PATH (set CARGO=/path/to/cargo or install rustup)" >&2
        exit 1
    fi
fi

echo "==> cargo build -p harness-server --release"
"${CARGO_BIN}" build \
    --manifest-path "${REPO_ROOT}/backend/Cargo.toml" \
    -p harness-server \
    --release

BINARY="${REPO_ROOT}/backend/target/release/harness-server"
if [[ ! -x "${BINARY}" ]]; then
    echo "FAIL: cargo succeeded but ${BINARY} is missing or not executable" >&2
    exit 1
fi

if (( STRIP == 1 )); then
    echo "==> strip ${BINARY}"
    # `strip` on macOS is conservative by default and safe to run on
    # release Mach-O binaries — it removes local symbols only.
    strip -x "${BINARY}"
fi

size_bytes="$(stat -f '%z' "${BINARY}")"
printf 'OK  harness-server built (%s bytes) at %s\n' "${size_bytes}" "${BINARY}"
