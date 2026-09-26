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
#   --root pam-enable <service>  sudo, polkit-1, a lock screen, or lockscreens
#   --root pam-disable <service>  for all lock screens installed; see pam.sh
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
			local service="${1:-}" known=0 s rc=0
			local -a services=("$service")
			for s in "${FU_PAM_SERVICES[@]}" "${FU_PAM_LOCKERS[@]}"; do
				[[ $s == "$service" ]] && known=1
			done
			if [[ $service == lockscreens ]]; then
				known=1
				mapfile -t services < <(fu_pam_lockers)
			fi
			(( known )) || { fu_bad "$(fu_msg "Not a service this can be used for: %s" "$service")"; return 1; }
			for s in "${services[@]}"; do
				if [[ $verb == pam-enable ]]; then
					fu_pam_enable "$s" || rc=1
				else
					fu_pam_disable "$s" || rc=1
				fi
			done
			return $rc
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
# What to add to Hyprland's config, in the Lua it has since 0.56 or in the
# older format.
fu_agent_hint() {
	# shellcheck disable=SC2088  # shown to the user, not a path to open
	if [[ -f $FU_XDG_CONFIG/hypr/hyprland.lua ]]; then
		fu_note "$(fu_msg "Hyprland does not start the lock screen agent by itself. Add this to %s:" "~/.config/hypr/hyprland.lua")"
		fu_say "    hl.on(\"hyprland.start\", function ()"
		fu_say "      hl.exec_cmd(\"systemctl --user start $FU_UNIT_AGENT\")"
		fu_say "    end)"
	else
		fu_note "$(fu_msg "Hyprland does not start the lock screen agent by itself. Add this to %s:" "~/.config/hypr/hyprland.conf")"
		fu_say "    exec-once = systemctl --user start $FU_UNIT_AGENT"
	fi
}

# ---------------------------------------------------------------------------
# The bubble's GNOME Shell extension
# ---------------------------------------------------------------------------
# GNOME lets no program draw above its windows; its extensions may. The
# package brings one, and turning face unlock on switches it on.

FU_GNOME_EXTENSION="face-unlock@loonixtools.github.io"

# fu_gnome_extension_enable
# gnome-extensions only knows the extensions GNOME found when the session
# started. One that came with the package later goes straight into the
# setting, and GNOME loads it at the next login: returns 2 then.
fu_gnome_extension_enable() {
	local list new
	fu_have gsettings || return 1
	if gnome-extensions info "$FU_GNOME_EXTENSION" > /dev/null 2>&1; then
		gnome-extensions enable "$FU_GNOME_EXTENSION" > /dev/null 2>&1 && return 0
	fi
	list="$(gsettings get org.gnome.shell enabled-extensions 2>/dev/null)" || return 1
	case "$list" in
		*"'$FU_GNOME_EXTENSION'"*) return 2 ;;
		'@as []'|'[]') new="['$FU_GNOME_EXTENSION']" ;;
		*) new="${list%]}, '$FU_GNOME_EXTENSION']" ;;
	esac
	gsettings set org.gnome.shell enabled-extensions "$new" || return 1
	return 2
}

fu_gnome_extension_disable() {
	local list
	fu_have gsettings || return 0
	gnome-extensions disable "$FU_GNOME_EXTENSION" > /dev/null 2>&1 && return 0
	list="$(gsettings get org.gnome.shell enabled-extensions 2>/dev/null)" || return 0
	list="${list//, \'$FU_GNOME_EXTENSION\'/}"
	list="${list//\'$FU_GNOME_EXTENSION\', /}"
	list="${list//\'$FU_GNOME_EXTENSION\'/}"
	[[ $list == '[]' ]] && list='@as []'
	gsettings set org.gnome.shell enabled-extensions "$list" > /dev/null 2>&1 || true
}

# ---------------------------------------------------------------------------
# The lock screen where it is a program of its own (Hyprland, Niri)
# ---------------------------------------------------------------------------
# Either face-unlock's (`face-unlock lock`, with the bubble), or the user's
# own program. hyprlock then shows what the bubble would as a line of text: a
# label in its config reads a file the agent writes (see
# src/agent/locktext.h). Asked once in a window, and under Settings.

FU_HYPRLOCK_SNIPPET="$FU_XDG_CONFIG/$FU_NAME/hyprlock.conf"
FU_HYPRLOCK_MARK="# $FU_NAME: what face unlock is doing, at the top"

fu_own_lock_here() {
	[[ $FU_DESKTOP != plasma && $FU_DESKTOP != gnome ]]
}

