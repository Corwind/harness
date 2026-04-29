#!/usr/bin/env bash
# Submit Harness.app for Apple notarisation and staple the result.
#
# Usage:
#   HARNESS_NOTARY_PROFILE=harness-notary scripts/notarize.sh [path/to/Harness.app]
#
# Required: a notarytool keychain profile created once per developer with
#
#   xcrun notarytool store-credentials harness-notary \
#       --apple-id you@example.com \
#       --team-id  ABCD123456 \
#       --password app-specific-password
#
# Defaults:
#   bundle path  → dist/Harness.app
#
# This script is documented but never invoked in CI: notarisation requires
# real Apple ID credentials that must not live in the repo. Run it locally
# from a release branch after scripts/sign.sh produces a hardened-runtime
# signed bundle.

set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
REPO_ROOT="$(cd "${SCRIPT_DIR}/.." && pwd)"

APP_PATH="${1:-${REPO_ROOT}/dist/Harness.app}"
PROFILE="${HARNESS_NOTARY_PROFILE:-}"

if [[ -z "${PROFILE}" ]]; then
    echo "FAIL: HARNESS_NOTARY_PROFILE is not set" >&2
    echo "      run: xcrun notarytool store-credentials <name> ..." >&2
    exit 2
fi

if [[ ! -d "${APP_PATH}" ]]; then
    echo "FAIL: bundle not found at ${APP_PATH}" >&2
    exit 2
fi

if ! command -v xcrun >/dev/null 2>&1; then
    echo "xcrun not found — install Xcode command-line tools" >&2
    exit 1
fi

# notarytool only accepts .zip / .pkg / .dmg uploads; wrap the bundle in a
# zip in a tempdir so we don't leave artefacts in dist/.
WORKDIR="$(mktemp -d)"
trap 'rm -rf "${WORKDIR}"' EXIT
ZIP_PATH="${WORKDIR}/Harness.zip"

echo "==> ditto ${APP_PATH} -> ${ZIP_PATH}"
ditto -c -k --keepParent "${APP_PATH}" "${ZIP_PATH}"

echo "==> xcrun notarytool submit (--wait)"
xcrun notarytool submit "${ZIP_PATH}" \
    --keychain-profile "${PROFILE}" \
    --wait

echo "==> xcrun stapler staple"
xcrun stapler staple "${APP_PATH}"

echo "==> xcrun stapler validate"
xcrun stapler validate "${APP_PATH}"

printf 'OK  %s notarised + stapled (profile "%s")\n' "${APP_PATH}" "${PROFILE}"
