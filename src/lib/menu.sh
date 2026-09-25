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
FU_TERM_SAVED=''

fu_ui_term_raw() {
	fu_have stty || return 0
	[[ -t 0 ]] || return 0
	[[ -n $FU_TERM_SAVED ]] && return 0

	FU_TERM_SAVED="$(stty -g 2>/dev/null)" || { FU_TERM_SAVED=''; return 0; }
	stty -icanon -echo min 1 time 0 2>/dev/null || true
}

fu_ui_term_restore() {
	[[ -n $FU_TERM_SAVED ]] || return 0
	stty "$FU_TERM_SAVED" 2>/dev/null || true
	FU_TERM_SAVED=''
}

# Runs an action with the terminal handed back to normal line mode, so anything
# it prints or prompts for (sudo's password prompt, above all) behaves the way
# a program expects.
fu_ui_cooked() {
	fu_ui_term_restore
	"$@"
	local rc=$?
	fu_ui_term_raw
	return $rc
}

# fu_read_key
# One keypress, resolved to a symbolic name. Arrow keys arrive as ESC [ A, so
# the tail of the sequence is consumed here rather than being mistaken for
# three separate presses.
fu_read_key() {
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

# fu_ui_read_line <initial>
# A minimal line editor built on fu_read_key, with the result in
# FU_LINE_RESULT. This exists instead of bash's own `read -r` because mixing
# line mode into a single-key interface breaks it: after one cooked-mode read
# the following `read -sn1` stops receiving keystrokes entirely.
FU_LINE_RESULT=''

fu_ui_read_line() {
	local buf="${1:-}" key

	FU_LINE_RESULT=''
	printf '%s' "$buf"

	while true; do
		key="$(fu_read_key)" || { printf '\n'; return 1; }

		case "$key" in
			enter)
				printf '\n'
				FU_LINE_RESULT="$buf"
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

# _fu_ui_take_notices
# The messages the last action left, as lines for under a frame, in
# FU_UI_NOTICE_TEXT. Each is shown once.
FU_UI_NOTICE_TEXT=''

_fu_ui_take_notices() {
	local n
	FU_UI_NOTICE_TEXT=''
	(( ${#FU_UI_NOTICES[@]} )) || return 0
	FU_UI_NOTICE_TEXT=$'\n'
	for n in "${FU_UI_NOTICES[@]}"; do
		FU_UI_NOTICE_TEXT+="  $n"$'\n'
	done
	FU_UI_NOTICES=()
}

# fu_ui_confirm <question>
fu_ui_confirm() {
	local key hint yes
	# TRANSLATORS: the letter after "[" is the key for yes. y works too.
	hint="$(fu_msg "[y/N]")"
	yes="${hint:1:1}"
	printf '\n  %s %s ' "$1" "$hint"
	key="$(fu_read_key)" || return 1
	printf '%s\n' "$key"
	[[ ${key,,} == y || ${key,,} == "${yes,,}" ]]
}

# _fu_width <text>
# The columns the text takes, into FU_WIDTH. ${#s} counts characters, but
# Chinese, Japanese and Korean ones take two columns each.
FU_WIDTH=0

_fu_width() {
	local wide="${1//[^　-〿぀-ヿ㐀-䶿一-鿿가-힯豈-﫿＀-｠]/}"
	FU_WIDTH=$(( ${#1} + ${#wide} ))
}

# _fu_row <label> <value>
# printf's %-28s pads by bytes, so a label containing "ü" comes out one column
# short. The padding is computed here instead.
_fu_row() {
	local label="$1" value="$2" pad
	_fu_width "$label"
	pad=$(( 30 - FU_WIDTH ))
	(( pad < 0 )) && pad=0
	printf '  %s%*s %s\n' "$label" "$pad" '' "$value"
}

_fu_onoff() {
	if [[ $1 == yes ]]; then
		printf '%s%s%s' "$FU_C_GREEN" "$(fu_msg "ON")" "$FU_C_RESET"
	else
		printf '%s%s%s' "$FU_C_DIM" "$(fu_msg "OFF")" "$FU_C_RESET"
	fi
}

_fu_small_onoff() {
	if [[ $1 == yes ]]; then
		printf '%s%s%s' "$FU_C_GREEN" "$(fu_msg "on")" "$FU_C_RESET"
	else
		printf '%s%s%s' "$FU_C_DIM" "$(fu_msg "off")" "$FU_C_RESET"
	fi
}

# fu_value_label <Key> <value>
# How a setting's value reads in the menu.
fu_value_label() {
	case "$1:$2" in
		Liveness:heavy)     fu_msg "strict" ;;
		Liveness:light)     fu_msg "basic" ;;
		Liveness:off)       fu_msg "off" ;;
		Strictness:normal)  fu_msg "normal" ;;
		Strictness:strict)  fu_msg "strict" ;;
		Strictness:relaxed) fu_msg "relaxed" ;;
		BubbleStyle:full)   fu_msg "island with the face" ;;
		BubbleStyle:minimal) fu_msg "small pill with a lock" ;;
		AnimationSpeed:normal) fu_msg "normal" ;;
		AnimationSpeed:fast) fu_msg "fast" ;;
		AnimationSpeed:slow) fu_msg "slow" ;;
		ScanSeconds:*)      fu_msg "%s seconds" "$2" ;;
		Camera:auto)        fu_msg "automatic" ;;
		*)                  printf '%s' "$2" ;;
	esac
}

