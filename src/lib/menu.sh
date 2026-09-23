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

# _pfu_ui_take_notices
# The messages the last action left, as lines for under a frame, in
# PFU_UI_NOTICE_TEXT. Each is shown once.
PFU_UI_NOTICE_TEXT=''

_pfu_ui_take_notices() {
	local n
	PFU_UI_NOTICE_TEXT=''
	(( ${#PFU_UI_NOTICES[@]} )) || return 0
	PFU_UI_NOTICE_TEXT=$'\n'
	for n in "${PFU_UI_NOTICES[@]}"; do
		PFU_UI_NOTICE_TEXT+="  $n"$'\n'
	done
	PFU_UI_NOTICES=()
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
		Liveness:heavy)     pfu_msg "strict" ;;
		Liveness:light)     pfu_msg "basic" ;;
		Liveness:off)       pfu_msg "off" ;;
		Strictness:normal)  pfu_msg "normal" ;;
		Strictness:strict)  pfu_msg "strict" ;;
		Strictness:relaxed) pfu_msg "relaxed" ;;
		BubbleStyle:full)   pfu_msg "island with the face" ;;
		BubbleStyle:minimal) pfu_msg "small pill with a lock" ;;
		AnimationSpeed:normal) pfu_msg "normal" ;;
		AnimationSpeed:fast) pfu_msg "fast" ;;
		AnimationSpeed:slow) pfu_msg "slow" ;;
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
# Format: scope|Key|type|default|label-msgid|choices|needs
#   scope  user (this user's file), sys (the system file, through sudo),
#          pam (the service of that name, through sudo), or group for a
#          heading, with only the label after it
#   type   bool, choice (steps through the choices) or camera
#   needs  a bool setting this one does nothing without; it is dimmed while
#          that is off
PFU_SETTINGS=(
	"group|Lock screen"
	"user|LockScreen|bool|yes|Unlock with your face"
	"user|ScanOnWake|bool|yes|Scan when you come back||LockScreen"
	"user|ScanOnLock|bool|no|Scan right after locking||LockScreen"
	"group|Password prompts"
	"pam|sudo|bool|no|sudo in a terminal"
	"pam|polkit-1|bool|no|Admin prompts"
	"group|Recognition"
	"sys|Liveness|choice|light|Photo check|off,light,heavy"
	"sys|Strictness|choice|normal|How closely the face has to match|relaxed,normal,strict"
	"sys|Attention|bool|yes|Only when you look at the screen"
	"sys|Camera|camera|auto|Camera"
	"sys|ScanSeconds|choice|5|How long a scan lasts|3,4,5,6,8,10"
	"sys|Adapt|bool|yes|Learn from every unlock"
	"sys|SkipLidClosed|bool|yes|Not when the lid is closed"
	"group|Bubble"
	"user|Bubble|bool|yes|Show the bubble"
	"user|BubbleStyle|choice|full|Style|full,minimal|Bubble"
	"user|AnimationSpeed|choice|normal|Animation speed|slow,normal,fast|Bubble"
	"user|BubbleForPrompts|bool|yes|Also for sudo and admin prompts||Bubble"
)

