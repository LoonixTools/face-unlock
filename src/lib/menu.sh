# shellcheck shell=bash
#
# The interactive front end.
#
# This is the whole configuration interface. There are two files behind it
# and the face data behind the daemon, but no part of the program ever asks
# anybody to open one: one switch on the front screen, the faces, a settings
# list, and a way to try it.

# Terminal mode.
#
# bash flips the terminal into non-canonical mode for each `read -sn1` and back
# out again in between. That gap matters: in canonical mode DEL is the ERASE
# character, so the line discipline eats it instead of delivering it, and a
# backspace typed while the interface was between reads simply vanishes.
# Holding non-canonical mode for the whole interface removes the gap.
PFU_TERM_SAVED=''

pfu_ui_term_raw() {
	pfu_have stty || return 0
	[[ -t 0 ]] || return 0
	[[ -n $PFU_TERM_SAVED ]] && return 0

	PFU_TERM_SAVED="$(stty -g 2>/dev/null)" || { PFU_TERM_SAVED=''; return 0; }
	stty -icanon -echo min 1 time 0 2>/dev/null || true
}

pfu_ui_term_restore() {
	[[ -n $PFU_TERM_SAVED ]] || return 0
	stty "$PFU_TERM_SAVED" 2>/dev/null || true
	PFU_TERM_SAVED=''
}

# Runs an action with the terminal handed back to normal line mode, so anything
# it prints or prompts for (sudo's password prompt, above all) behaves the way
# a program expects.
pfu_ui_cooked() {
	pfu_ui_term_restore
	"$@"
	local rc=$?
	pfu_ui_term_raw
	return $rc
}

# pfu_read_key
# One keypress, resolved to a symbolic name. Arrow keys arrive as ESC [ A, so
# the tail of the sequence is consumed here rather than being mistaken for
# three separate presses.
pfu_read_key() {
	local k rest

	IFS= read -rsn1 k || return 1

	case "$k" in
		$'\e')
			if IFS= read -rsn2 -t 0.05 rest; then
				case "$rest" in
					'[A') printf 'up\n' ;;
					'[B') printf 'down\n' ;;
					'[C') printf 'right\n' ;;
					'[D') printf 'left\n' ;;
					*)    printf 'escape\n' ;;
				esac
			else
				printf 'escape\n'
			fi
			;;
		''|$'\r')      printf 'enter\n' ;;
		$'\x7f'|$'\b') printf 'backspace\n' ;;
		' ')           printf 'space\n' ;;
		*)             printf '%s\n' "$k" ;;
	esac
}

# pfu_ui_read_line <initial>
# A minimal line editor built on pfu_read_key, with the result in
# PFU_LINE_RESULT. This exists instead of bash's own `read -r` because mixing
# line mode into a single-key interface breaks it: after one cooked-mode read
# the following `read -sn1` stops receiving keystrokes entirely.
PFU_LINE_RESULT=''

