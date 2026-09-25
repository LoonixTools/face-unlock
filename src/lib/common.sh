# shellcheck shell=bash
#
# Paths, translations and output helpers.
#
# The shell side never touches the camera or the face data. Those belong to
# the daemon, and everything here that needs them asks it through
# face-unlock-ctl. What this side does write is the user's own settings
# file, and (through sudo, and only when asked) the system settings and the
# PAM files of sudo and polkit.

FU_VERSION="@VERSION@"
FU_NAME="face-unlock"
FU_PRETTY="Face Unlock"

FU_LIBDIR="${FU_LIBDIR:-@LIBDIR@}"
FU_LIBEXECDIR="${FU_LIBEXECDIR:-@LIBEXECDIR@}"
FU_LOCALEDIR="${FU_LOCALEDIR:-@LOCALEDIR@}"
FU_PAMDIR="${FU_PAMDIR:-@PAMDIR@}"

FU_CTL="${FU_CTL:-$FU_LIBEXECDIR/face-unlock-ctl}"
FU_AGENT="${FU_AGENT:-$FU_LIBEXECDIR/face-unlock-agent}"
FU_PAM_MODULE="${FU_PAM_MODULE:-$FU_PAMDIR/pam_face_unlock.so}"

FU_XDG_CONFIG="${XDG_CONFIG_HOME:-$HOME/.config}"
FU_CONFDIR="${FU_CONFDIR:-${FU_XDG_CONFIG}/${FU_NAME}}"
FU_CONFIG="${FU_CONFIG:-${FU_CONFDIR}/config}"

# The system settings. Only root writes them; see system.sh.
FU_SYSCONFIG="${FU_SYSCONFIG:-/etc/${FU_NAME}/config}"

FU_UNIT_SOCKET="face-unlockd.socket"
FU_UNIT_AGENT="face-unlock-agent.service"

# The desktop this runs in: plasma, gnome, hyprland, niri or other.
case ":${XDG_CURRENT_DESKTOP:-}:" in
	*:KDE:*)      FU_DESKTOP=plasma ;;
	*:GNOME:*)    FU_DESKTOP=gnome ;;
	*:Hyprland:*) FU_DESKTOP=hyprland ;;
	*:niri:*)     FU_DESKTOP=niri ;;
	*)            FU_DESKTOP=other ;;
esac

# ---------------------------------------------------------------------------
# Translations
# ---------------------------------------------------------------------------

export TEXTDOMAIN="face-unlock"
export TEXTDOMAINDIR="${FU_LOCALEDIR}"

