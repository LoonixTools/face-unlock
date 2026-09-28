#!/usr/bin/env bash
#
# Builds the OpenCV the Arch tarball links in, as static libraries.
#
# Arch moves to a new OpenCV now and then, each with a new soname, and a
# daemon linked against the old one then no longer starts. Linked in
# statically, the daemon brings its own. Only the modules face-unlock uses are
# built, with OpenCV's own copies of zlib, libjpeg and libpng, so nothing is
# left to load at run time. The camera is read through V4L2, which OpenCV
# talks to directly.
#
#   packaging/build-opencv.sh [PREFIX]
#
# Installs into PREFIX (build/opencv-static by default) and does nothing when
# PREFIX already holds what this script builds. Needs: cmake, a C++ compiler,
# curl.

set -euo pipefail

here="$(cd -- "$(dirname -- "${BASH_SOURCE[0]}")/.." && pwd)"
prefix="${1:-$here/build/opencv-static}"

# 4.x, like the deb and the rpm. The static dnn of 5.0.0 does not link
# (https://github.com/opencv/opencv/issues/29342).
version=4.14.0
sha256=ee8fb9b30eb60850431b4656447080e3737b56e45719c92b67f245950609f86e

# The flags Arch builds its own packages with: hardening, and full RELRO.
if [[ -f /etc/makepkg.conf ]]; then
	# shellcheck source=/dev/null
	source /etc/makepkg.conf
	export CFLAGS CXXFLAGS LDFLAGS
fi

# A change to this script or to the flags is a different build.
stamp="$({ cat "${BASH_SOURCE[0]}"; echo "${CFLAGS:-} ${CXXFLAGS:-} ${LDFLAGS:-}"; } | sha256sum | cut -d' ' -f1)"
if [[ -f $prefix/.stamp && $(<"$prefix/.stamp") == "$stamp" ]]; then
	echo "$prefix already has this OpenCV"
	exit 0
fi
# It is emptied before the install, so it has to be one of ours.
if [[ -e $prefix && ! -f $prefix/.stamp ]]; then
	echo "$0: $prefix exists and was not made by this script" >&2
	exit 1
fi

work="$(mktemp -d)"
trap 'rm -rf -- "$work"' EXIT

curl -fL --retry 3 -o "$work/opencv.tar.gz" \
	"https://github.com/opencv/opencv/archive/refs/tags/$version.tar.gz"
echo "$sha256  $work/opencv.tar.gz" | sha256sum -c --quiet -
tar -xzf "$work/opencv.tar.gz" -C "$work"
src="$work/opencv-$version"

# BUILD_LIST adds what the listed modules need (calib3d, features2d, flann).
# The rest is switched off because it would be picked up from the system or
# downloaded: codecs, video backends, IPP and the other accelerators.
cmake -S "$src" -B "$work/build" \
	-DCMAKE_BUILD_TYPE=Release \
	-DCMAKE_INSTALL_PREFIX="$prefix" \
	-DBUILD_SHARED_LIBS=OFF \
	-DBUILD_LIST=core,imgproc,imgcodecs,videoio,objdetect,dnn \
	-DBUILD_TESTS=OFF -DBUILD_PERF_TESTS=OFF -DBUILD_EXAMPLES=OFF \
	-DBUILD_DOCS=OFF -DBUILD_opencv_apps=OFF -DBUILD_JAVA=OFF \
	-DBUILD_ZLIB=ON -DBUILD_JPEG=ON -DBUILD_PNG=ON -DBUILD_PROTOBUF=ON \
	-DWITH_V4L=ON \
	-DWITH_FFMPEG=OFF -DWITH_GSTREAMER=OFF -DWITH_OBSENSOR=OFF \
	-DWITH_TIFF=OFF -DWITH_WEBP=OFF -DWITH_AVIF=OFF -DWITH_OPENEXR=OFF \
	-DWITH_OPENJPEG=OFF -DWITH_JASPER=OFF \
	-DWITH_IPP=OFF -DWITH_ITT=OFF -DWITH_OPENCL=OFF -DWITH_VA=OFF -DWITH_VA_INTEL=OFF \
	-DWITH_LAPACK=OFF -DWITH_EIGEN=OFF -DWITH_FLATBUFFERS=OFF -DWITH_QUIRC=OFF -DWITH_ADE=OFF
cmake --build "$work/build" --parallel "$(nproc)"

rm -rf -- "$prefix"
cmake --install "$work/build"

# OpenCV installs the licences of the libraries in it, but not its own.
mv "$prefix"/share/licenses/opencv* "$prefix/share/licenses/opencv"
install -Dm644 "$src/LICENSE" "$prefix/share/licenses/opencv/LICENSE"

# A picture with a face, for the check that the models load and find one.
install -Dm644 "$src/samples/data/messi5.jpg" "$prefix/share/face-unlock-check/face.jpg"
echo "$stamp" > "$prefix/.stamp"