pfu_ui_read_line() {
	local buf="${1:-}" key

	PFU_LINE_RESULT=''
	printf '%s' "$buf"

	while true; do
		key="$(pfu_read_key)" || { printf '\n'; return 1; }

		case "$key" in
			enter)
				printf '\n'
				PFU_LINE_RESULT="$buf"
				return 0
				;;
			escape)
				printf '\n'
				return 1
				;;
			backspace)
				if [[ -n $buf ]]; then
					buf="${buf%?}"
					printf '\b \b'
				fi
				;;
			space)
				buf+=' '
				printf ' '
				;;
			up|down|left|right) ;;
			*)
				[[ ${#key} -eq 1 ]] || continue
				buf+="$key"
				printf '%s' "$key"
				;;
		esac
	done
}

pfu_pause() {
	printf '\n  %s' "$(pfu_msg "Press any key to continue...")"
	read -rsn1 _ || true
	printf '\n'
}

# pfu_ui_confirm <question>
pfu_ui_confirm() {
	local key
	printf '\n  %s %s ' "$1" "$(pfu_msg "[y/N]")"
	key="$(pfu_read_key)" || return 1
	printf '%s\n' "$key"
	[[ $key == [yYjJ] ]]
}

# _pfu_row <label> <value>
# printf's %-28s pads by bytes, so a label containing "ü" comes out one column
# short. ${#s} counts characters in a UTF-8 locale, so the padding is computed
# here instead.
_pfu_row() {
	local label="$1" value="$2" pad
	pad=$(( 30 - ${#label} ))
	(( pad < 0 )) && pad=0
	printf '  %s%*s %s\n' "$label" "$pad" '' "$value"
}

_pfu_onoff() {
	if [[ $1 == yes ]]; then
		printf '%s%s%s' "$PFU_C_GREEN" "$(pfu_msg "ON")" "$PFU_C_RESET"
	else
		printf '%s%s%s' "$PFU_C_DIM" "$(pfu_msg "OFF")" "$PFU_C_RESET"
	fi
}

_pfu_small_onoff() {
	if [[ $1 == yes ]]; then
		printf '%s%s%s' "$PFU_C_GREEN" "$(pfu_msg "on")" "$PFU_C_RESET"
	else
		printf '%s%s%s' "$PFU_C_DIM" "$(pfu_msg "off")" "$PFU_C_RESET"
	fi
}

# pfu_value_label <Key> <value>
# How a setting's value reads in the menu.
pfu_value_label() {
	case "$1:$2" in
		Liveness:heavy)     pfu_msg "strict (blink or turn your head)" ;;
		Liveness:light)     pfu_msg "basic (screens and phone edges)" ;;
		Liveness:off)       pfu_msg "off" ;;
		Strictness:normal)  pfu_msg "normal" ;;
		Strictness:strict)  pfu_msg "strict" ;;
		Strictness:relaxed) pfu_msg "relaxed" ;;
		BubbleStyle:full)   pfu_msg "island with the face" ;;
		BubbleStyle:minimal) pfu_msg "small pill with a lock" ;;
		ScanSeconds:*)      pfu_msg "%s seconds" "$2" ;;
		Camera:auto)        pfu_msg "automatic" ;;
		*)                  printf '%s' "$2" ;;
	esac
}

# ---------------------------------------------------------------------------
# Status
# ---------------------------------------------------------------------------

