# shellcheck shell=bash
#
# The few things that need root, and how the menu gets them done.
#
# The menu runs as the user. When it needs root it runs this same program
# again through sudo (or run0, or doas) with --root and one verb, the way
# cachy-auto-update does, so the password is typed into the terminal the user
# is already looking at. The verbs are all there is: there is no way to hand
# the root side a command of one's own.
#
#   --root set <Key> <Value>     one line of /etc/plasma-face-unlock/config
#   --root pam-enable <service>  sudo or polkit-1, see pam.sh
#   --root pam-disable <service>
#   --root socket-enable         start the daemon's socket, and at boot
#   --root socket-disable

PFU_SELF="${PFU_SELF:-$(readlink -f "${BASH_SOURCE[1]:-$0}")}"

# What each system setting may be set to. Anything else is refused on the root
# side, whatever the menu sent.
declare -A PFU_SYS_VALID=(
	[Camera]='^(auto|/dev/video[0-9]+)$'
	[Liveness]='^(off|light|heavy)$'
	[Strictness]='^(relaxed|normal|strict)$'
	[Attention]='^(yes|no)$'
	[ScanSeconds]='^([2-9]|1[0-5])$'
	[Adapt]='^(yes|no)$'
	[SkipLidClosed]='^(yes|no)$'
	[MaxFailures]='^([1-9]|1[0-9]|20)$'
	[LockoutMinutes]='^[1-9][0-9]{0,3}$'
)

# pfu_root <verb> [args...]
pfu_root() {
	local candidate

	if [[ $EUID -eq 0 ]]; then
		pfu_root_verb "$@"
		return
	fi
	for candidate in sudo run0 doas; do
		if pfu_have "$candidate"; then
			"$candidate" "$PFU_SELF" --root "$@"
			return
		fi
	done
	pfu_bad "$(pfu_msg "This needs root, and neither sudo, run0 nor doas is installed.")"
	return 1
}

pfu_root_verb() {
	local verb="${1:-}"
	[[ $# -gt 0 ]] && shift

	if [[ $EUID -ne 0 ]]; then
		pfu_bad "$(pfu_msg "This command has to run as root.")"
		return 2
	fi

	case "$verb" in
		set)
			local key="${1:-}" value="${2:-}"
			if [[ -z ${PFU_SYS_VALID[$key]+set} || ! $value =~ ${PFU_SYS_VALID[$key]} ]]; then
				pfu_bad "$(pfu_msg "Not a valid setting: %s=%s" "$key" "$value")"
				return 1
			fi
			pfu_kv_set "$PFU_SYSCONFIG" "$key" "$value" "$PFU_PRETTY (system settings)" || return 1
			chmod 0644 "$PFU_SYSCONFIG"
			;;
		pam-enable|pam-disable)
			local service="${1:-}" known=0 s
			for s in "${PFU_PAM_SERVICES[@]}"; do
				[[ $s == "$service" ]] && known=1
			done
			(( known )) || { pfu_bad "$(pfu_msg "Not a service this can be used for: %s" "$service")"; return 1; }
			if [[ $verb == pam-enable ]]; then
				pfu_pam_enable "$service"
			else
				pfu_pam_disable "$service"
			fi
			;;
		socket-enable)
			systemctl enable --now "$PFU_UNIT_SOCKET" > /dev/null 2>&1
			;;
		socket-disable)
			systemctl disable --now "$PFU_UNIT_SOCKET" > /dev/null 2>&1
			;;
		*)
			pfu_bad "$(pfu_msg "Unknown command: %s" "$verb")"
			return 1
			;;
	esac
}

# ---------------------------------------------------------------------------
# The two services
# ---------------------------------------------------------------------------

pfu_socket_enabled() {
	systemctl is-enabled --quiet "$PFU_UNIT_SOCKET" 2>/dev/null
}

pfu_agent_available() {
	pfu_have systemctl && [[ -n ${XDG_RUNTIME_DIR:-} ]]
}

pfu_agent_enabled() {
	systemctl --user is-enabled --quiet "$PFU_UNIT_AGENT" 2>/dev/null
}

pfu_agent_running() {
	systemctl --user is-active --quiet "$PFU_UNIT_AGENT" 2>/dev/null
}

pfu_agent_enable() {
	systemctl --user daemon-reload > /dev/null 2>&1 || true
	systemctl --user enable --now "$PFU_UNIT_AGENT" > /dev/null 2>&1
}

pfu_agent_disable() {
	systemctl --user disable --now "$PFU_UNIT_AGENT" > /dev/null 2>&1 || true
}
