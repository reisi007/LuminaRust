#!/bin/sh
# Bundle third-party license texts + written source offers into a release
# bundle (F-078-R3/R4). Copies THIRD-PARTY-NOTICES.md and everything under
# licenses/ into DEST, verifies every required file exists, and writes a
# SHA256 manifest. No network access, no silent fallbacks: a missing file
# aborts with a non-zero exit status.
#
# Usage:
#   scripts/release/bundle-licenses.sh [DEST_DIR]
#
# DEST_DIR defaults to "<repo-root>/dist/licenses". Run from anywhere.
set -eu

ROOT=$(CDPATH= cd -- "$(dirname -- "$0")/../.." && pwd)
DEST="${1:-$ROOT/dist/licenses}"

REQUIRED="
THIRD-PARTY-NOTICES.md
licenses/README.md
licenses/libraw/COPYRIGHT
licenses/libraw/LICENSE.LGPL
licenses/libraw/LICENSE.CDDL
licenses/libraw/SOURCE-OFFER.md
licenses/lensfun/COPYING.LGPL-3.0
licenses/lensfun/COPYING.CC-BY-SA-3.0
licenses/lensfun/SOURCE-OFFER.md
licenses/models/BiRefNet-LICENSE-MIT.txt
licenses/models/SAM-2-LICENSE-Apache-2.0.txt
licenses/models/ONNXRuntime-LICENSE-MIT.txt
licenses/models/MODEL-SOURCE-OFFER.md
"

missing=0
for rel in $REQUIRED; do
    if [ ! -f "$ROOT/$rel" ]; then
        echo "ERROR: required license artifact missing: $ROOT/$rel" >&2
        missing=1
    fi
done
if [ "$missing" -ne 0 ]; then
    echo "Aborting: license inventory incomplete (see errors above)." >&2
    exit 1
fi

rm -rf "$DEST"
mkdir -p "$DEST/licenses"

cp "$ROOT/THIRD-PARTY-NOTICES.md" "$DEST/THIRD-PARTY-NOTICES.md"
for rel in $REQUIRED; do
    case "$rel" in
        THIRD-PARTY-NOTICES.md) continue ;;
    esac
    mkdir -p "$DEST/$(dirname "$rel")"
    cp "$ROOT/$rel" "$DEST/$rel"
done

# SHA256 manifest over the bundled license payload (paths relative to licenses/).
( cd "$ROOT/licenses" && find . -type f -print | LC_ALL=C sort \
    | xargs shasum -a 256 ) > "$DEST/licenses/CHECKSUMS.sha256"
cp "$DEST/licenses/CHECKSUMS.sha256" "$DEST/CHECKSUMS.sha256"

echo "License bundle written to: $DEST"
echo "Files:"
(cd "$DEST" && find . -type f | LC_ALL=C sort | sed 's/^/  /')
echo
echo "Verify integrity with:"
echo "  (cd '$DEST/licenses' && shasum -a 256 -c CHECKSUMS.sha256)"
