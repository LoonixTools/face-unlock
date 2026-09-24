#!/usr/bin/env bash
#
# Collects every translatable string into po/plasma-face-unlock.pot and brings
# the translations up to date with it. One catalog serves all four parts:
# the shell code (pfu_msg), the agent (i18n in C++ and QML) and the PAM module
# (dgettext), so a word is translated once wherever it shows up.
#
# The settings labels and headings live in an array in menu.sh and reach
# gettext at run time, so xgettext cannot see them; they are pulled out here
# first.

set -euo pipefail

cd -- "$(dirname -- "${BASH_SOURCE[0]}")/.."
tmp="$(mktemp -d)"
trap 'rm -rf -- "$tmp"' EXIT

version="$(make -s version)"

# The labels, as calls xgettext understands, pointing back at menu.sh.
awk -F'|' '/^\t"(user|sys|pam)\|/ { sub(/"$/, "", $5); print "pfu_msg \"" $5 "\"" }
          /^\t"group\|/ { sub(/"$/, "", $2); print "pfu_msg \"" $2 "\"" }' src/lib/menu.sh > "$tmp/settings.sh"

common=(--from-code=UTF-8 --add-comments=TRANSLATORS --package-name=plasma-face-unlock --package-version="$version"
        --msgid-bugs-address=https://github.com/LoonixTools/plasma-face-unlock/issues)

xgettext "${common[@]}" -L Shell -k --keyword=pfu_msg --keyword=pfu_msg_into:2 --keyword=pfu_msg_in:2 \
	-o "$tmp/shell.pot" src/plasma-face-unlock src/lib/*.sh "$tmp/settings.sh"
sed -i "s|$tmp/settings.sh:[0-9]*|src/lib/menu.sh|g" "$tmp/shell.pot"
xgettext "${common[@]}" -L C++ --keyword=i18n --keyword=_ -o "$tmp/native.pot" src/agent/*.cpp src/pam/*.c
xgettext "${common[@]}" -L JavaScript --keyword=i18n -o "$tmp/qml.pot" src/agent/qml/*.qml

msgcat --use-first -o po/plasma-face-unlock.pot "$tmp/shell.pot" "$tmp/native.pot" "$tmp/qml.pot"
sed -i -e '1i # Translation template for plasma-face-unlock.\n# Copyright (C) 2026 Felitendo\n# This file is distributed under the same license as plasma-face-unlock.' -e '1,4d' \
	po/plasma-face-unlock.pot

for po in po/*.po; do
	msgmerge --quiet --update --backup=none --no-fuzzy-matching "$po" po/plasma-face-unlock.pot
done
echo "po/plasma-face-unlock.pot: $(grep -c '^msgid' po/plasma-face-unlock.pot) strings"
