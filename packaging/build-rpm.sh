#!/usr/bin/env bash
#
# Builds the binary package for Fedora into dist/.
#
# The spec takes the version as a macro rather than carrying one of its own,
# for the same reason the Debian control file has a placeholder: the Makefile
# is where the version is written down. The two networks are sources of their
# own, downloaded (and checked) by `make models` before rpmbuild sees them.
#
# Needs: rpmbuild, make, cmake, a C++ compiler, the -devel packages the spec
# lists, msgfmt (gettext), scdoc, curl, systemd-rpm-macros.

set -euo pipefail

here="$(cd -- "$(dirname -- "${BASH_SOURCE[0]}")/.." && pwd)"
version="${1:-$(make -s -C "$here" version)}"
name=face-unlock

command -v rpmbuild > /dev/null || { echo "$0: rpmbuild is not installed" >&2; exit 1; }

make -C "$here" models

top="$(mktemp -d)"
trap 'rm -rf -- "$top"' EXIT
mkdir -p "$top"/{BUILD,BUILDROOT,RPMS,SOURCES,SPECS}

# The working tree as it is, not as it was committed: a package built from a
# checkout has to contain what is in that checkout.
tar czf "$top/SOURCES/$name-$version.tar.gz" \
	--transform "s,^\\.,$name-$version," \
	--exclude=./.git --exclude=./dist --exclude=./build --exclude=./models --exclude=./po/'*.mo' \
	-C "$here" .
cp "$here"/models/*.onnx "$top/SOURCES/"

rpmbuild \
	--define "_topdir $top" \
	--define "_version $version" \
	-bb "$here/packaging/rpm/$name.spec" > /dev/null

mkdir -p "$here/dist"
find "$top/RPMS" -name '*.rpm' -exec cp {} "$here/dist/" \;

ls "$here/dist/$name-$version"*.rpm