# fu_hyprlock_text_on
# 0 done, 1 could not write, 2 no hyprlock, 3 hyprlock has no config here.
fu_hyprlock_text_on() {
	local conf="$FU_XDG_CONFIG/hypr/hyprlock.conf"
	fu_have hyprlock || return 2
	[[ -f $conf ]] || return 3
	mkdir -p "${FU_HYPRLOCK_SNIPPET%/*}" || return 1
	cat > "$FU_HYPRLOCK_SNIPPET" <<- EOF || return 1
		# Written by $FU_NAME: what face unlock is doing, as a line at the top of
		# hyprlock. Picking $FU_NAME's lock screen in its menu takes it out again.
		label {
		    monitor =
		    text = cmd[update:0:1] cat "\$XDG_RUNTIME_DIR/$FU_NAME/lock-text" 2>/dev/null
		    color = rgba(255, 255, 255, 0.95)
		    font_size = 20
		    shadow_passes = 2
		    shadow_size = 3
		    halign = center
		    valign = top
		    position = 0, -48
		    zindex = 10
		}
	EOF
	grep -qxF "$FU_HYPRLOCK_MARK" "$conf" && return 0
	# With ~ where it can, like a hand-written config. hyprlock expands it.
	local snippet="$FU_HYPRLOCK_SNIPPET"
	# shellcheck disable=SC2088  # for hyprlock, not for the shell
	[[ $snippet == "$HOME"/* ]] && snippet="~/${snippet#"$HOME"/}"
	printf '\n%s\nsource = %s\n' "$FU_HYPRLOCK_MARK" "$snippet" >> "$conf" || return 1
}

# fu_hyprlock_text_off
# The mark, the source line under it, and the empty lines before it. Written
# back in place, so a hyprlock.conf that is a link to a dotfile stays one.
fu_hyprlock_text_off() {
	local conf="$FU_XDG_CONFIG/hypr/hyprlock.conf" content
	rm -f "$FU_HYPRLOCK_SNIPPET"
	[[ -f $conf ]] && grep -qxF "$FU_HYPRLOCK_MARK" "$conf" || return 0
	content="$(awk -v mark="$FU_HYPRLOCK_MARK" '
		$0 == mark { held = ""; skip = 1; next }
		skip && /^[[:space:]]*source[[:space:]]*=/ { skip = 0; next }
		{ skip = 0 }
		/^[[:space:]]*$/ { held = held $0 "\n"; next }
		{ printf "%s", held; held = ""; print }
	' "$conf")" || return 1
	printf '%s\n' "$content" > "$conf"
}

# fu_own_lock_hint
# Where face-unlock's lock screen has to be started from: the user's key and
# idle lock, which are theirs to change.
fu_own_lock_hint() {
	# shellcheck disable=SC2088  # shown to the user, not a path to open
	case "$FU_DESKTOP" in
		hyprland)
			if [[ -f $FU_XDG_CONFIG/hypr/hyprland.lua ]]; then
				fu_note "$(fu_msg "To lock with it, add this to %s, and set %s in %s:" "~/.config/hypr/hyprland.lua" "lock_cmd = $FU_NAME lock" "hypridle.conf")"
				fu_code "hl.bind(\"SUPER + L\", hl.dsp.exec_cmd(\"$FU_NAME lock\"))"
			else
				fu_note "$(fu_msg "To lock with it, add this to %s, and set %s in %s:" "~/.config/hypr/hyprland.conf" "lock_cmd = $FU_NAME lock" "hypridle.conf")"
				fu_code "bind = SUPER, L, exec, $FU_NAME lock"
			fi
			;;
		niri)
			fu_note "$(fu_msg "To lock with it, add this to the binds in %s, and use %s in swayidle:" "~/.config/niri/config.kdl" "$FU_NAME lock")"
			fu_code "Mod+Alt+L { spawn \"$FU_NAME\" \"lock\"; }"
			;;
		*)
			fu_note "$(fu_msg "To lock with it, run %s where your lock screen is started now." "$FU_NAME lock")"
			;;
	esac
}

# fu_lock_style_set <own|yours>
fu_lock_style_set() {
	# shellcheck disable=SC2088  # shown to the user, not a path to open
	local conf="~/.config/hypr/hyprlock.conf"
	case "$1" in
		own)
			fu_config_set LockScreenStyle own || { fu_bad "$(fu_msg "Could not save the setting.")"; return 1; }
			fu_hyprlock_text_off
			fu_ok "$(fu_msg "You lock with face-unlock's lock screen now.")"
			fu_own_lock_hint
			;;
		yours)
			fu_config_set LockScreenStyle yours || { fu_bad "$(fu_msg "Could not save the setting.")"; return 1; }
			fu_hyprlock_text_on
			case $? in
				0) fu_ok "$(fu_msg "hyprlock shows what face unlock is doing at the top, from the next time it locks.")" ;;
				1) fu_bad "$(fu_msg "Could not add the line to %s." "$conf")" ;;
				2) fu_ok "$(fu_msg "Your lock screen stays as it is. Press Enter on the empty password field to scan.")" ;;
				3) fu_note "$(fu_msg "hyprlock has no %s, so it cannot show the text." "$conf")" ;;
			esac
			;;
		*) return 1 ;;
	esac
}

# fu_lock_choose
# Asks in a window which lock screen to use, and carries it out. 1 when the
# window was closed or cannot open.
fu_lock_choose() {
	local choice
	[[ -n ${WAYLAND_DISPLAY:-} ]] || return 1
	choice="$("$FU_AGENT" --choose-lock-screen 2> /dev/null)" || return 1
	fu_lock_style_set "$choice"
}

fu_agent_disable() {
	systemctl --user disable --now "$FU_UNIT_AGENT" > /dev/null 2>&1 || true
}