# ---------------------------------------------------------------------------
# Status
# ---------------------------------------------------------------------------

# fu_ui_status
# Shared by the `status` subcommand and the menu header.
fu_ui_status() {
	local names='' i camera agent=yes

	fu_config_load
	fu_status_load

	_fu_row "$(fu_msg "Face unlock")" "$(_fu_onoff "$CFG_ENABLED")"
	printf '\n'

	if [[ $FU_ST_REACHABLE != yes ]]; then
		_fu_row "$(fu_msg "Service")" "${FU_C_YELLOW}$(fu_msg "not running")${FU_C_RESET}"
		if [[ $CFG_ENABLED == yes ]]; then
			printf '\n  %s%s%s\n' "$FU_C_DIM" "$(fu_msg "Turning face unlock on again starts it.")" "$FU_C_RESET"
		fi
		return 0
	fi

	fu_faces_load
	for i in "${!FU_FACE_NAMES[@]}"; do
		[[ ${FU_FACE_ON[i]} == true ]] || continue
		names="${names:+$names, }${FU_FACE_NAMES[i]}"
	done
	if (( ${#FU_FACE_IDS[@]} == 0 )); then
		_fu_row "$(fu_msg "Faces")" "${FU_C_YELLOW}$(fu_msg "none set up yet")${FU_C_RESET}"
	else
		_fu_row "$(fu_msg "Faces")" "${FU_ST_FACES} ${FU_C_DIM}(${names:-$(fu_msg "all turned off")})${FU_C_RESET}"
	fi

	if [[ $FU_ST_CAMERA_PRESENT == true ]]; then
		camera="${FU_ST_CAMERA_NAME:-$FU_ST_CAMERA}"
	else
		camera="${FU_C_YELLOW}$(fu_msg "none found")${FU_C_RESET}"
	fi
	_fu_row "$(fu_msg "Camera")" "$camera"

	_fu_row "$(fu_msg "Lock screen")" "$(_fu_small_onoff "$( [[ $CFG_ENABLED == yes && $CFG_LOCK == yes ]] && echo yes || echo no)")"
	if [[ $CFG_ENABLED == yes && $CFG_LOCK == yes ]] && fu_agent_available && ! fu_agent_running; then
		agent=no
	fi
	_fu_row "$(fu_msg "sudo")" "$(_fu_small_onoff "$(fu_pam_enabled sudo && echo yes || echo no)")"
	_fu_row "$(fu_msg "Admin prompts")" "$(_fu_small_onoff "$(fu_pam_enabled polkit-1 && echo yes || echo no)")"
	_fu_row "$(fu_msg "Photo check")" "$(fu_value_label Liveness "$CFG_LIVENESS")"

	if (( FU_ST_LOCKOUT > 0 )); then
		_fu_row "$(fu_msg "Paused")" "${FU_C_YELLOW}$(fu_msg "for %d more minutes, or until the password is used" "$(( (FU_ST_LOCKOUT + 59) / 60 ))")${FU_C_RESET}"
	fi
	_fu_row "$(fu_msg "Last unlock")" "$(fu_time_ago "$FU_ST_LAST")"

	if [[ $FU_ST_MODELS != true ]]; then
		printf '\n  %s%s%s\n' "$FU_C_RED" "$(fu_msg "The recognition models are missing. Reinstall the package.")" "$FU_C_RESET"
	fi
	if [[ $agent == no ]]; then
		printf '\n  %s%s%s\n' "$FU_C_YELLOW" "$(fu_msg "The lock screen agent is not running.")" "$FU_C_RESET"
		fu_agent_autostarts || fu_agent_hint
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
#          that is off. Layers is no setting: whether the desktop can show
#          the bubble at all (GNOME cannot).
FU_SETTINGS=(
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
	"user|Bubble|bool|yes|Show the bubble||Layers"
	"user|BubbleStyle|choice|full|Style|full,minimal|Bubble"
	"user|AnimationSpeed|choice|normal|Animation speed|slow,normal,fast|Bubble"
	"user|BubbleForPrompts|bool|yes|Also for sudo and admin prompts||Bubble"
)

# fu_setting_help <Key> <value>
# What a setting does, shown under the list for the selected one.
fu_setting_help() {
	if [[ $FU_DESKTOP == gnome && $1 =~ ^(Bubble|BubbleStyle|AnimationSpeed|BubbleForPrompts)$ ]]; then
		fu_msg "GNOME does not let other programs show above its windows, so there is no bubble on GNOME."
		return
	fi
	case "$1:$2" in
		LockScreen:*)       fu_msg "Unlocks the lock screen when it sees your face. Off: only your password works there." ;;
		ScanOnWake:*)       fu_msg "Scans when you press a key or move the mouse on the lock screen, and when the computer wakes up." ;;
		ScanOnLock:*)       fu_msg "Scans as soon as the screen locks. Off by default: if you lock it yourself, it would unlock again right away." ;;
		sudo:*)             fu_msg "sudo takes your face instead of the password. No match: you type the password as usual." ;;
		polkit-1:*)         fu_msg "The password windows of your desktop and apps, for example when you install software. No match: you type the password." ;;
		Liveness:heavy)     fu_msg "You have to blink or turn your head a little. This also stops printed photos." ;;
		Liveness:off)       fu_msg "No check at all. Only for trying out a camera." ;;
		Liveness:*)         fu_msg "Stops photos on a phone, a tablet or glossy paper. You do not have to blink. A matte printed photo can get through." ;;
		Strictness:strict)  fu_msg "Fewer wrong matches, but it may not know you with glasses or in bad light." ;;
		Strictness:relaxed) fu_msg "Knows you more easily, but also somebody who looks a lot like you." ;;
		Strictness:*)       fu_msg "The default. Right for most people." ;;
		Attention:*)        fu_msg "Your eyes have to be open and on the screen. So it does not unlock while you look away or sleep." ;;
		Camera:*)           fu_msg "Automatic takes the first normal camera. After a change, set up your face again. An infrared camera needs its light on (linux-enable-ir-emitter)." ;;
		ScanSeconds:*)      fu_msg "How long the camera looks for your face before it gives up." ;;
		Adapt:*)            fu_msg "After a sure match it keeps how you look now. So a new haircut or glasses need no new setup." ;;
		SkipLidClosed:*)    fu_msg "No scan while the laptop is closed, for example at a desk with an external screen." ;;
		Bubble:*)           fu_msg "The bubble at the top of the screen shows what the camera is doing." ;;
		BubbleStyle:minimal) fu_msg "A small pill with a lock that opens." ;;
		BubbleStyle:*)      fu_msg "An island with a face that looks around, then rings and a tick." ;;
		AnimationSpeed:*)   fu_msg "How fast the bubble moves." ;;
		BubbleForPrompts:*) fu_msg "Shows the bubble when sudo or an admin prompt scans your face, too." ;;
	esac
}

