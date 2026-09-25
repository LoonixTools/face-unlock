# shellcheck shell=bash
#
# Asking the daemon.
#
# Through face-unlock-ctl, which prints every message as a line of tab
# separated key=value pairs (see src/ctl/main.cpp). What comes back is parsed
# into plain variables and arrays here, so the rest of the shell code never
# sees the wire format.

# _fu_fields <line>
# Splits one line of ctl output into the associative array FU_F.
declare -A FU_F=()

_fu_fields() {
	local line="$1" field
	local -a parts
	FU_F=()
	IFS=$'\t' read -r -a parts <<< "$line"
	for field in "${parts[@]}"; do
		FU_F["${field%%=*}"]="${field#*=}"
	done
}

fu_ctl() {
	"$FU_CTL" "$@"
}

# fu_status_load
# FU_ST_REACHABLE is yes when the daemon answered at all. Everything else
# is only filled in then.
fu_status_load() {
	local out
	FU_ST_REACHABLE=no
	FU_ST_FACES=0
	FU_ST_LOCKOUT=0
	FU_ST_LAST=0
	FU_ST_PURPOSE=''
	FU_ST_CAMERA=''
	FU_ST_CAMERA_NAME=''
	FU_ST_CAMERA_PRESENT=no
	FU_ST_MODELS=no

	out="$(fu_ctl status 2>/dev/null | tail -n1)"
	_fu_fields "$out"
	[[ ${FU_F[ok]:-} == true ]] || return 1

	FU_ST_REACHABLE=yes
	FU_ST_FACES="${FU_F[faces]:-0}"
	FU_ST_LOCKOUT="${FU_F[lockout]:-0}"
	FU_ST_LAST="${FU_F[lastUnlock]:-0}"
	FU_ST_PURPOSE="${FU_F[lastPurpose]:-}"
	FU_ST_CAMERA="${FU_F[cameraPath]:-}"
	FU_ST_CAMERA_NAME="${FU_F[cameraName]:-}"
	FU_ST_CAMERA_PRESENT="${FU_F[cameraPresent]:-false}"
	FU_ST_MODELS="${FU_F[models]:-false}"
	return 0
}

# fu_faces_load
# The caller's faces, into parallel arrays.
fu_faces_load() {
	local line
	FU_FACE_IDS=() FU_FACE_NAMES=() FU_FACE_ON=() FU_FACE_SAMPLES=() FU_FACE_LEARNED=() FU_FACE_CREATED=()

	while IFS= read -r line; do
		_fu_fields "$line"
		[[ ${FU_F[event]:-} == item ]] || continue
		FU_FACE_IDS+=("${FU_F[id]}")
		FU_FACE_NAMES+=("${FU_F[name]}")
		FU_FACE_ON+=("${FU_F[enabled]}")
		FU_FACE_SAMPLES+=("${FU_F[samples]:-0}")
		FU_FACE_LEARNED+=("${FU_F[adaptive]:-0}")
		FU_FACE_CREATED+=("${FU_F[created]:-0}")
	done < <(fu_ctl list 2>/dev/null)
}

# fu_cameras_load
fu_cameras_load() {
	local line
	FU_CAM_PATHS=() FU_CAM_NAMES=() FU_CAM_IR=()
	FU_CAM_AUTO=''

	while IFS= read -r line; do
		_fu_fields "$line"
		case "${FU_F[event]:-}" in
			item)
				FU_CAM_PATHS+=("${FU_F[path]}")
				FU_CAM_NAMES+=("${FU_F[name]}")
				FU_CAM_IR+=("${FU_F[infrared]}")
				;;
			result)
				FU_CAM_AUTO="${FU_F[auto]:-}"
				;;
		esac
	done < <(fu_ctl cameras 2>/dev/null)
}