# pfu_ui_status
# Shared by the `status` subcommand and the menu header.
pfu_ui_status() {
	local names='' i camera

	pfu_config_load
	pfu_status_load

	_pfu_row "$(pfu_msg "Face unlock")" "$(_pfu_onoff "$CFG_ENABLED")"
	printf '\n'

	if [[ $PFU_ST_REACHABLE != yes ]]; then
		_pfu_row "$(pfu_msg "Service")" "${PFU_C_YELLOW}$(pfu_msg "not running")${PFU_C_RESET}"
		if [[ $CFG_ENABLED == yes ]]; then
			printf '\n  %s%s%s\n' "$PFU_C_DIM" "$(pfu_msg "Turning face unlock on again starts it.")" "$PFU_C_RESET"
		fi
		return 0
	fi

	pfu_faces_load
	for i in "${!PFU_FACE_NAMES[@]}"; do
		[[ ${PFU_FACE_ON[i]} == true ]] || continue
		names="${names:+$names, }${PFU_FACE_NAMES[i]}"
	done
	if (( ${#PFU_FACE_IDS[@]} == 0 )); then
		_pfu_row "$(pfu_msg "Faces")" "${PFU_C_YELLOW}$(pfu_msg "none set up yet")${PFU_C_RESET}"
	else
		_pfu_row "$(pfu_msg "Faces")" "${PFU_ST_FACES} ${PFU_C_DIM}(${names:-$(pfu_msg "all turned off")})${PFU_C_RESET}"
	fi

	if [[ $PFU_ST_CAMERA_PRESENT == true ]]; then
		camera="${PFU_ST_CAMERA_NAME:-$PFU_ST_CAMERA}"
	else
		camera="${PFU_C_YELLOW}$(pfu_msg "none found")${PFU_C_RESET}"
	fi
	_pfu_row "$(pfu_msg "Camera")" "$camera"

	_pfu_row "$(pfu_msg "Lock screen")" "$(_pfu_small_onoff "$( [[ $CFG_ENABLED == yes && $CFG_LOCK == yes ]] && echo yes || echo no)")"
	_pfu_row "$(pfu_msg "sudo")" "$(_pfu_small_onoff "$(pfu_pam_enabled sudo && echo yes || echo no)")"
	_pfu_row "$(pfu_msg "Admin prompts")" "$(_pfu_small_onoff "$(pfu_pam_enabled polkit-1 && echo yes || echo no)")"
	_pfu_row "$(pfu_msg "Photo check")" "$(pfu_value_label Liveness "$CFG_LIVENESS")"

	if (( PFU_ST_LOCKOUT > 0 )); then
		_pfu_row "$(pfu_msg "Paused")" "${PFU_C_YELLOW}$(pfu_msg "for %d more minutes, or until the password is used" "$(( (PFU_ST_LOCKOUT + 59) / 60 ))")${PFU_C_RESET}"
	fi
	_pfu_row "$(pfu_msg "Last unlock")" "$(pfu_time_ago "$PFU_ST_LAST")"

	if [[ $PFU_ST_MODELS != true ]]; then
		printf '\n  %s%s%s\n' "$PFU_C_RED" "$(pfu_msg "The recognition models are missing. Reinstall the package.")" "$PFU_C_RESET"
	fi
}

# ---------------------------------------------------------------------------
# Settings
# ---------------------------------------------------------------------------
# Format: scope|Key|type|default|label-msgid|choices
#   scope  user (this user's file), sys (the system file, through sudo) or
#          pam (the service of that name, through sudo)
#   type   bool, choice (cycles through the choices) or camera
PFU_SETTINGS=(
	"user|LockScreen|bool|yes|Unlock the lock screen"
	"user|ScanOnWake|bool|yes|Look when somebody comes back to the screen"
	"user|ScanOnLock|bool|no|Look right after the screen locks"
	"pam|sudo|bool|no|Use for sudo in a terminal"
	"pam|polkit-1|bool|no|Use for admin prompts"
	"sys|Liveness|choice|heavy|Photo check|heavy,light,off"
	"sys|Strictness|choice|normal|How closely a face has to match|normal,strict,relaxed"
	"sys|Attention|bool|yes|Only while looking at the screen"
	"sys|Camera|camera|auto|Camera"
	"sys|ScanSeconds|choice|5|How long one look lasts|3,4,5,6,8,10"
	"sys|Adapt|bool|yes|Learn from every unlock"
	"sys|SkipLidClosed|bool|yes|Not while the lid is closed"
	"user|Bubble|bool|yes|Show the bubble at the top"
	"user|BubbleStyle|choice|full|Bubble style|full,minimal"
	"user|BubbleForPrompts|bool|yes|Bubble for sudo and admin prompts too"
)

# _pfu_setting_value <scope> <Key> <default>
# The current value, in PFU_SETTING_VALUE.
PFU_SETTING_VALUE=''

_pfu_setting_value() {
	case "$1" in
		user) _pfu_kv_lookup "$PFU_CONFIG" "$2" "$3"; PFU_SETTING_VALUE="$PFU_KV_VALUE" ;;
		sys)  _pfu_kv_lookup "$PFU_SYSCONFIG" "$2" "$3"; PFU_SETTING_VALUE="$PFU_KV_VALUE" ;;
		pam)  if pfu_pam_enabled "$2"; then PFU_SETTING_VALUE=yes; else PFU_SETTING_VALUE=no; fi ;;
	esac
}

# _pfu_next_choice <current> <a,b,c>
_pfu_next_choice() {
	local current="$1" list="$2" first='' found=0 c
	local -a choices
	IFS=',' read -r -a choices <<< "$list"
	for c in "${choices[@]}"; do
		[[ -z $first ]] && first="$c"
		if (( found )); then
			printf '%s\n' "$c"
			return
		fi
		[[ $c == "$current" ]] && found=1
	done
	printf '%s\n' "$first"
}

# _pfu_next_camera <current>
_pfu_next_camera() {
	local current="$1" i
	pfu_cameras_load
	local -a all=(auto "${PFU_CAM_PATHS[@]}")
	for i in "${!all[@]}"; do
		if [[ ${all[i]} == "$current" ]]; then
			printf '%s\n' "${all[(i + 1) % ${#all[@]}]}"
			return
		fi
	done
	printf 'auto\n'
}