# pfu_setting_help <Key> <value>
# What a setting does, shown under the list for the selected one.
pfu_setting_help() {
	case "$1:$2" in
		LockScreen:*)       pfu_msg "Unlocks the lock screen when it sees your face. Off: only your password works there." ;;
		ScanOnWake:*)       pfu_msg "Scans when you press a key or move the mouse on the lock screen, and when the computer wakes up." ;;
		ScanOnLock:*)       pfu_msg "Scans as soon as the screen locks. Off by default: if you lock it yourself, it would unlock again right away." ;;
		sudo:*)             pfu_msg "sudo takes your face instead of the password. No match: you type the password as usual." ;;
		polkit-1:*)         pfu_msg "The password windows of Plasma and apps, for example when you install software. No match: you type the password." ;;
		Liveness:heavy)     pfu_msg "You have to blink or turn your head a little. This also stops printed photos." ;;
		Liveness:off)       pfu_msg "No check at all. Only for trying out a camera." ;;
		Liveness:*)         pfu_msg "Stops photos on a phone, a tablet or glossy paper. You do not have to blink. A matte printed photo can get through." ;;
		Strictness:strict)  pfu_msg "Fewer wrong matches, but it may not know you with glasses or in bad light." ;;
		Strictness:relaxed) pfu_msg "Knows you more easily, but also somebody who looks a lot like you." ;;
		Strictness:*)       pfu_msg "The default. Right for most people." ;;
		Attention:*)        pfu_msg "Your eyes have to be open and on the screen. So it does not unlock while you look away or sleep." ;;
		Camera:*)           pfu_msg "Automatic takes the first normal camera. After a change, set up your face again. An infrared camera needs its light on (linux-enable-ir-emitter)." ;;
		ScanSeconds:*)      pfu_msg "How long the camera looks for your face before it gives up." ;;
		Adapt:*)            pfu_msg "After a sure match it keeps how you look now. So a new haircut or glasses need no new setup." ;;
		SkipLidClosed:*)    pfu_msg "No scan while the laptop is closed, for example at a desk with an external screen." ;;
		Bubble:*)           pfu_msg "The bubble at the top of the screen shows what the camera is doing." ;;
		BubbleStyle:minimal) pfu_msg "A small pill with a lock that opens." ;;
		BubbleStyle:*)      pfu_msg "An island with a face that looks around, then rings and a tick." ;;
		AnimationSpeed:*)   pfu_msg "How fast the bubble moves." ;;
		BubbleForPrompts:*) pfu_msg "Shows the bubble when sudo or an admin prompt scans your face, too." ;;
	esac
}

# _pfu_wrap <width> <text>
# Word wrap, into PFU_WRAP_LINES.
PFU_WRAP_LINES=()

