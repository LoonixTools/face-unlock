# shellcheck shell=bash
#
# Asking the daemon.
#
# Through plasma-face-unlock-ctl, which prints every message as a line of tab
# separated key=value pairs (see src/ctl/main.cpp). What comes back is parsed
# into plain variables and arrays here, so the rest of the shell code never
# sees the wire format.

# _pfu_fields <line>
# Splits one line of ctl output into the associative array PFU_F.
declare -A PFU_F=()

_pfu_fields() {
	local line="$1" field
	local -a parts
	PFU_F=()
	IFS=$'\t' read -r -a parts <<< "$line"
	for field in "${parts[@]}"; do
		PFU_F["${field%%=*}"]="${field#*=}"
	done
}

pfu_ctl() {
	"$PFU_CTL" "$@"
}

# pfu_status_load
# PFU_ST_REACHABLE is yes when the daemon answered at all. Everything else
# is only filled in then.
pfu_status_load() {
	local out
	PFU_ST_REACHABLE=no
	PFU_ST_FACES=0
	PFU_ST_LOCKOUT=0
	PFU_ST_LAST=0
	PFU_ST_PURPOSE=''
	PFU_ST_CAMERA=''
	PFU_ST_CAMERA_NAME=''
	PFU_ST_CAMERA_PRESENT=no
	PFU_ST_MODELS=no

	out="$(pfu_ctl status 2>/dev/null | tail -n1)"
	_pfu_fields "$out"
	[[ ${PFU_F[ok]:-} == true ]] || return 1

	PFU_ST_REACHABLE=yes
	PFU_ST_FACES="${PFU_F[faces]:-0}"
	PFU_ST_LOCKOUT="${PFU_F[lockout]:-0}"
	PFU_ST_LAST="${PFU_F[lastUnlock]:-0}"
	PFU_ST_PURPOSE="${PFU_F[lastPurpose]:-}"
	PFU_ST_CAMERA="${PFU_F[cameraPath]:-}"
	PFU_ST_CAMERA_NAME="${PFU_F[cameraName]:-}"
	PFU_ST_CAMERA_PRESENT="${PFU_F[cameraPresent]:-false}"
	PFU_ST_MODELS="${PFU_F[models]:-false}"
	return 0
}

# pfu_faces_load
# The caller's faces, into parallel arrays.
pfu_faces_load() {
	local line
	PFU_FACE_IDS=() PFU_FACE_NAMES=() PFU_FACE_ON=() PFU_FACE_SAMPLES=() PFU_FACE_LEARNED=() PFU_FACE_CREATED=()

	while IFS= read -r line; do
		_pfu_fields "$line"
		[[ ${PFU_F[event]:-} == item ]] || continue
		PFU_FACE_IDS+=("${PFU_F[id]}")
		PFU_FACE_NAMES+=("${PFU_F[name]}")
		PFU_FACE_ON+=("${PFU_F[enabled]}")
		PFU_FACE_SAMPLES+=("${PFU_F[samples]:-0}")
		PFU_FACE_LEARNED+=("${PFU_F[adaptive]:-0}")
		PFU_FACE_CREATED+=("${PFU_F[created]:-0}")
	done < <(pfu_ctl list 2>/dev/null)
}

# pfu_cameras_load
pfu_cameras_load() {
	local line
	PFU_CAM_PATHS=() PFU_CAM_NAMES=() PFU_CAM_IR=()
	PFU_CAM_AUTO=''

	while IFS= read -r line; do
		_pfu_fields "$line"
		case "${PFU_F[event]:-}" in
			item)
				PFU_CAM_PATHS+=("${PFU_F[path]}")
				PFU_CAM_NAMES+=("${PFU_F[name]}")
				PFU_CAM_IR+=("${PFU_F[infrared]}")
				;;
			result)
				PFU_CAM_AUTO="${PFU_F[auto]:-}"
				;;
		esac
	done < <(pfu_ctl cameras 2>/dev/null)
}