_pfu_camera_label() {
	local value="$1" i
	if [[ $value == auto ]]; then
		for i in "${!PFU_CAM_PATHS[@]}"; do
			if [[ ${PFU_CAM_PATHS[i]} == "$PFU_CAM_AUTO" ]]; then
				pfu_msg "automatic (%s)" "${PFU_CAM_NAMES[i]}"
				return
			fi
		done
		pfu_msg "automatic"
		return
	fi
	for i in "${!PFU_CAM_PATHS[@]}"; do
		if [[ ${PFU_CAM_PATHS[i]} == "$value" ]]; then
			printf '%s' "${PFU_CAM_NAMES[i]}"
			[[ ${PFU_CAM_IR[i]} == true ]] && printf ' %s' "$(pfu_msg "(infrared)")"
			return
		fi
	done
	printf '%s %s' "$value" "$(pfu_msg "(not connected)")"
}

# _pfu_setting_change <scope> <Key> <type> <current> <choices>
_pfu_setting_change() {
	local scope="$1" key="$2" type="$3" current="$4" choices="$5" next

	case "$type" in
		bool)   if pfu_is_true "$current"; then next=no; else next=yes; fi ;;
		choice) next="$(_pfu_next_choice "$current" "$choices")" ;;
		camera) next="$(_pfu_next_camera "$current")" ;;
	esac

	case "$scope" in
		user)
			pfu_config_set "$key" "$next" || { pfu_bad "$(pfu_msg "Could not save the setting.")"; pfu_pause; }
			;;
		sys)
			printf '\n'
			pfu_ui_cooked pfu_root set "$key" "$next" || pfu_pause
			PFU_KV_CACHE=()
			;;
		pam)
			printf '\n'
			if [[ $next == yes ]]; then
				pfu_ui_cooked pfu_root pam-enable "$key" || pfu_pause
			else
				pfu_ui_cooked pfu_root pam-disable "$key" || pfu_pause
			fi
			# Remembered, so that turning face unlock off and on again brings
			# it back.
			case "$key" in
				sudo)     pfu_config_set Sudo "$next" ;;
				polkit-1) pfu_config_set Polkit "$next" ;;
			esac
			;;
	esac
}

# pfu_ui_settings
# A cursor list rather than a numbered menu. The frame is assembled in memory
# and written once, and everything constant is resolved before the loop.
pfu_ui_settings() {
	local count=${#PFU_SETTINGS[@]}
	local -a scopes=() keys=() types=() defaults=() labels=() choices=() values=()
	local spec scope key type default label choice locale i frame row pad dirty=1 cursor=0 shown

	locale="$(pfu_ui_locale)"
	for spec in "${PFU_SETTINGS[@]}"; do
		IFS='|' read -r scope key type default label choice <<< "$spec"
		scopes+=("$scope"); keys+=("$key"); types+=("$type"); defaults+=("$default"); choices+=("$choice")
		pfu_msg_into "$locale" "$label"
		labels+=("$PFU_MSG_RESULT")
	done

	local title hint legend l_on l_off
	pfu_msg_into "$locale" "Settings"; title="$PFU_MSG_RESULT"
	pfu_msg_into "$locale" "Up/Down: select, Space or Right: change, q: back"; hint="$PFU_MSG_RESULT"
	pfu_msg_into "$locale" "Settings marked * are for the whole computer and ask for your password."; legend="$PFU_MSG_RESULT"
	pfu_msg_into "$locale" "ON"; l_on="$PFU_MSG_RESULT"
	pfu_msg_into "$locale" "OFF"; l_off="$PFU_MSG_RESULT"

	local clearseq
	clearseq="$(clear 2>/dev/null)" || clearseq=$'\033[H\033[2J'

	pfu_cameras_load

	while true; do
		if (( dirty )); then
			PFU_KV_CACHE=()
			for i in "${!keys[@]}"; do
				_pfu_setting_value "${scopes[i]}" "${keys[i]}" "${defaults[i]}"
				values[i]="$PFU_SETTING_VALUE"
			done
			dirty=0
		fi

		frame="$clearseq"$'\n'"${PFU_C_BOLD}${PFU_C_BLUE}  ${title}${PFU_C_RESET}"$'\n\n'

		local marker selected="${PFU_C_BLUE}▸${PFU_C_RESET} " star
		for i in "${!keys[@]}"; do
			case "${types[i]}" in
				bool)
					if pfu_is_true "${values[i]}"; then
						shown="${PFU_C_GREEN}${l_on}${PFU_C_RESET}"
					else
						shown="${PFU_C_DIM}${l_off}${PFU_C_RESET}"
					fi
					;;
				camera) shown="$(_pfu_camera_label "${values[i]}")" ;;
				*)      shown="$(pfu_value_label "${keys[i]}" "${values[i]}")" ;;
			esac
			star=' '
			[[ ${scopes[i]} != user ]] && star='*'
			pad=$(( 44 - ${#labels[i]} ))
			(( pad < 0 )) && pad=0
			if (( i == cursor )); then marker="$selected"; else marker='  '; fi
			printf -v row '  %s%s%s%*s %s' "$marker" "${labels[i]}" "${PFU_C_DIM}${star}${PFU_C_RESET}" "$pad" '' "$shown"
			frame+="$row"$'\n'
			# A gap between the groups: the lock screen, other prompts, how it
			# checks, the bubble.
			case "${keys[i]}" in
				ScanOnLock|polkit-1|SkipLidClosed) frame+=$'\n' ;;
			esac
		done

		frame+=$'\n'"  ${PFU_C_DIM}${legend}${PFU_C_RESET}"$'\n'
		frame+="  ${PFU_C_DIM}${hint}${PFU_C_RESET}"$'\n'
		printf '%s' "$frame"

		key="$(pfu_read_key)" || return 0

		case "$key" in
			up|k)   cursor=$(( (cursor - 1 + count) % count )) ;;
			down|j) cursor=$(( (cursor + 1) % count )) ;;
			space|enter|right|l)
				_pfu_setting_change "${scopes[cursor]}" "${keys[cursor]}" "${types[cursor]}" "${values[cursor]}" "${choices[cursor]}"
				dirty=1
				;;
			q|Q|escape) return 0 ;;
			*) ;;
		esac
	done
}

