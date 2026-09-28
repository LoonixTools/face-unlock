#!/usr/bin/env bash
#
# Builds the tarball for Arch Linux into dist/. The AUR package
# face-unlock-bin is made from it.
#
# Like the deb and the rpm, it holds what `make install` produced, under a
# folder named like the tarball. One thing is different: OpenCV is linked in
# statically (packaging/build-opencv.sh), so a new OpenCV on Arch does not
# break the daemon. Qt stays shared. The agent uses Qt's private API, so the
# tarball fits the Qt that Arch had when it was built.
#
# Needs: make, cmake, a C++ compiler, the packages check.yml installs (without
# opencv), msgfmt (gettext), scdoc, curl, zstd, and the static OpenCV:
#
#   packaging/build-opencv.sh && packaging/build-tarball.sh

set -euo pipefail

here="$(cd -- "$(dirname -- "${BASH_SOURCE[0]}")/.." && pwd)"
version="${1:-$(make -s -C "$here" version)}"
opencv="${OPENCV_PREFIX:-$here/build/opencv-static}"
name="face-unlock-$version-arch-$(uname -m)"

for tool in msgfmt scdoc cmake readelf zstd; do
	command -v "$tool" > /dev/null || { echo "$0: $tool is not installed" >&2; exit 1; }
done
opencv_dir="$(dirname "$(find "$opencv" -name OpenCVConfig.cmake -print -quit 2>/dev/null)")"
[[ $opencv_dir != . ]] || { echo "$0: no OpenCV in $opencv, run packaging/build-opencv.sh first" >&2; exit 1; }

root="$(mktemp -d)"
work="$(mktemp -d)"
trap 'rm -rf -- "$root" "$work"' EXIT
dest="$root/$name"

make -C "$here" models
make -C "$here" install \
	DESTDIR="$dest" \
	PREFIX=/usr \
	VERSION="$version" \
	BUILDDIR="$work/build" \
	PAMDIR=/usr/lib/security \
	SYSTEMUNITDIR=/usr/lib/systemd/system \
	USERUNITDIR=/usr/lib/systemd/user \
	CMAKE_FLAGS="-DOpenCV_DIR=$opencv_dir"

# The tests, and the models on a real face: the part a smaller OpenCV could
# break without anything else noticing.
ctest --test-dir "$work/build" --output-on-failure
face="$opencv/share/face-unlock-check/face.jpg"
"$work/build/test_images" "$dest/usr/share/face-unlock/models" "$face" "$face" | tee "$work/faces"
grep -q 'similarity' "$work/faces" || { echo "$0: the models found no face" >&2; exit 1; }

if readelf -d "$dest/usr/lib/face-unlock/face-unlockd" | grep -q 'libopencv'; then
	echo "$0: the daemon still loads a shared OpenCV" >&2
	exit 1
fi

# The program is GPL, OpenCV and the libraries in it come with their own
# licences, and the copyright file names the models' authors and licences.
lic="$dest/usr/share/licenses/face-unlock"
install -Dm644 "$here/LICENSE" "$lic/LICENSE"
install -Dm644 "$here/packaging/deb/copyright" "$lic/copyright"
cp -r "$opencv/share/licenses/opencv" "$lic/opencv"

mkdir -p "$here/dist"
out="$here/dist/$name.tar.zst"
tar -C "$root" --owner=0 --group=0 --numeric-owner --sort=name \
	-I 'zstd -19 -T0' -cf "$out" "$name"

echo "$out"
