#!/usr/bin/env bash
# Build a Debian package for ft991a-radio-control.
# Usage: ./packaging/build-deb.sh [--skip-build]
#
# Outputs: ft991a-radio-control_<version>_amd64.deb in the project root.
#
# Mirrors ts570d/packaging/build-deb.sh's exact structure (see
# docs/adr/0005-debian-and-windows-packaging.md) — same staging-tree
# approach, same DEP-5 copyright generation from LICENSE.txt, same
# --skip-build contract (required by radio-cat-rs's shared
# release-app.yml workflow, which builds binaries itself first and then
# calls this script with --skip-build).
#
# `pin-test` is NOT a workspace member of this repo — it lives in
# radio-cat-rs's `cat-transport-serial` crate (radio-cat-rs ADR 0006 §6)
# and is consumed here as a git dependency. `cargo build -p <package>
# --bin <bin>` selects by package ID across the *whole resolved dependency
# graph*, not only workspace members, so `cargo build --release -p
# cat-transport-serial --bin pin-test` works from inside this repo despite
# `cat-transport-serial` never appearing in this repo's own
# `[workspace] members` list — verified directly (not just asserted) by
# running it and confirming `target/release/pin-test` appears, the exact
# path this script (and ts570d's identical script) expects it at.

set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
ROOT="$(cd "${SCRIPT_DIR}/.." && pwd)"

VERSION="$(grep '^version' "${ROOT}/Cargo.toml" | head -1 | sed 's/.*= *"\(.*\)"/\1/')"
ARCH="amd64"
PKG="ft991a-radio-control_${VERSION}_${ARCH}"
STAGING="${ROOT}/target/debian/${PKG}"

# ── 1. Build release binaries ────────────────────────────────────────────────
if [[ "${1:-}" != "--skip-build" ]]; then
    echo "==> cargo build --release"
    (cd "${ROOT}" && cargo build --release)
    echo "==> cargo build --release -p cat-transport-serial --bin pin-test"
    (cd "${ROOT}" && cargo build --release -p cat-transport-serial --bin pin-test)
fi

RELEASE="${ROOT}/target/release"

# ── 2. Stage package tree ────────────────────────────────────────────────────
echo "==> Staging into ${STAGING}"
rm -rf "${STAGING}"
install -d "${STAGING}/DEBIAN"
install -d "${STAGING}/usr/bin"
install -d "${STAGING}/usr/share/doc/ft991a-radio-control"
install -d "${STAGING}/usr/share/man/man1"

# Binaries — rename to final installed names
install -m 0755 "${RELEASE}/ft991a"    "${STAGING}/usr/bin/ft991a-control"
install -m 0755 "${RELEASE}/emulator"  "${STAGING}/usr/bin/ft991a-emulator"
install -m 0755 "${RELEASE}/pin-test"  "${STAGING}/usr/bin/rs232c-pintest"

# Control file (substitute version)
sed "s/^Version:.*/Version: ${VERSION}/" \
    "${SCRIPT_DIR}/DEBIAN/control" > "${STAGING}/DEBIAN/control"

# Copyright — DEP-5 format with full Apache 2.0 license text from LICENSE.txt
{
    cat <<HEADER
Format: https://www.debian.org/doc/packaging-manuals/copyright-format/1.0/
Upstream-Name: ft991a-radio-control
Upstream-Contact: Matt Franklin <radiombf@gmail.com>
Source: https://github.com/kf0uwv/ft991a

Files: *
Copyright: 2026 Matt Franklin <radiombf@gmail.com>
License: Apache-2.0

License: Apache-2.0
HEADER
    # Indent every line of the license text by one space (DEP-5 requirement).
    # Blank lines become a single " ." to preserve paragraph breaks.
    sed 's/^$/ ./; s/^/ /' "${ROOT}/LICENSE.txt"
} > "${STAGING}/usr/share/doc/ft991a-radio-control/copyright"

# ── 3. Build .deb ────────────────────────────────────────────────────────────
OUT="${ROOT}/${PKG}.deb"
echo "==> dpkg-deb --build ${STAGING} ${OUT}"
dpkg-deb --build "${STAGING}" "${OUT}"

echo ""
echo "Package built: ${OUT}"
echo ""
dpkg-deb --info "${OUT}"
echo ""
echo "Contents:"
dpkg-deb --contents "${OUT}"
