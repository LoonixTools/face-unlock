# shellcheck shell=bash
#
# Paths, translations and output helpers.
#
# The shell side never touches the camera or the face data. Those belong to
# the daemon, and everything here that needs them asks it through
# plasma-face-unlock-ctl. What this side does write is the user's own settings
# file, and (through sudo, and only when asked) the system settings and the
# PAM files of sudo and polkit.

PFU_VERSION="@VERSION@"
PFU_NAME="plasma-face-unlock"
PFU_PRETTY="Plasma Face Unlock"

PFU_LIBDIR="${PFU_LIBDIR:-@LIBDIR@}"
PFU_LIBEXECDIR="${PFU_LIBEXECDIR:-@LIBEXECDIR@}"
PFU_LOCALEDIR="${PFU_LOCALEDIR:-@LOCALEDIR@}"
PFU_PAMDIR="${PFU_PAMDIR:-@PAMDIR@}"

PFU_CTL="${PFU_CTL:-$PFU_LIBEXECDIR/plasma-face-unlock-ctl}"
PFU_AGENT="${PFU_AGENT:-$PFU_LIBEXECDIR/plasma-face-unlock-agent}"
PFU_PAM_MODULE="${PFU_PAM_MODULE:-$PFU_PAMDIR/pam_plasma_face_unlock.so}"

PFU_XDG_CONFIG="${XDG_CONFIG_HOME:-$HOME/.config}"
PFU_CONFDIR="${PFU_CONFDIR:-${PFU_XDG_CONFIG}/${PFU_NAME}}"
PFU_CONFIG="${PFU_CONFIG:-${PFU_CONFDIR}/config}"

# The system settings. Only root writes them; see system.sh.
PFU_SYSCONFIG="${PFU_SYSCONFIG:-/etc/${PFU_NAME}/config}"

PFU_UNIT_SOCKET="plasma-face-unlockd.socket"
PFU_UNIT_AGENT="plasma-face-unlock-agent.service"

# ---------------------------------------------------------------------------
# Translations
# ---------------------------------------------------------------------------

export TEXTDOMAIN="plasma-face-unlock"
export TEXTDOMAINDIR="${PFU_LOCALEDIR}"

pfu_ui_locale() {
	local l="${PFU_UI_LOCALE:-}"

	if [[ -z $l ]]; then
		l="${LC_ALL:-}"
		[[ -z $l ]] && l="${LC_MESSAGES:-}"
		[[ -z $l ]] && l="${LANG:-}"
	fi

	# systemd writes /etc/locale.conf and most distributions use it; Debian and
	# Ubuntu keep the same LANG= line in /etc/default/locale instead.
	if [[ -z $l ]]; then
		local f
		for f in /etc/locale.conf /etc/default/locale; do
			[[ -r $f ]] || continue
			l="$(sed -n 's/^LANG=//p' "$f" | tr -d '"' | head -n1)"
			[[ -n $l ]] && break
		done
	fi

	printf '%s\n' "${l:-C}"
}

# Every gettext lookup is a fork and the settings screen redraws a screenful of
# labels per keypress, so results are memoized.
declare -A PFU_MSG_CACHE=()

PFU_MSG_RESULT=''

# pfu_msg_into <locale> <msgid>
# Plain lookup with the result in PFU_MSG_RESULT and no printf formatting, for
# callers that would otherwise pay a fork per label per frame.
pfu_msg_into() {
	local locale="$1" msgid="$2" cachekey
	cachekey="${locale}"$'\x1f'"${msgid}"

	if [[ -n ${PFU_MSG_CACHE[$cachekey]+set} ]]; then
		PFU_MSG_RESULT="${PFU_MSG_CACHE[$cachekey]}"
		return 0
	fi

	PFU_MSG_RESULT="$(LC_ALL="$locale" LANGUAGE="${locale%%.*}" gettext -- "$msgid" 2>/dev/null)"
	[[ -n $PFU_MSG_RESULT ]] || PFU_MSG_RESULT="$msgid"
	PFU_MSG_CACHE[$cachekey]="$PFU_MSG_RESULT"
	return 0
}

