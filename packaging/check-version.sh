#!/usr/bin/env bash
#
# Refuses a release whose tag and Makefile disagree.
#
# The version is baked into the program at install time from the Makefile, and
# the packages take theirs from the same place. But the tag is what people see
# and what the release is named after. A tag that says something else produces
# a package called 1.0.4 containing a program that reports 1.0.3, and nothing
# would have complained.
#
# Anything that is not a v-tag (a run started by hand from a branch) is not a
# release and has nothing to check.

set -euo pipefail

here="$(cd -- "$(dirname -- "${BASH_SOURCE[0]}")/.." && pwd)"
ref="${1:-}"

case "$ref" in
	v[0-9]*) ;;
	*)
		echo "not a release tag (${ref:-none}), nothing to check against"
		exit 0
		;;
esac

tag_version="${ref#v}"
make_version="$(make -s -C "$here" version)"

if [[ $tag_version != "$make_version" ]]; then
	echo "tag $ref says $tag_version, the Makefile says $make_version" >&2
	echo "Bump VERSION in the Makefile to match the tag, or retag." >&2
	exit 1
fi

echo "$ref matches the Makefile"