# fu_reason_text <reason>
# What a daemon answer means, for people.
fu_reason_text() {
	case "$1" in
		mismatch)     fu_msg "The face did not match." ;;
		spoof)        fu_msg "That looked like a photo or a screen." ;;
		liveness)     fu_msg "The face matched, but did not blink or move." ;;
		attention)    fu_msg "The face was not looking at the screen." ;;
		quality)      fu_msg "The picture was too dark, too blurry or too far away." ;;
		no-face)      fu_msg "No face in view." ;;
		lockout)      fu_msg "Face unlock is paused after too many tries. Unlock once with your password." ;;
		not-enrolled) fu_msg "No face is set up yet." ;;
		camera)       fu_msg "The camera could not be used." ;;
		models)       fu_msg "The recognition models are missing. Reinstall the package." ;;
		lid-closed)   fu_msg "The lid is closed." ;;
		busy)         fu_msg "The camera is busy with another scan." ;;
		unreachable)  fu_msg "The face unlock service is not running." ;;
		denied)       fu_msg "Not allowed." ;;
		*)            fu_msg "Something went wrong (%s)." "$1" ;;
	esac
	printf '\n'
}

# fu_test
# One scan, with everything the checks see on one line that keeps updating.
# The bubble shows it too, if the agent is running.
fu_test() {
	local line started=0 shown='' score='-' matched=0

	fu_say "  $(fu_msg "Look at the camera. Blink once, or turn your head a little.")"
	printf '\n'

	while IFS= read -r line; do
		_fu_fields "$line"
		case "${FU_F[event]:-}" in
			started)
				started=1
				;;
			face)
				printf '\r\033[K  %s\n' "$(fu_msg "Face found.")"
				;;
			hint)
				case "${FU_F[hint]}" in
					blink)  printf '\r\033[K  %s\n' "$(fu_msg "Recognised. Now blink once, or turn your head a little.")" ;;
					look)   printf '\r\033[K  %s\n' "$(fu_msg "Look at the screen.")" ;;
					closer) printf '\r\033[K  %s\n' "$(fu_msg "Move closer to the camera.")" ;;
					light)  printf '\r\033[K  %s\n' "$(fu_msg "It is too dark to see your face.")" ;;
				esac
				;;
			frame)
				[[ -n ${FU_F[score]:-} ]] && score="${FU_F[score]}"
				matched="${FU_F[matched]:-0}"
				# match, turn of the head, eyes, and how close each photo check is
				# to firing, as numbers for anybody tuning it.
				printf -v shown '  %s %-6s %s %5s°  %s %-5s  %s %-4s %s %-4s  %s %-4s %s %-4s' \
					"$(fu_msg "match")" "$score" "$(fu_msg "turn")" "${FU_F[yaw]:-0}" \
					"$(fu_msg "eyes")" "${FU_F[eyes]:-0}" \
					"$(fu_msg "depth")" "${FU_F[depth]:-0}" "$(fu_msg "blink")" "${FU_F[blink]:-0}" \
					"$(fu_msg "glare")" "${FU_F[glare]:-0}" "$(fu_msg "edge")" "${FU_F[device]:-0}"
				[[ -n $FU_INTERACTIVE ]] && printf '\r\033[K%s%s%s' "$FU_C_DIM" "$shown" "$FU_C_RESET"
				;;
			result)
				printf '\r\033[K'
				if [[ ${FU_F[ok]} == true ]]; then
					local how=''
					case "${FU_F[liveness]:-}" in
						blink) how="$(fu_msg "blink")" ;;
						depth) how="$(fu_msg "head turn")" ;;
					esac
					fu_ok "$(fu_msg "Recognised as %s (match %s) in %s ms." "${FU_F[name]}" "${FU_F[score]}" "${FU_F[ms]}")"
					[[ -n $how ]] && fu_say "  $(fu_msg "Sign of life: %s" "$how")"
				else
					fu_bad "$(fu_reason_text "${FU_F[reason]}")"
					[[ -n ${FU_F[message]:-} ]] && fu_say "  ${FU_F[message]}"
					if [[ -n ${FU_F[lockout]:-} ]]; then
						fu_note "$(fu_msg "That was too many tries. Face unlock is paused until you unlock with your password.")"
					fi
				fi
				(( started )) || true
				return 0
				;;
		esac
	done < <(fu_ctl test 2>/dev/null)

	printf '\n'
	fu_bad "$(fu_reason_text unreachable)"
	return 1
}
