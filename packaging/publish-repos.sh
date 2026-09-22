#!/usr/bin/env bash
#
# Puts the packages that were just built into the APT and RPM repositories on
# the gh-pages branch, and regenerates the indexes over everything that is
# there.
#
# Old versions are kept rather than replaced. An index built over all of them
# is what lets somebody pin a version or go back to one, and it costs a few
# hundred kilobytes.
#
# Usage: publish-repos.sh <gh-pages checkout> <directory of new packages>
#
# Needs: dpkg-dev, apt-utils, createrepo-c, gpg, and a secret key already
# imported. Its id is taken from the keyring.

set -euo pipefail

pages="$(cd -- "$1" && pwd)"
incoming="$(cd -- "$2" && pwd)"
here="$(cd -- "$(dirname -- "${BASH_SOURCE[0]}")/.." && pwd)"

base_url="${PFU_REPO_URL:-https://loonixtools.github.io/plasma-face-unlock}"

keyid="$(gpg --list-secret-keys --with-colons | awk -F: '/^sec:/ { print $5; exit }')"
[[ -n $keyid ]] || { echo "$0: no secret key in the keyring" >&2; exit 1; }

mkdir -p "$pages/deb" "$pages/rpm"
cp -- "$incoming"/*.deb "$pages/deb/"
cp -- "$incoming"/*.rpm "$pages/rpm/"

# ---------------------------------------------------------------------------
# APT
# ---------------------------------------------------------------------------
# A flat repository: the packages and their index sit in one directory and the
# sources line ends in "./". There is one distribution here and it is the same
# package for all of them, so the suite and component machinery of a pool
# layout would describe nothing.
(
	cd "$pages/deb"

	# The old index must be gone before the new one is written: apt-ftparchive
	# hashes every file in the directory, and a Release that hashes the
	# previous Release is a Release that cannot be verified.
	rm -f Packages Packages.gz Release Release.gpg InRelease

	dpkg-scanpackages --multiversion . > Packages
	gzip -9kf Packages

	apt-ftparchive \
		-o APT::FTPArchive::Release::Origin=plasma-face-unlock \
		-o APT::FTPArchive::Release::Label=plasma-face-unlock \
		-o APT::FTPArchive::Release::Suite=stable \
		-o APT::FTPArchive::Release::Codename=stable \
		-o APT::FTPArchive::Release::Architectures=amd64 \
		-o APT::FTPArchive::Release::Components=main \
		release . > Release

	# Both signatures: InRelease is what current apt fetches, Release.gpg is
	# what an older one falls back to.
	gpg --batch --yes --local-user "$keyid" --clearsign --output InRelease Release
	gpg --batch --yes --local-user "$keyid" --detach-sign --armor --output Release.gpg Release
)

# ---------------------------------------------------------------------------
# RPM
# ---------------------------------------------------------------------------
(
	cd "$pages/rpm"
	createrepo_c --quiet --update .
	rm -f repodata/repomd.xml.asc
	gpg --batch --yes --local-user "$keyid" --detach-sign --armor repodata/repomd.xml
)

# ---------------------------------------------------------------------------
# The key and the landing page
# ---------------------------------------------------------------------------
gpg --armor --export "$keyid" > "$pages/KEY.gpg"

sed "s|@BASEURL@|$base_url|g" "$here/packaging/pages/index.html" > "$pages/index.html"
sed "s|@BASEURL@|$base_url|g" "$here/packaging/pages/plasma-face-unlock.repo" \
	> "$pages/plasma-face-unlock.repo"

# Pages would otherwise hand the whole directory to Jekyll, which drops every
# file whose name starts with an underscore and can rewrite the rest.
touch "$pages/.nojekyll"

echo "signed with $keyid"
ls -1 "$pages/deb" "$pages/rpm"