# _fu_wrap <width> <text>
# Word wrap, into FU_WRAP_LINES.
FU_WRAP_LINES=()

_fu_wrap() {
	local width="$1" word line='' used=0 c i
	local -a words
	FU_WRAP_LINES=()
	read -r -a words <<< "$2"
	for word in "${words[@]}"; do
		_fu_width "$word"
		if (( used > 0 && used + 1 + FU_WIDTH > width )); then
			FU_WRAP_LINES+=("$line")
			line='' used=0
		fi
		(( used > 0 )) && line+=' ' used=$(( used + 1 ))
		if (( used + FU_WIDTH <= width )); then
			line+="$word" used=$(( used + FU_WIDTH ))
			continue
		fi
		# Longer than a line: Chinese and Japanese have no spaces, so break
		# between any two characters.
		for (( i = 0; i < ${#word}; i++ )); do
			c="${word:i:1}"
			_fu_width "$c"
			if (( used + FU_WIDTH > width )); then
				FU_WRAP_LINES+=("$line")
				line='' used=0
			fi
			line+="$c" used=$(( used + FU_WIDTH ))
		done
	done
	[[ -n $line ]] && FU_WRAP_LINES+=("$line")
	return 0
}

# _fu_setting_value <scope> <Key> <default>
# The current value, in FU_SETTING_VALUE.
FU_SETTING_VALUE=''

_fu_setting_value() {
	case "$1" in
		user) _fu_kv_lookup "$FU_CONFIG" "$2" "$3"; FU_SETTING_VALUE="$FU_KV_VALUE" ;;
		sys)  _fu_kv_lookup "$FU_SYSCONFIG" "$2" "$3"; FU_SETTING_VALUE="$FU_KV_VALUE" ;;
		pam)  if fu_pam_enabled "$2"; then FU_SETTING_VALUE=yes; else FU_SETTING_VALUE=no; fi ;;
	esac
}

