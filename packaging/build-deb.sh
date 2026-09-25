#!/usr/bin/env bash
#
# Builds the binary package for Debian, Ubuntu and their derivatives into
# dist/.
#
# Everything the package contains comes out of `make install`. This only wraps
# what that produced, so there is exactly one description of where a file goes
# and it is the Makefile.
#
# Unlike the shell-only tools, this one is compiled, so the package is for one
# architecture and its library dependencies are read off the binaries by
# dpkg-shlibdeps rather than written down by hand.
#
# Needs: make, cmake, a C++ compiler, the -dev packages the README lists,
# dpkg-dev, msgfmt (gettext), scdoc, curl.

set -euo pipefail

here="$(cd -- "$(dirname -- "${BASH_SOURCE[0]}")/.." && pwd)"
version="${1:-$(make -s -C "$here" version)}"
# The package depends on the exact Qt of the distribution it is built on, so
# each one gets a build of its own, told apart by a suffix: ~deb13, ~ubuntu26.04.
debversion="$version${DEB_SUFFIX:-}"
name=face-unlock

# A package without its man page or its translations is not a package this
# should be quietly willing to produce.
for tool in msgfmt scdoc dpkg-deb dpkg-shlibdeps cmake; do
	command -v "$tool" > /dev/null || { echo "$0: $tool is not installed" >&2; exit 1; }
done

root="$(mktemp -d)"
work="$(mktemp -d)"
trap 'rm -rf -- "$root" "$work"' EXIT

make -C "$here" models
make -C "$here" install \
	DESTDIR="$root" \
	PREFIX=/usr \
	VERSION="$version" \
	BUILDDIR="$work/build" \
	SYSTEMUNITDIR=/usr/lib/systemd/system \
	USERUNITDIR=/usr/lib/systemd/user

arch="$(dpkg --print-architecture)"

# The shared libraries the binaries need, as package names.
mkdir -p "$work/debian"
printf 'Source: %s\n\nPackage: %s\nArchitecture: any\n' "$name" "$name" > "$work/debian/control"
mapfile -t elves < <(find "$root" -type f \( -name '*.so' -o -perm -u+x \) -exec sh -c 'head -c4 "$1" | grep -q ELF' _ {} \; -print)
depends="$(cd "$work" && dpkg-shlibdeps -O "${elves[@]/#/-e}" 2>/dev/null | sed -n 's/^shlibs:Depends=//p')"
[[ -n $depends ]] || { echo "$0: dpkg-shlibdeps found no dependencies" >&2; exit 1; }

install -d "$root/DEBIAN"
sed -e "s|@VERSION@|$debversion|g" -e "s|@ARCH@|$arch|g" -e "s|@DEPENDS@|$depends|g" \
	"$here/packaging/deb/control" > "$root/DEBIAN/control"
install -Dm644 "$here/packaging/deb/copyright" "$root/usr/share/doc/$name/copyright"

# The daemon's socket is switched on by the program itself, the first time it
# is turned on, so there is nothing to do as root at install time. Removing
# the package stops it.
cat > "$root/DEBIAN/prerm" <<'SH'
#!/bin/sh
set -e
if [ "$1" = remove ] && [ -d /run/systemd/system ]; then
	systemctl disable --now face-unlockd.socket face-unlockd.service >/dev/null 2>&1 || true
fi
SH
chmod 755 "$root/DEBIAN/prerm"

# Faces, settings and PAM lines of plasma-face-unlock, the old name.
cat > "$root/DEBIAN/postinst" <<'SH'
#!/bin/sh
set -e
if [ "$1" = configure ]; then
	face-unlock --root migrate || true
fi
SH
chmod 755 "$root/DEBIAN/postinst"

( cd "$root" && find . -type f ! -path './DEBIAN/*' -printf '%P\0' \
	| LC_ALL=C sort -z | xargs -0 md5sum > DEBIAN/md5sums )

mkdir -p "$here/dist"
out="$here/dist/${name}_${debversion}_${arch}.deb"
dpkg-deb --root-owner-group --build "$root" "$out" > /dev/null

echo "$out"
