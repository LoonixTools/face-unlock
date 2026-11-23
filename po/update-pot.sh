#!/usr/bin/env bash
#
# Collects every translatable string into po/face-unlock.pot and brings the
# translations up to date with it. One catalog serves every part: the
# command, the window, the agent and the daemon (tr and trf), and the PAM
# module (say), so a word is translated once wherever it shows up.

set -euo pipefail

cd -- "$(dirname -- "${BASH_SOURCE[0]}")/.."

version="$(make -s version)"

mapfile -t files < <(find src gui/src agent/src daemon/src vision/src pam/src -name '*.rs' | LC_ALL=C sort)

xgettext --from-code=UTF-8 --add-comments=TRANSLATORS --package-name=face-unlock --package-version="$version" \
	--msgid-bugs-address=https://github.com/LoonixTools/face-unlock/issues \
	-L Rust -k --keyword=tr --keyword=trf --keyword=say:3 \
	-o po/face-unlock.pot "${files[@]}"
sed -i -e '1i # Translation template for face-unlock.\n# Copyright (C) 2026 Felitendo\n# This file is distributed under the same license as face-unlock.' -e '1,4d' \
	po/face-unlock.pot

for po in po/*.po; do
	msgmerge --quiet --update --backup=none --no-fuzzy-matching "$po" po/face-unlock.pot
done
echo "po/face-unlock.pot: $(grep -c '^msgid' po/face-unlock.pot) strings"
