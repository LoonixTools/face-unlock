#!/usr/bin/env bash
#
# Builds the GitHub release notes for a tag, the way big projects such as
# Immich lay them out: the hand-written entry from CHANGELOG.md (a welcome,
# the highlights), a support section, then every commit since the last
# release, sorted by kind, and a link to the full changelog.
#
#   .github/release-notes.sh v1.2.0           the notes
#   .github/release-notes.sh --title v1.2.0   the title (checks the entry exists)
#
# Commit authors come from the GitHub API through gh. Without it the list
# goes without them.

set -euo pipefail
cd -- "$(dirname -- "${BASH_SOURCE[0]}")/.."

title=0
if [[ ${1:-} == --title ]]; then
	title=1
	shift
fi
tag="${1:?usage: release-notes.sh [--title] vX.Y.Z}"

if ! grep -qx "## $tag" CHANGELOG.md; then
	echo "CHANGELOG.md has no entry for $tag. Add \"## $tag\" first." >&2
	exit 1
fi
if (( title )); then
	printf '%s\n' "$tag"
	exit 0
fi

repo="${GITHUB_REPOSITORY:-$(git remote get-url origin | sed -E 's#^.*github\.com[:/]##; s#\.git$##')}"
name="${repo#*/}"

# The entry: up to the next one, without the date line (GitHub shows the
# date), one heading level up, outside code blocks.
entry="$(awk -v h="## $tag" '
	$0 == h { on = 1; next }
	on && /^## / { exit }
	!on { next }
	/^```/ { code = !code }
	!code && /^_[0-9]{4}-[0-9]{2}-[0-9]{2}_$/ { next }
	!code && /^###/ { sub(/^#/, "") }
	{ print }
' CHANGELOG.md | sed -e '/./,$!d')"

# The commits: from the last release, or from the start. A tag that does not
# exist yet is a preview of what HEAD would become.
if git rev-parse -q --verify "refs/tags/$tag" > /dev/null; then
	target="$tag"
	prev="$(git describe --tags --abbrev=0 --match 'v[0-9]*' "$tag^" 2>/dev/null || true)"
else
	target="$(git rev-parse HEAD)"
	prev="$(git describe --tags --abbrev=0 --match 'v[0-9]*' HEAD 2>/dev/null || true)"
fi

declare -A login=()
if command -v gh > /dev/null; then
	if [[ -n $prev ]]; then
		api=(api "repos/$repo/compare/$prev...$target" --jq '.commits[] | [.sha, (.author.login // "")] | @tsv')
	else
		api=(api --paginate "repos/$repo/commits?sha=$target&per_page=100" --jq '.[] | [.sha, (.author.login // "")] | @tsv')
	fi
	while IFS=$'\t' read -r sha who; do
		[[ -n $who ]] && login[$sha]="$who"
	done < <(gh "${api[@]}" 2>/dev/null || true)
fi

declare -A list=()
count_maint=0
while IFS=$'\t' read -r sha subject body; do
	# Version bumps say nothing a reader needs.
	[[ $subject =~ ^(chore:\ )?($name\ )?v?[0-9]+\.[0-9]+\.[0-9]+$ ]] && continue

	case "$subject" in
		*!:*) kind=breaking ;;
		feat:* | feat\(*) kind=feat ;;
		fix:* | fix\(*) kind=fix ;;
		perf:* | perf\(*) kind=enh ;;
		docs:* | docs\(*) kind=docs ;;
		chore* | ci:* | ci\(* | build* | refactor* | test* | style*) kind=maint ;;
		# Older commits without a prefix, sorted by what they say.
		*README* | *readme* | *Readme*) kind=docs ;;
		Add\ * | Put\ * | Introduce\ *) kind=feat ;;
		Fix\ * | Repair\ * | Recover\ * | Stop\ *) kind=fix ;;
		*\ *) kind=enh ;;
		*) kind=maint ;;
	esac
	[[ $body == *"BREAKING CHANGE"* ]] && kind=breaking
	[[ $subject == "Initial commit" ]] && kind=maint

	line="* $subject"
	[[ -n ${login[$sha]:-} ]] && line+=" by @${login[$sha]}"
	line+=" in https://github.com/$repo/commit/$sha"
	list[$kind]+="$line"$'\n'
	[[ $kind == maint ]] && count_maint=$((count_maint + 1))
done < <(git log --reverse --no-merges --format='%H%x09%s%x09%b%x1e' "${prev:+$prev..}$target" | tr '\n\036' ' \n' | sed 's/^ //')

changes=''
for pair in "breaking:🚨 Breaking Changes" "feat:🚀 Features" "enh:🌟 Enhancements" "fix:🐛 Bug fixes" "docs:📚 Documentation"; do
	kind="${pair%%:*}"
	[[ -n ${list[$kind]:-} ]] || continue
	changes+="### ${pair#*:}"$'\n'"${list[$kind]}"
done
if [[ -n ${list[maint]:-} ]]; then
	changes+=$'\n'"<details><summary>🧰 Maintenance ($count_maint)</summary>"$'\n\n'"${list[maint]}"$'\n'"</details>"$'\n'
fi

if [[ -n $prev ]]; then
	full="**Full Changelog**: https://github.com/$repo/compare/$prev...$tag"
else
	full="**Full Changelog**: https://github.com/$repo/commits/$tag"
fi

# A release with highlights is a big one: it gets a heading and the support
# section, as in Immich. A patch is a sentence or two and the list.
if grep -q '^## Highlights$' <<< "$entry"; then
	cat <<EOF
# 🚀 $name $tag

$entry

## ☕ Support $name

If $name is useful to you, you can buy me a coffee. It keeps these tools going. Found a bug or have an idea? Tell me in the [issues](https://github.com/$repo/issues).

<a href="https://ko-fi.com/felitendo"><img src="https://storage.ko-fi.com/cdn/kofi5.png?v=6" alt="Buy me a coffee on Ko-fi" height="48"></a>

----

EOF
else
	printf '%s\n\n' "$entry"
fi
printf "## What's Changed\n%s\n\n%s\n" "$changes" "$full"
