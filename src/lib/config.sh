# shellcheck shell=bash
#
# Reading and writing the settings files.
#
# Two of them, in the same format: ~/.config/plasma-face-unlock/config for
# what this user wants from the lock screen and the bubble, and
# /etc/plasma-face-unlock/config for what the daemon does (which camera, how
# strict), which only root writes. Neither is meant to be edited by hand:
# every option is in the menu.
#
# Both are parsed rather than sourced. One "Key=Value" per line, '#'
# comments. The daemon and the agent read them with the same rules (see
# src/core/keyvalue.cpp).

declare -A PFU_KV_CACHE=()

# _pfu_kv_lookup <file> <Key> [default]
# Result in PFU_KV_VALUE. Assigning rather than printing matters on the
# settings screen, which reads every key on every frame: a command
# substitution there is a fork, and forks are the whole cost of a redraw.
PFU_KV_VALUE=''

_pfu_kv_lookup() {
	local file="$1" key="$2" default="${3:-}" val='' line content

	PFU_KV_VALUE="$default"
	[[ -r $file ]] || return 0

	if [[ -n ${PFU_KV_CACHE[$file]+set} ]]; then
		content="${PFU_KV_CACHE[$file]}"
	else
		content="$(< "$file")"
		PFU_KV_CACHE[$file]="$content"
	fi

	while IFS= read -r line; do
		[[ $line == *"$key"* ]] || continue
		[[ $line =~ ^[[:space:]]*"$key"[[:space:]]*=(.*)$ ]] || continue
		val="${BASH_REMATCH[1]}"
	done <<< "$content"

	val="${val%%#*}"
	val="${val#"${val%%[![:space:]]*}"}"
	val="${val%"${val##*[![:space:]]}"}"
	val="${val%\"}"
	val="${val#\"}"

	[[ -n $val ]] && PFU_KV_VALUE="$val"
	return 0
}

pfu_is_true() {
	case "${1,,}" in
		yes|y|true|1|on|enabled) return 0 ;;
		*) return 1 ;;
	esac
}

# pfu_kv_set <file> <Key> <Value> <header line>
pfu_kv_set() {
	local file="$1" key="$2" value="$3" header="$4" tmp

	if [[ ! -e $file ]]; then
		mkdir -p "$(dirname "$file")" || return 1
		{
			printf '# %s\n' "$header"
			printf '#\n'
			printf '# Written by `%s`. Nothing here needs editing by hand:\n' "$PFU_NAME"
			printf '# every option is in the menu.\n'
		} > "$file" || return 1
	fi
	[[ -w $file ]] || return 1

	tmp="$(mktemp "${file}.XXXXXX")" || return 1
	chmod --reference="$file" "$tmp" 2>/dev/null || chmod 0644 "$tmp"

	if grep -qE "^[[:space:]]*#?[[:space:]]*${key}[[:space:]]*=" "$file"; then
		awk -v key="$key" -v value="$value" '
			!done && $0 ~ "^[[:space:]]*#?[[:space:]]*" key "[[:space:]]*=" {
				print key "=" value; done = 1; next
			}
			# drop any further occurrences so the file cannot grow duplicates
			$0 ~ "^[[:space:]]*" key "[[:space:]]*=" { next }
			{ print }
		' "$file" > "$tmp" || { rm -f "$tmp"; return 1; }
	else
		cat "$file" > "$tmp" || { rm -f "$tmp"; return 1; }
		printf '%s=%s\n' "$key" "$value" >> "$tmp"
	fi

	# Replaced, not rewritten in place: the agent watches the directory and
	# picks up the new file the moment it lands.
	mv -f "$tmp" "$file"
	unset 'PFU_KV_CACHE[$file]'
}

# ---------------------------------------------------------------------------
# This user's settings
# ---------------------------------------------------------------------------

pfu_config_get() {
	_pfu_kv_lookup "$PFU_CONFIG" "$@"
	printf '%s\n' "$PFU_KV_VALUE"
}

pfu_config_set() {
	pfu_kv_set "$PFU_CONFIG" "$1" "$2" "$PFU_PRETTY"
}

# ---------------------------------------------------------------------------
# The system settings (read by anybody, written by root)
# ---------------------------------------------------------------------------

pfu_sys_get() {
	_pfu_kv_lookup "$PFU_SYSCONFIG" "$@"
	printf '%s\n' "$PFU_KV_VALUE"
}

# pfu_config_load
# Everything the menu shows, resolved once per screen.
pfu_config_load() {
	PFU_KV_CACHE=()

	_pfu_kv_lookup "$PFU_CONFIG" Enabled no;          CFG_ENABLED=no;  pfu_is_true "$PFU_KV_VALUE" && CFG_ENABLED=yes
	_pfu_kv_lookup "$PFU_CONFIG" LockScreen yes;      CFG_LOCK=no;     pfu_is_true "$PFU_KV_VALUE" && CFG_LOCK=yes
	_pfu_kv_lookup "$PFU_CONFIG" Sudo no;             CFG_SUDO=no;     pfu_is_true "$PFU_KV_VALUE" && CFG_SUDO=yes
	_pfu_kv_lookup "$PFU_CONFIG" Polkit no;           CFG_POLKIT=no;   pfu_is_true "$PFU_KV_VALUE" && CFG_POLKIT=yes

	_pfu_kv_lookup "$PFU_SYSCONFIG" Liveness heavy;   CFG_LIVENESS="$PFU_KV_VALUE"
	_pfu_kv_lookup "$PFU_SYSCONFIG" Camera auto;      CFG_CAMERA="$PFU_KV_VALUE"
	return 0
}
