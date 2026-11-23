#!/usr/bin/env bash
#
# Builds the tarball for Arch Linux into dist/. The AUR package
# face-unlock-bin is made from it.
#
# Like the deb and the rpm, it holds what `make install` produced, under a
# folder named like the tarball.
#
# Needs: make, cargo, a C compiler, pam, libxkbcommon, scdoc, curl, zstd.

set -euo pipefail

here="$(cd -- "$(dirname -- "${BASH_SOURCE[0]}")/.." && pwd)"
version="${1:-$(make -s -C "$here" version)}"
name="face-unlock-$version-arch-$(uname -m)"

# The flags Arch builds its own packages with: hardening, and full RELRO.
if [[ -f /etc/makepkg.conf ]]; then
	# shellcheck source=/dev/null
	source /etc/makepkg.conf
	export CFLAGS LDFLAGS RUSTFLAGS
fi

for tool in scdoc cargo zstd; do
	command -v "$tool" > /dev/null || { echo "$0: $tool is not installed" >&2; exit 1; }
done

root="$(mktemp -d)"
trap 'rm -rf -- "$root"' EXIT
dest="$root/$name"

make -C "$here" models
make -C "$here" build PREFIX=/usr PAMDIR=/usr/lib/security
make -C "$here" install \
	DESTDIR="$dest" \
	PREFIX=/usr \
	PAMDIR=/usr/lib/security \
	SYSTEMUNITDIR=/usr/lib/systemd/system \
	USERUNITDIR=/usr/lib/systemd/user

# The program is GPL, and the copyright file names the models' authors and
# licences.
lic="$dest/usr/share/licenses/face-unlock"
install -Dm644 "$here/LICENSE" "$lic/LICENSE"
install -Dm644 "$here/packaging/deb/copyright" "$lic/copyright"

mkdir -p "$here/dist"
out="$here/dist/$name.tar.zst"
tar -C "$root" --owner=0 --group=0 --numeric-owner --sort=name \
	-I 'zstd -19 -T0' -cf "$out" "$name"

echo "$out"