# ---------------------------------------------------------------------------
# Faces
# ---------------------------------------------------------------------------

pfu_ui_faces() {
	local key frame row pad i cursor=0 count locale shown

	locale="$(pfu_ui_locale)"
	local title hint empty l_on l_off
	pfu_msg_into "$locale" "Faces"; title="$PFU_MSG_RESULT"
	pfu_msg_into "$locale" "Up/Down: select, Space: on/off, r: rename, d: delete, a: add, q: back"; hint="$PFU_MSG_RESULT"
	pfu_msg_into "$locale" "No face is set up yet. Press a to add one."; empty="$PFU_MSG_RESULT"
	pfu_msg_into "$locale" "on"; l_on="$PFU_MSG_RESULT"
	pfu_msg_into "$locale" "off"; l_off="$PFU_MSG_RESULT"

	local clearseq
	clearseq="$(clear 2>/dev/null)" || clearseq=$'\033[H\033[2J'

	while true; do
		pfu_faces_load
		count=${#PFU_FACE_IDS[@]}
		(( cursor >= count )) && cursor=$(( count > 0 ? count - 1 : 0 ))

		frame="$clearseq"$'\n'"${PFU_C_BOLD}${PFU_C_BLUE}  ${title}${PFU_C_RESET}"$'\n\n'
		if (( count == 0 )); then
			frame+="  ${PFU_C_DIM}${empty}${PFU_C_RESET}"$'\n'
		fi
		local marker selected="${PFU_C_BLUE}▸${PFU_C_RESET} " samples
		for i in "${!PFU_FACE_IDS[@]}"; do
			if [[ ${PFU_FACE_ON[i]} == true ]]; then shown="${PFU_C_GREEN}${l_on}${PFU_C_RESET}"; else shown="${PFU_C_DIM}${l_off}${PFU_C_RESET}"; fi
			samples="$(pfu_msg "%s samples, %s learned" "${PFU_FACE_SAMPLES[i]}" "${PFU_FACE_LEARNED[i]}")"
			pad=$(( 30 - ${#PFU_FACE_NAMES[i]} ))
			(( pad < 0 )) && pad=0
			if (( i == cursor )); then marker="$selected"; else marker='  '; fi
			printf -v row '  %s%s%*s %s  %s%s%s' "$marker" "${PFU_FACE_NAMES[i]}" "$pad" '' "$shown" "$PFU_C_DIM" "$samples" "$PFU_C_RESET"
			frame+="$row"$'\n'
		done
		frame+=$'\n'"  ${PFU_C_DIM}${hint}${PFU_C_RESET}"$'\n'
		printf '%s' "$frame"

		key="$(pfu_read_key)" || return 0
		case "$key" in
			up|k)   (( count )) && cursor=$(( (cursor - 1 + count) % count )) ;;
			down|j) (( count )) && cursor=$(( (cursor + 1) % count )) ;;
			space|enter)
				(( count )) || continue
				if [[ ${PFU_FACE_ON[cursor]} == true ]]; then
					pfu_ctl disable "${PFU_FACE_IDS[cursor]}" > /dev/null
				else
					pfu_ctl enable "${PFU_FACE_IDS[cursor]}" > /dev/null
				fi
				;;
			r|R)
				(( count )) || continue
				printf '\n  %s ' "$(pfu_msg "New name:")"
				if pfu_ui_read_line "${PFU_FACE_NAMES[cursor]}" && [[ -n $PFU_LINE_RESULT ]]; then
					pfu_ctl rename "${PFU_FACE_IDS[cursor]}" "$PFU_LINE_RESULT" > /dev/null
				fi
				;;
			d|D)
				(( count )) || continue
				if pfu_ui_confirm "$(pfu_msg "Delete \"%s\"?" "${PFU_FACE_NAMES[cursor]}")"; then
					pfu_ctl remove "${PFU_FACE_IDS[cursor]}" > /dev/null
				fi
				;;
			a|A)
				pfu_ui_cooked pfu_do_setup
				[[ -n $PFU_UI_NEEDS_ACK ]] && pfu_pause
				PFU_UI_NEEDS_ACK=''
				;;
			q|Q|escape) return 0 ;;
			*) ;;
		esac
	done
}