fu_ui_locale() {
	local l="${FU_UI_LOCALE:-}"

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
declare -A FU_MSG_CACHE=()

FU_MSG_RESULT=''

# fu_msg_into <locale> <msgid>
# Plain lookup with the result in FU_MSG_RESULT and no printf formatting, for
# callers that would otherwise pay a fork per label per frame.
fu_msg_into() {
	local locale="$1" msgid="$2" cachekey
	cachekey="${locale}"$'\x1f'"${msgid}"

	if [[ -n ${FU_MSG_CACHE[$cachekey]+set} ]]; then
		FU_MSG_RESULT="${FU_MSG_CACHE[$cachekey]}"
		return 0
	fi

	FU_MSG_RESULT="$(LC_ALL="$locale" LANGUAGE="${locale%%.*}" gettext -- "$msgid" 2>/dev/null)"
	[[ -n $FU_MSG_RESULT ]] || FU_MSG_RESULT="$msgid"
	FU_MSG_CACHE[$cachekey]="$FU_MSG_RESULT"
	return 0
}

# fu_msg_in <locale> <msgid> [printf args...]
fu_msg_in() {
	local locale="$1" msgid="$2"
	shift 2

	fu_msg_into "$locale" "$msgid"

	# With no arguments the message is plain text, not a format string. Feeding
	# it to printf anyway would turn a literal percent sign in a translation
	# into an invalid conversion.
	if (( $# == 0 )); then
		printf '%s' "$FU_MSG_RESULT"
		return
	fi

	# shellcheck disable=SC2059  # the format string is the translated message
	printf -- "$FU_MSG_RESULT" "$@"
}

FU_LOCALE_CACHED=''

# fu_msg <msgid> [printf args...]
fu_msg() {
	[[ -n $FU_LOCALE_CACHED ]] || FU_LOCALE_CACHED="$(fu_ui_locale)"
	fu_msg_in "$FU_LOCALE_CACHED" "$@"
}

# ---------------------------------------------------------------------------
# Output
# ---------------------------------------------------------------------------

# Decided once, while stdout is still whatever the process was started with:
# testing -t 1 at the point of use is wrong for anything called through $(...),
# which sees a pipe and would conclude nobody is watching.
FU_INTERACTIVE=''
[[ -t 1 ]] && FU_INTERACTIVE=1

if [[ -n $FU_INTERACTIVE && -z ${NO_COLOR:-} ]]; then
	FU_C_RESET=$'\033[0m'
	FU_C_BOLD=$'\033[1m'
	FU_C_DIM=$'\033[2m'
	FU_C_BLUE=$'\033[38;2;52;153;255m'
	FU_C_GREEN=$'\033[32m'
	FU_C_YELLOW=$'\033[33m'
	FU_C_RED=$'\033[31m'
else
	FU_C_RESET='' FU_C_BOLD='' FU_C_DIM='' FU_C_BLUE=''
	FU_C_GREEN='' FU_C_YELLOW='' FU_C_RED=''
fi

# What fu_ok, fu_bad and fu_note printed. The menu redraws straight after an
# action, which wipes the screen, so it shows these again under the new frame
# rather than holding everything up for a key press.
FU_UI_NOTICES=()

fu_say()  { printf '%s\n' "$*"; }
fu_head() { printf '\n%s%s%s\n\n' "$FU_C_BOLD$FU_C_BLUE" "$*" "$FU_C_RESET"; }
fu_ok()   { FU_UI_NOTICES+=("$FU_C_GREEN✔$FU_C_RESET $*"); printf '%s✔%s %s\n' "$FU_C_GREEN" "$FU_C_RESET" "$*"; }
fu_bad()  { FU_UI_NOTICES+=("$FU_C_RED✘$FU_C_RESET $*"); printf '%s✘%s %s\n' "$FU_C_RED" "$FU_C_RESET" "$*" >&2; }
fu_note() { FU_UI_NOTICES+=("$FU_C_DIM•$FU_C_RESET $*"); printf '%s•%s %s\n' "$FU_C_DIM" "$FU_C_RESET" "$*"; }

fu_have() { command -v "$1" > /dev/null 2>&1; }

# Human-readable "x minutes ago" for a unix timestamp. 0 or empty yields the
# translated "never".
#
# Written with [[ ]] and a variable for each number on purpose: xgettext reads
# the < of an (( )) as a redirection and loses every string after it.
fu_time_ago() {
	local ts="$1" now delta n

	if [[ ! $ts =~ ^[0-9]+$ || $ts -eq 0 ]]; then
		fu_msg "never"
		printf '\n'
		return
	fi

	now="$(date +%s)"
	delta=$(( now - ts ))
	[[ $delta -lt 0 ]] && delta=0

	if [[ $delta -lt 60 ]]; then
		fu_msg "just now"
	elif [[ $delta -lt 120 ]]; then
		fu_msg "1 minute ago"
	elif [[ $delta -lt 3600 ]]; then
		n=$(( delta / 60 ))
		fu_msg "%d minutes ago" "$n"
	elif [[ $delta -lt 7200 ]]; then
		fu_msg "1 hour ago"
	elif [[ $delta -lt 86400 ]]; then
		n=$(( delta / 3600 ))
		fu_msg "%d hours ago" "$n"
	elif [[ $delta -lt 172800 ]]; then
		fu_msg "1 day ago"
	else
		n=$(( delta / 86400 ))
		fu_msg "%d days ago" "$n"
	fi
	printf '\n'
}