# _fu_step <current> <step> <choice>...
# The choice <step> (1 or -1) away from the current one, round at the ends.
_fu_step() {
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

# _fu_next_choice <current> <a,b,c> <step>
_fu_next_choice() {
	local -a choices
	IFS=',' read -r -a choices <<< "$2"
	_fu_step "$1" "$3" "${choices[@]}"
}

# _fu_next_camera <current> <step>
_fu_next_camera() {
	fu_cameras_load
	_fu_step "$1" "$2" auto "${FU_CAM_PATHS[@]}"
}

_fu_camera_label() {
	local value="$1" i
	if [[ $value == auto ]]; then
		for i in "${!FU_CAM_PATHS[@]}"; do
			if [[ ${FU_CAM_PATHS[i]} == "$FU_CAM_AUTO" ]]; then
				fu_msg "automatic (%s)" "${FU_CAM_NAMES[i]}"
				return
			fi
		done
		fu_msg "automatic"
		return
	fi
	for i in "${!FU_CAM_PATHS[@]}"; do
		if [[ ${FU_CAM_PATHS[i]} == "$value" ]]; then
			printf '%s' "${FU_CAM_NAMES[i]}"
			[[ ${FU_CAM_IR[i]} == true ]] && printf ' %s' "$(fu_msg "(infrared)")"
			return
		fi
	done
	printf '%s %s' "$value" "$(fu_msg "(not connected)")"
}

# _fu_setting_change <scope> <Key> <type> <current> <choices> <step>
_fu_setting_change() {
	local scope="$1" key="$2" type="$3" current="$4" options="$5" step="$6" next

	case "$type" in
		bool)   if fu_is_true "$current"; then next=no; else next=yes; fi ;;
		choice) next="$(_fu_next_choice "$current" "$options" "$step")" ;;
		camera) next="$(_fu_next_camera "$current" "$step")" ;;
	esac

	case "$scope" in
		user)
			fu_config_set "$key" "$next" || fu_bad "$(fu_msg "Could not save the setting.")"
			;;
		sys)
			printf '\n'
			fu_ui_cooked fu_root set "$key" "$next" || fu_bad "$(fu_msg "Could not save the setting.")"
			FU_KV_CACHE=()
			;;
		pam)
			printf '\n'
			if [[ $next == yes ]]; then
				fu_ui_cooked fu_root pam-enable "$key" || fu_bad "$(fu_msg "Could not save the setting.")"
			else
				fu_ui_cooked fu_root pam-disable "$key" || fu_bad "$(fu_msg "Could not save the setting.")"
			fi
			# Remembered, so that turning face unlock off and on again brings
			# it back.
			case "$key" in
				sudo)     fu_config_set Sudo "$next" ;;
				polkit-1) fu_config_set Polkit "$next" ;;
			esac
			;;
	esac
}