# pfu_msg_in <locale> <msgid> [printf args...]
pfu_msg_in() {
	local locale="$1" msgid="$2"
	shift 2

	pfu_msg_into "$locale" "$msgid"

	# With no arguments the message is plain text, not a format string. Feeding
	# it to printf anyway would turn a literal percent sign in a translation
	# into an invalid conversion.
	if (( $# == 0 )); then
		printf '%s' "$PFU_MSG_RESULT"
		return
	fi

	# shellcheck disable=SC2059  # the format string is the translated message
	printf -- "$PFU_MSG_RESULT" "$@"
}

PFU_LOCALE_CACHED=''

# pfu_msg <msgid> [printf args...]
pfu_msg() {
	[[ -n $PFU_LOCALE_CACHED ]] || PFU_LOCALE_CACHED="$(pfu_ui_locale)"
	pfu_msg_in "$PFU_LOCALE_CACHED" "$@"
}

# ---------------------------------------------------------------------------
# Output
# ---------------------------------------------------------------------------

# Decided once, while stdout is still whatever the process was started with:
# testing -t 1 at the point of use is wrong for anything called through $(...),
# which sees a pipe and would conclude nobody is watching.
PFU_INTERACTIVE=''
[[ -t 1 ]] && PFU_INTERACTIVE=1

if [[ -n $PFU_INTERACTIVE && -z ${NO_COLOR:-} ]]; then
	PFU_C_RESET=$'\033[0m'
	PFU_C_BOLD=$'\033[1m'
	PFU_C_DIM=$'\033[2m'
	PFU_C_BLUE=$'\033[38;2;52;153;255m'
	PFU_C_GREEN=$'\033[32m'
	PFU_C_YELLOW=$'\033[33m'
	PFU_C_RED=$'\033[31m'
else
	PFU_C_RESET='' PFU_C_BOLD='' PFU_C_DIM='' PFU_C_BLUE=''
	PFU_C_GREEN='' PFU_C_YELLOW='' PFU_C_RED=''
fi

# What pfu_ok, pfu_bad and pfu_note printed. The menu redraws straight after an
# action, which wipes the screen, so it shows these again under the new frame
# rather than holding everything up for a key press.
PFU_UI_NOTICES=()

pfu_say()  { printf '%s\n' "$*"; }
pfu_head() { printf '\n%s%s%s\n\n' "$PFU_C_BOLD$PFU_C_BLUE" "$*" "$PFU_C_RESET"; }
pfu_ok()   { PFU_UI_NOTICES+=("$PFU_C_GREEN✔$PFU_C_RESET $*"); printf '%s✔%s %s\n' "$PFU_C_GREEN" "$PFU_C_RESET" "$*"; }
pfu_bad()  { PFU_UI_NOTICES+=("$PFU_C_RED✘$PFU_C_RESET $*"); printf '%s✘%s %s\n' "$PFU_C_RED" "$PFU_C_RESET" "$*" >&2; }
pfu_note() { PFU_UI_NOTICES+=("$PFU_C_DIM•$PFU_C_RESET $*"); printf '%s•%s %s\n' "$PFU_C_DIM" "$PFU_C_RESET" "$*"; }

pfu_have() { command -v "$1" > /dev/null 2>&1; }

# Human-readable "x minutes ago" for a unix timestamp. 0 or empty yields the
# translated "never".
#
# Written with [[ ]] and a variable for each number on purpose: xgettext reads
# the < of an (( )) as a redirection and loses every string after it.
pfu_time_ago() {
	local ts="$1" now delta n

	if [[ ! $ts =~ ^[0-9]+$ || $ts -eq 0 ]]; then
		pfu_msg "never"
		printf '\n'
		return
	fi

	now="$(date +%s)"
	delta=$(( now - ts ))
	[[ $delta -lt 0 ]] && delta=0

	if [[ $delta -lt 60 ]]; then
		pfu_msg "just now"
	elif [[ $delta -lt 3600 ]]; then
		n=$(( delta / 60 ))
		pfu_msg "%d minutes ago" "$n"
	elif [[ $delta -lt 86400 ]]; then
		n=$(( delta / 3600 ))
		pfu_msg "%d hours ago" "$n"
	else
		n=$(( delta / 86400 ))
		pfu_msg "%d days ago" "$n"
	fi
	printf '\n'
}
