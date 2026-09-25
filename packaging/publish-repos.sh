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

base_url="${FU_REPO_URL:-https://loonixtools.github.io/face-unlock}"

keyid="$(gpg --list-secret-keys --with-colons | awk -F: '/^sec:/ { print $5; exit }')"
[[ -n $keyid ]] || { echo "$0: no secret key in the keyring" >&2; exit 1; }

mkdir -p "$pages/rpm"
cp -- "$incoming"/*.rpm "$pages/rpm/"

# One APT repository per distribution: each .deb depends on the exact Qt it was
# built against, and its version carries the suffix saying which one that was.
for suite in trixie:deb13 resolute:ubuntu26.04; do
	codename="${suite%%:*}"
	tag="${suite#*:}"
	mkdir -p "$pages/deb/$codename"
	cp -- "$incoming"/*"~${tag}_"*.deb "$pages/deb/$codename/"
	(
		cd "$pages/deb/$codename"
		rm -f Packages Packages.gz Release Release.gpg InRelease
		dpkg-scanpackages --multiversion . > Packages
		gzip -9kf Packages
		apt-ftparchive \
			-o APT::FTPArchive::Release::Origin=face-unlock \
			-o APT::FTPArchive::Release::Label=face-unlock \
			-o APT::FTPArchive::Release::Suite="$codename" \
			-o APT::FTPArchive::Release::Codename="$codename" \
			-o APT::FTPArchive::Release::Architectures=amd64 \
			-o APT::FTPArchive::Release::Components=main \
			release . > Release
		gpg --batch --yes --local-user "$keyid" --clearsign --output InRelease Release
		gpg --batch --yes --local-user "$keyid" --detach-sign --armor --output Release.gpg Release
	)
done

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
sed "s|@BASEURL@|$base_url|g" "$here/packaging/pages/face-unlock.repo" \
	> "$pages/face-unlock.repo"

# Pages would otherwise hand the whole directory to Jekyll, which drops every
# file whose name starts with an underscore and can rewrite the rest.
touch "$pages/.nojekyll"

echo "signed with $keyid"
ls -1 "$pages"/deb/* "$pages/rpm"