# fu_ui_settings
# A cursor list in groups, with what the selected setting does under it. The
# frame is assembled in memory and written once, and everything constant is
# resolved before the loop.
fu_ui_settings() {
	local -a scopes=() keys=() types=() defaults=() labels=() widths=() choices=() needs=() values=() rows=()
	local spec scope key type default label choice need locale i j frame row pad dirty=1 cursor=0 shown
	local width=0 wrap cols

	locale="$(fu_ui_locale)"
	for spec in "${FU_SETTINGS[@]}"; do
		IFS='|' read -r scope key type default label choice need <<< "$spec"
		# A heading has its label where the key would be.
		[[ $scope == group ]] && label="$key" key=''
		scopes+=("$scope"); keys+=("$key"); types+=("$type"); defaults+=("$default")
		choices+=("$choice"); needs+=("$need")
		fu_msg_into "$locale" "$label"
		labels+=("$FU_MSG_RESULT")
		_fu_width "$FU_MSG_RESULT"
		widths+=("$FU_WIDTH")
		if [[ $scope != group ]]; then
			rows+=($(( ${#keys[@]} - 1 )))
			(( FU_WIDTH > width )) && width=$FU_WIDTH
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
	fu_msg_into "$locale" "Settings"; title="$FU_MSG_RESULT"
	fu_msg_into "$locale" "↑↓ select   ←→ or Space: change   q: back"; hint="$FU_MSG_RESULT"
	fu_msg_into "$locale" "for all users, asks for your password"; shared="$FU_MSG_RESULT"
	fu_msg_into "$locale" "ON"; l_on="$FU_MSG_RESULT"
	fu_msg_into "$locale" "OFF"; l_off="$FU_MSG_RESULT"

	local clearseq
	clearseq="$(clear 2>/dev/null)" || clearseq=$'\033[H\033[2J'

	fu_cameras_load

	while true; do
		if (( dirty )); then
			FU_KV_CACHE=()
			declare -A current=()
			for i in "${rows[@]}"; do
				_fu_setting_value "${scopes[i]}" "${keys[i]}" "${defaults[i]}"
				values[i]="$FU_SETTING_VALUE"
				current[${keys[i]}]="$FU_SETTING_VALUE"
			done
			current[Layers]=yes
			[[ $FU_DESKTOP == gnome ]] && current[Layers]=no current[Bubble]=no
			dirty=0
		fi

		frame="$clearseq"$'\n'"${FU_C_BOLD}${FU_C_BLUE}  ${title}${FU_C_RESET}"$'\n'

		local selected=${rows[cursor]} marker before after dim
		for i in "${!keys[@]}"; do
			if [[ ${scopes[i]} == group ]]; then
				frame+=$'\n'"  ${FU_C_BOLD}${labels[i]}${FU_C_RESET}"
				# The next row says whose settings these are.
				[[ ${scopes[i + 1]} != user ]] && frame+="  ${FU_C_DIM}${shared}${FU_C_RESET}"
				frame+=$'\n'
				continue
			fi
			case "${types[i]}" in
				bool)
					if fu_is_true "${values[i]}"; then
						shown="${FU_C_GREEN}${l_on}${FU_C_RESET}"
					else
						shown="${FU_C_DIM}${l_off}${FU_C_RESET}"
					fi
					;;
				camera) shown="$(_fu_camera_label "${values[i]}")" ;;
				*)      shown="$(fu_value_label "${keys[i]}" "${values[i]}")" ;;
			esac
			# Arrows on the selected choice: left and right step through it.
			before='  ' after=''
			if (( i == selected )) && [[ ${types[i]} != bool ]]; then
				before="${FU_C_BLUE}◂${FU_C_RESET} " after=" ${FU_C_BLUE}▸${FU_C_RESET}"
			fi
			dim=''
			[[ -n ${needs[i]} ]] && ! fu_is_true "${current[${needs[i]}]}" && dim="$FU_C_DIM"
			pad=$(( width + 2 - widths[i] ))
			if (( i == selected )); then marker="${FU_C_BLUE}▸${FU_C_RESET} "; else marker='  '; fi
			printf -v row '  %s%s%s%s%*s%s%s%s' "$marker" "$dim" "${labels[i]}" "$FU_C_RESET" "$pad" '' "$before" "$dim$shown$FU_C_RESET" "$after"
			frame+="$row"$'\n'
		done

		# What the selected setting does, in a box of fixed height so the
		# screen does not jump while moving through the list.
		_fu_wrap "$wrap" "$(fu_setting_help "${keys[selected]}" "${values[selected]}")"
		frame+=$'\n'"  ${FU_C_DIM}${rule}${FU_C_RESET}"$'\n'
		for j in 0 1 2; do
			frame+="  ${FU_WRAP_LINES[j]:-}"$'\n'
		done
		frame+=$'\n'"  ${FU_C_DIM}${hint}${FU_C_RESET}"$'\n'
		_fu_ui_take_notices
		frame+="$FU_UI_NOTICE_TEXT"
		printf '%s' "$frame"

		key="$(fu_read_key)" || return 0

		case "$key" in
			up|k)   cursor=$(( (cursor - 1 + count) % count )) ;;
			down|j) cursor=$(( (cursor + 1) % count )) ;;
			space|enter|right|l|left|h)
				local step=1
				[[ $key == left || $key == h ]] && step=-1
				_fu_setting_change "${scopes[selected]}" "${keys[selected]}" "${types[selected]}" "${values[selected]}" "${choices[selected]}" "$step"
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