# ---------------------------------------------------------------------------
# The menu
# ---------------------------------------------------------------------------

pfu_ui_menu() {
	local choice

	pfu_ui_term_raw
	trap 'pfu_ui_term_restore' EXIT INT TERM

	while true; do
		clear 2>/dev/null || true
		pfu_head "  $PFU_PRETTY"
		pfu_ui_status
		printf '\n'
		printf '  [1] %s\n' "$(pfu_msg "Turn face unlock on or off")"
		printf '  [2] %s\n' "$(pfu_msg "Add a face")"
		printf '  [3] %s\n' "$(pfu_msg "Faces")"
		printf '  [4] %s\n' "$(pfu_msg "Settings")"
		printf '  [5] %s\n' "$(pfu_msg "Try it")"
		printf '  [q] %s\n' "$(pfu_msg "Quit")"
		printf '\n  > '

		choice="$(pfu_read_key)" || {
			printf '\n'; pfu_ui_term_restore; trap - EXIT INT TERM; return 0
		}
		case "$choice" in
			enter|space|up|down|left|right|escape) choice='' ;;
		esac
		printf '%s\n' "$choice"

		PFU_UI_NEEDS_ACK=''
		case "$choice" in
			1)
				pfu_config_load
				if [[ $CFG_ENABLED == yes ]]; then
					pfu_ui_cooked pfu_do_disable
				else
					pfu_ui_cooked pfu_do_enable
				fi
				[[ -n $PFU_UI_NEEDS_ACK ]] && pfu_pause
				;;
			2)
				pfu_ui_cooked pfu_do_setup
				[[ -n $PFU_UI_NEEDS_ACK ]] && pfu_pause
				;;
			3) pfu_ui_faces ;;
			4) pfu_ui_settings ;;
			5)
				printf '\n'
				pfu_ui_cooked pfu_test
				pfu_pause
				;;
			q|Q) pfu_ui_term_restore; trap - EXIT INT TERM; return 0 ;;
			# Anything else (Enter, arrow keys, stray characters) just
			# redraws. Escape is deliberately not a quit key, so a mistyped
			# arrow key cannot close the menu.
			*) ;;
		esac
	done
}