_pfu_wrap() {
	local width="$1" word line=''
	local -a words
	PFU_WRAP_LINES=()
	read -r -a words <<< "$2"
	for word in "${words[@]}"; do
		if [[ -n $line ]] && (( ${#line} + 1 + ${#word} > width )); then
			PFU_WRAP_LINES+=("$line")
			line="$word"
		else
			line="${line:+$line }$word"
		fi
	done
	[[ -n $line ]] && PFU_WRAP_LINES+=("$line")
	return 0
}

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

# _pfu_step <current> <step> <choice>...
# The choice <step> (1 or -1) away from the current one, round at the ends.
_pfu_step() {
	local current="$1" step="$2" i
	shift 2
	local -a all=("$@")
	for i in "${!all[@]}"; do
		if [[ ${all[i]} == "$current" ]]; then
			printf '%s\n' "${all[(i + step + ${#all[@]}) % ${#all[@]}]}"
			return
		fi
	done
	printf '%s\n' "${all[0]}"
}

# _pfu_next_choice <current> <a,b,c> <step>
_pfu_next_choice() {
	local -a choices
	IFS=',' read -r -a choices <<< "$2"
	_pfu_step "$1" "$3" "${choices[@]}"
}

# _pfu_next_camera <current> <step>
_pfu_next_camera() {
	pfu_cameras_load
	_pfu_step "$1" "$2" auto "${PFU_CAM_PATHS[@]}"
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

# _pfu_setting_change <scope> <Key> <type> <current> <choices> <step>
_pfu_setting_change() {
	local scope="$1" key="$2" type="$3" current="$4" choices="$5" step="$6" next

	case "$type" in
		bool)   if pfu_is_true "$current"; then next=no; else next=yes; fi ;;
		choice) next="$(_pfu_next_choice "$current" "$choices" "$step")" ;;
		camera) next="$(_pfu_next_camera "$current" "$step")" ;;
	esac

	case "$scope" in
		user)
			pfu_config_set "$key" "$next" || pfu_bad "$(pfu_msg "Could not save the setting.")"
			;;
		sys)
			printf '\n'
			pfu_ui_cooked pfu_root set "$key" "$next" || pfu_bad "$(pfu_msg "Could not save the setting.")"
			PFU_KV_CACHE=()
			;;
		pam)
			printf '\n'
			if [[ $next == yes ]]; then
				pfu_ui_cooked pfu_root pam-enable "$key" || pfu_bad "$(pfu_msg "Could not save the setting.")"
			else
				pfu_ui_cooked pfu_root pam-disable "$key" || pfu_bad "$(pfu_msg "Could not save the setting.")"
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
# A cursor list in groups, with what the selected setting does under it. The
# frame is assembled in memory and written once, and everything constant is
# resolved before the loop.
pfu_ui_settings() {
	local -a scopes=() keys=() types=() defaults=() labels=() choices=() needs=() values=() rows=()
	local spec scope key type default label choice need locale i j frame row pad dirty=1 cursor=0 shown
	local width=0 wrap cols

	locale="$(pfu_ui_locale)"
	for spec in "${PFU_SETTINGS[@]}"; do
		IFS='|' read -r scope key type default label choice need <<< "$spec"
		# A heading has its label where the key would be.
		[[ $scope == group ]] && label="$key" key=''
		scopes+=("$scope"); keys+=("$key"); types+=("$type"); defaults+=("$default")
		choices+=("$choice"); needs+=("$need")
		pfu_msg_into "$locale" "$label"
		labels+=("$PFU_MSG_RESULT")
		if [[ $scope != group ]]; then
			rows+=($(( ${#keys[@]} - 1 )))
			(( ${#PFU_MSG_RESULT} > width )) && width=${#PFU_MSG_RESULT}
		fi
	done
	local count=${#rows[@]}

	cols="$(tput cols 2>/dev/null)" || cols=80
	[[ $cols =~ ^[0-9]+$ ]] || cols=80
	wrap=$(( cols - 4 ))
	(( wrap > 72 )) && wrap=72
	local rule
	printf -v rule '%*s' "$wrap" ''
	rule="${rule// /─}"

	local title hint shared l_on l_off
	pfu_msg_into "$locale" "Settings"; title="$PFU_MSG_RESULT"
	pfu_msg_into "$locale" "↑↓ select   ←→ or Space: change   q: back"; hint="$PFU_MSG_RESULT"
	pfu_msg_into "$locale" "for all users, asks for your password"; shared="$PFU_MSG_RESULT"
	pfu_msg_into "$locale" "ON"; l_on="$PFU_MSG_RESULT"
	pfu_msg_into "$locale" "OFF"; l_off="$PFU_MSG_RESULT"

	local clearseq
	clearseq="$(clear 2>/dev/null)" || clearseq=$'\033[H\033[2J'

	pfu_cameras_load

	while true; do
		if (( dirty )); then
			PFU_KV_CACHE=()
			declare -A current=()
			for i in "${rows[@]}"; do
				_pfu_setting_value "${scopes[i]}" "${keys[i]}" "${defaults[i]}"
				values[i]="$PFU_SETTING_VALUE"
				current[${keys[i]}]="$PFU_SETTING_VALUE"
			done
			dirty=0
		fi

		frame="$clearseq"$'\n'"${PFU_C_BOLD}${PFU_C_BLUE}  ${title}${PFU_C_RESET}"$'\n'

		local selected=${rows[cursor]} marker before after dim
		for i in "${!keys[@]}"; do
			if [[ ${scopes[i]} == group ]]; then
				frame+=$'\n'"  ${PFU_C_BOLD}${labels[i]}${PFU_C_RESET}"
				# The next row says whose settings these are.
				[[ ${scopes[i + 1]} != user ]] && frame+="  ${PFU_C_DIM}${shared}${PFU_C_RESET}"
				frame+=$'\n'
				continue
			fi
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
			# Arrows on the selected choice: left and right step through it.
			before='  ' after=''
			if (( i == selected )) && [[ ${types[i]} != bool ]]; then
				before="${PFU_C_BLUE}◂${PFU_C_RESET} " after=" ${PFU_C_BLUE}▸${PFU_C_RESET}"
			fi
			dim=''
			[[ -n ${needs[i]} ]] && ! pfu_is_true "${current[${needs[i]}]}" && dim="$PFU_C_DIM"
			pad=$(( width + 2 - ${#labels[i]} ))
			if (( i == selected )); then marker="${PFU_C_BLUE}▸${PFU_C_RESET} "; else marker='  '; fi
			printf -v row '  %s%s%s%s%*s%s%s%s' "$marker" "$dim" "${labels[i]}" "$PFU_C_RESET" "$pad" '' "$before" "$dim$shown$PFU_C_RESET" "$after"
			frame+="$row"$'\n'
		done

		# What the selected setting does, in a box of fixed height so the
		# screen does not jump while moving through the list.
		_pfu_wrap "$wrap" "$(pfu_setting_help "${keys[selected]}" "${values[selected]}")"
		frame+=$'\n'"  ${PFU_C_DIM}${rule}${PFU_C_RESET}"$'\n'
		for j in 0 1 2; do
			frame+="  ${PFU_WRAP_LINES[j]:-}"$'\n'
		done
		frame+=$'\n'"  ${PFU_C_DIM}${hint}${PFU_C_RESET}"$'\n'
		_pfu_ui_take_notices
		frame+="$PFU_UI_NOTICE_TEXT"
		printf '%s' "$frame"

		key="$(pfu_read_key)" || return 0

		case "$key" in
			up|k)   cursor=$(( (cursor - 1 + count) % count )) ;;
			down|j) cursor=$(( (cursor + 1) % count )) ;;
			space|enter|right|l|left|h)
				local step=1
				[[ $key == left || $key == h ]] && step=-1
				_pfu_setting_change "${scopes[selected]}" "${keys[selected]}" "${types[selected]}" "${values[selected]}" "${choices[selected]}" "$step"
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
		_pfu_ui_take_notices
		frame+="$PFU_UI_NOTICE_TEXT"
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
			a|A) pfu_ui_cooked pfu_do_setup ;;
			q|Q|escape) return 0 ;;
			*) ;;
		esac
	done
}

# ---------------------------------------------------------------------------
# The menu
# ---------------------------------------------------------------------------

pfu_ui_menu() {
	local choice keep=0

	pfu_ui_term_raw
	trap 'pfu_ui_term_restore' EXIT INT TERM

	while true; do
		# After a test the menu goes under what the camera saw instead.
		if (( keep )); then
			keep=0
		else
			clear 2>/dev/null || true
		fi
		pfu_head "  $PFU_PRETTY"
		pfu_ui_status
		printf '\n'
		printf '  [1] %s\n' "$(pfu_msg "Turn face unlock on or off")"
		printf '  [2] %s\n' "$(pfu_msg "Add a face")"
		printf '  [3] %s\n' "$(pfu_msg "Faces")"
		printf '  [4] %s\n' "$(pfu_msg "Settings")"
		printf '  [5] %s\n' "$(pfu_msg "Try it")"
		printf '  [q] %s\n' "$(pfu_msg "Quit")"
		_pfu_ui_take_notices
		printf '%s' "$PFU_UI_NOTICE_TEXT"
		printf '\n  > '

		choice="$(pfu_read_key)" || {
			printf '\n'; pfu_ui_term_restore; trap - EXIT INT TERM; return 0
		}
		case "$choice" in
			enter|space|up|down|left|right|escape) choice='' ;;
		esac
		printf '%s\n' "$choice"

		case "$choice" in
			1)
				pfu_config_load
				if [[ $CFG_ENABLED == yes ]]; then
					pfu_ui_cooked pfu_do_disable
				else
					pfu_ui_cooked pfu_do_enable
				fi
				;;
			2) pfu_ui_cooked pfu_do_setup ;;
			3) pfu_ui_faces ;;
			4) pfu_ui_settings ;;
			5)
				printf '\n'
				pfu_ui_cooked pfu_test
				# Already on screen, with the rest of the test.
				PFU_UI_NOTICES=()
				keep=1
				;;
			q|Q) pfu_ui_term_restore; trap - EXIT INT TERM; return 0 ;;
			# Anything else (Enter, arrow keys, stray characters) just
			# redraws. Escape is deliberately not a quit key, so a mistyped
			# arrow key cannot close the menu.
			*) ;;
		esac
	done
}