fu_ui_faces() {
	local key frame row pad i cursor=0 count locale shown

	locale="$(fu_ui_locale)"
	local title hint empty l_on l_off
	fu_msg_into "$locale" "Faces"; title="$FU_MSG_RESULT"
	# TRANSLATORS: keep the letters, they are the keys.
	fu_msg_into "$locale" "Up/Down: select, Space: on/off, r: rename, d: delete, a: add, q: back"; hint="$FU_MSG_RESULT"
	fu_msg_into "$locale" "No face is set up yet. Press a to add one."; empty="$FU_MSG_RESULT"
	fu_msg_into "$locale" "on"; l_on="$FU_MSG_RESULT"
	fu_msg_into "$locale" "off"; l_off="$FU_MSG_RESULT"

	local clearseq
	clearseq="$(clear 2>/dev/null)" || clearseq=$'\033[H\033[2J'

	while true; do
		fu_faces_load
		count=${#FU_FACE_IDS[@]}
		(( cursor >= count )) && cursor=$(( count > 0 ? count - 1 : 0 ))

		frame="$clearseq"$'\n'"${FU_C_BOLD}${FU_C_BLUE}  ${title}${FU_C_RESET}"$'\n\n'
		if (( count == 0 )); then
			frame+="  ${FU_C_DIM}${empty}${FU_C_RESET}"$'\n'
		fi
		local marker selected="${FU_C_BLUE}▸${FU_C_RESET} " samples
		for i in "${!FU_FACE_IDS[@]}"; do
			if [[ ${FU_FACE_ON[i]} == true ]]; then shown="${FU_C_GREEN}${l_on}${FU_C_RESET}"; else shown="${FU_C_DIM}${l_off}${FU_C_RESET}"; fi
			samples="$(fu_msg "%s samples, %s learned" "${FU_FACE_SAMPLES[i]}" "${FU_FACE_LEARNED[i]}")"
			_fu_width "${FU_FACE_NAMES[i]}"
			pad=$(( 30 - FU_WIDTH ))
			(( pad < 0 )) && pad=0
			if (( i == cursor )); then marker="$selected"; else marker='  '; fi
			printf -v row '  %s%s%*s %s  %s%s%s' "$marker" "${FU_FACE_NAMES[i]}" "$pad" '' "$shown" "$FU_C_DIM" "$samples" "$FU_C_RESET"
			frame+="$row"$'\n'
		done
		frame+=$'\n'"  ${FU_C_DIM}${hint}${FU_C_RESET}"$'\n'
		_fu_ui_take_notices
		frame+="$FU_UI_NOTICE_TEXT"
		printf '%s' "$frame"

		key="$(fu_read_key)" || return 0
		case "$key" in
			up|k)   (( count )) && cursor=$(( (cursor - 1 + count) % count )) ;;
			down|j) (( count )) && cursor=$(( (cursor + 1) % count )) ;;
			space|enter)
				(( count )) || continue
				if [[ ${FU_FACE_ON[cursor]} == true ]]; then
					fu_ctl disable "${FU_FACE_IDS[cursor]}" > /dev/null
				else
					fu_ctl enable "${FU_FACE_IDS[cursor]}" > /dev/null
				fi
				;;
			r|R)
				(( count )) || continue
				printf '\n  %s ' "$(fu_msg "New name:")"
				if fu_ui_read_line "${FU_FACE_NAMES[cursor]}" && [[ -n $FU_LINE_RESULT ]]; then
					fu_ctl rename "${FU_FACE_IDS[cursor]}" "$FU_LINE_RESULT" > /dev/null
				fi
				;;
			d|D)
				(( count )) || continue
				if fu_ui_confirm "$(fu_msg "Delete \"%s\"?" "${FU_FACE_NAMES[cursor]}")"; then
					fu_ctl remove "${FU_FACE_IDS[cursor]}" > /dev/null
				fi
				;;
			a|A) fu_ui_cooked fu_do_setup ;;
			q|Q|escape) return 0 ;;
			*) ;;
		esac
	done
}