# pfu_reason_text <reason>
# What a daemon answer means, for people.
pfu_reason_text() {
	case "$1" in
		mismatch)     pfu_msg "The face did not match." ;;
		spoof)        pfu_msg "That looked like a photo or a screen." ;;
		liveness)     pfu_msg "The face matched, but did not blink or move." ;;
		attention)    pfu_msg "The face was not looking at the screen." ;;
		quality)      pfu_msg "The picture was too dark, too blurry or too far away." ;;
		no-face)      pfu_msg "No face in view." ;;
		lockout)      pfu_msg "Face unlock is paused after too many tries. Unlock once with your password." ;;
		not-enrolled) pfu_msg "No face is set up yet." ;;
		camera)       pfu_msg "The camera could not be used." ;;
		models)       pfu_msg "The recognition models are missing. Reinstall the package." ;;
		lid-closed)   pfu_msg "The lid is closed." ;;
		busy)         pfu_msg "The camera is busy with another scan." ;;
		unreachable)  pfu_msg "The face unlock service is not running." ;;
		denied)       pfu_msg "Not allowed." ;;
		*)            pfu_msg "Something went wrong (%s)." "$1" ;;
	esac
	printf '\n'
}

# pfu_test
# One scan, with everything the checks see on one line that keeps updating.
# The bubble shows it too, if the agent is running.
pfu_test() {
	local line started=0 shown='' score='-' matched=0

	pfu_say "  $(pfu_msg "Look at the camera. Blink once, or turn your head a little.")"
	printf '\n'

	while IFS= read -r line; do
		_pfu_fields "$line"
		case "${PFU_F[event]:-}" in
			started)
				started=1
				;;
			face)
				printf '\r\033[K  %s\n' "$(pfu_msg "Face found.")"
				;;
			hint)
				case "${PFU_F[hint]}" in
					blink)  printf '\r\033[K  %s\n' "$(pfu_msg "Recognised. Now blink once, or turn your head a little.")" ;;
					look)   printf '\r\033[K  %s\n' "$(pfu_msg "Look at the screen.")" ;;
					closer) printf '\r\033[K  %s\n' "$(pfu_msg "Move closer to the camera.")" ;;
					light)  printf '\r\033[K  %s\n' "$(pfu_msg "It is too dark to see your face.")" ;;
				esac
				;;
			frame)
				[[ -n ${PFU_F[score]:-} ]] && score="${PFU_F[score]}"
				matched="${PFU_F[matched]:-0}"
				# match, turn of the head, eyes, and how close each photo check is
				# to firing, as numbers for anybody tuning it.
				printf -v shown '  %s %-6s %s %5s°  %s %-5s  %s %-4s %s %-4s  %s %-4s %s %-4s' \
					"$(pfu_msg "match")" "$score" "$(pfu_msg "turn")" "${PFU_F[yaw]:-0}" \
					"$(pfu_msg "eyes")" "${PFU_F[eyes]:-0}" \
					"$(pfu_msg "depth")" "${PFU_F[depth]:-0}" "$(pfu_msg "blink")" "${PFU_F[blink]:-0}" \
					"$(pfu_msg "glare")" "${PFU_F[glare]:-0}" "$(pfu_msg "edge")" "${PFU_F[device]:-0}"
				[[ -n $PFU_INTERACTIVE ]] && printf '\r\033[K%s%s%s' "$PFU_C_DIM" "$shown" "$PFU_C_RESET"
				;;
			result)
				printf '\r\033[K'
				if [[ ${PFU_F[ok]} == true ]]; then
					local how=''
					case "${PFU_F[liveness]:-}" in
						blink) how="$(pfu_msg "blink")" ;;
						depth) how="$(pfu_msg "head turn")" ;;
					esac
					pfu_ok "$(pfu_msg "Recognised as %s (match %s) in %s ms." "${PFU_F[name]}" "${PFU_F[score]}" "${PFU_F[ms]}")"
					[[ -n $how ]] && pfu_say "  $(pfu_msg "Sign of life: %s" "$how")"
				else
					pfu_bad "$(pfu_reason_text "${PFU_F[reason]}")"
					[[ -n ${PFU_F[message]:-} ]] && pfu_say "  ${PFU_F[message]}"
					if [[ -n ${PFU_F[lockout]:-} ]]; then
						pfu_note "$(pfu_msg "That was too many tries. Face unlock is paused until you unlock with your password.")"
					fi
				fi
				(( started )) || true
				return 0
				;;
		esac
	done < <(pfu_ctl test 2>/dev/null)

	printf '\n'
	pfu_bad "$(pfu_reason_text unreachable)"
	return 1
}
