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
#   --root set <Key> <Value>     one line of /etc/face-unlock/config
#   --root pam-enable <service>  sudo or polkit-1, see pam.sh
#   --root pam-disable <service>
#   --root socket-enable         start the daemon's socket, and at boot
#   --root socket-disable
#   --root migrate               move over from plasma-face-unlock, see migrate.sh

FU_SELF="${FU_SELF:-$(readlink -f "${BASH_SOURCE[1]:-$0}")}"

# What each system setting may be set to. Anything else is refused on the root
# side, whatever the menu sent.
declare -A FU_SYS_VALID=(
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

# fu_root <verb> [args...]
fu_root() {
	local candidate

	if [[ $EUID -eq 0 ]]; then
		fu_root_verb "$@"
		return
	fi
	for candidate in sudo run0 doas; do
		if fu_have "$candidate"; then
			"$candidate" "$FU_SELF" --root "$@"
			return
		fi
	done
	fu_bad "$(fu_msg "This needs root, and neither sudo, run0 nor doas is installed.")"
	return 1
}

fu_root_verb() {
	local verb="${1:-}"
	[[ $# -gt 0 ]] && shift

	if [[ $EUID -ne 0 ]]; then
		fu_bad "$(fu_msg "This command has to run as root.")"
		return 2
	fi

	case "$verb" in
		set)
			local key="${1:-}" value="${2:-}"
			if [[ -z ${FU_SYS_VALID[$key]+set} || ! $value =~ ${FU_SYS_VALID[$key]} ]]; then
				fu_bad "$(fu_msg "Not a valid setting: %s=%s" "$key" "$value")"
				return 1
			fi
			fu_kv_set "$FU_SYSCONFIG" "$key" "$value" "$FU_PRETTY (system settings)" || return 1
			chmod 0644 "$FU_SYSCONFIG"
			;;
		pam-enable|pam-disable)
			local service="${1:-}" known=0 s
			for s in "${FU_PAM_SERVICES[@]}"; do
				[[ $s == "$service" ]] && known=1
			done
			(( known )) || { fu_bad "$(fu_msg "Not a service this can be used for: %s" "$service")"; return 1; }
			if [[ $verb == pam-enable ]]; then
				fu_pam_enable "$service"
			else
				fu_pam_disable "$service"
			fi
			;;
		socket-enable)
			systemctl enable --now "$FU_UNIT_SOCKET" > /dev/null 2>&1
			;;
		socket-disable)
			systemctl disable --now "$FU_UNIT_SOCKET" > /dev/null 2>&1
			;;
		migrate)
			fu_migrate_system
			;;
		*)
			fu_bad "$(fu_msg "Unknown command: %s" "$verb")"
			return 1
			;;
	esac
}

# ---------------------------------------------------------------------------
# The two services
# ---------------------------------------------------------------------------

fu_socket_enabled() {
	systemctl is-enabled --quiet "$FU_UNIT_SOCKET" 2>/dev/null
}

fu_agent_available() {
	fu_have systemctl && [[ -n ${XDG_RUNTIME_DIR:-} ]]
}

fu_agent_enabled() {
	systemctl --user is-enabled --quiet "$FU_UNIT_AGENT" 2>/dev/null
}

fu_agent_running() {
	systemctl --user is-active --quiet "$FU_UNIT_AGENT" 2>/dev/null
}

fu_agent_enable() {
	systemctl --user daemon-reload > /dev/null 2>&1 || true
	systemctl --user enable --now "$FU_UNIT_AGENT" > /dev/null 2>&1
}

# fu_agent_autostarts
# Whether the agent's service starts with the session. It is wanted by
# graphical-session.target, which Hyprland only reaches when it runs under
# uwsm. Plasma, GNOME and niri-session always do.
fu_agent_autostarts() {
	[[ $FU_DESKTOP != hyprland ]] || systemctl --user is-active --quiet graphical-session.target
}

# fu_agent_hint
fu_agent_hint() {
	# shellcheck disable=SC2088  # shown to the user, not a path to open
	fu_note "$(fu_msg "Hyprland does not start the lock screen agent by itself. Add this line to %s:" "~/.config/hypr/hyprland.conf")"
	fu_say "    exec-once = systemctl --user start $FU_UNIT_AGENT"
}

fu_agent_disable() {
	systemctl --user disable --now "$FU_UNIT_AGENT" > /dev/null 2>&1 || true
}