# ---------------------------------------------------------------------------
# The menu
# ---------------------------------------------------------------------------

fu_ui_menu() {
	local choice keep=0

	fu_ui_term_raw
	trap 'fu_ui_term_restore' EXIT INT TERM

	while true; do
		# After a test the menu goes under what the camera saw instead.
		if (( keep )); then
			keep=0
		else
			clear 2>/dev/null || true
		fi
		fu_head "  $FU_PRETTY"
		fu_ui_status
		printf '\n'
		printf '  [1] %s\n' "$(fu_msg "Turn face unlock on or off")"
		printf '  [2] %s\n' "$(fu_msg "Add a face")"
		printf '  [3] %s\n' "$(fu_msg "Faces")"
		printf '  [4] %s\n' "$(fu_msg "Settings")"
		printf '  [5] %s\n' "$(fu_msg "Try it")"
		printf '  [q] %s\n' "$(fu_msg "Quit")"
		_fu_ui_take_notices
		printf '%s' "$FU_UI_NOTICE_TEXT"
		printf '\n  > '

		choice="$(fu_read_key)" || {
			printf '\n'; fu_ui_term_restore; trap - EXIT INT TERM; return 0
		}
		case "$choice" in
			enter|space|up|down|left|right|escape) choice='' ;;
		esac
		printf '%s\n' "$choice"

		case "$choice" in
			1)
				fu_config_load
				if [[ $CFG_ENABLED == yes ]]; then
					fu_ui_cooked fu_do_disable
				else
					fu_ui_cooked fu_do_enable
				fi
				;;
			2) fu_ui_cooked fu_do_setup ;;
			3) fu_ui_faces ;;
			4) fu_ui_settings ;;
			5)
				printf '\n'
				fu_ui_cooked fu_test
				# Already on screen, with the rest of the test.
				FU_UI_NOTICES=()
				keep=1
				;;
			q|Q) fu_ui_term_restore; trap - EXIT INT TERM; return 0 ;;
			# Anything else (Enter, arrow keys, stray characters) just
			# redraws. Escape is deliberately not a quit key, so a mistyped
			# arrow key cannot close the menu.
			*) ;;
		esac
	done
}
