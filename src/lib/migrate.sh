# shellcheck shell=bash
#
# Moving over from plasma-face-unlock, the old name.
#
# The packages run `face-unlock --root migrate` when they are installed. The
# command runs it too (through sudo) when it finds something left over, for
# anybody who built it from source. This user's own settings need no root and
# are moved every time the command starts.

FU_OLD_NAME="plasma-face-unlock"
FU_OLD_PAM_MARK="# plasma-face-unlock: the face first, the password if that does not work"
FU_OLD_PAM_WRAPPER_MARK="# Written by plasma-face-unlock."

# Overridable for the tests only, like the PAM directories.
FU_OLD_SYSDIR="${FU_OLD_SYSDIR:-/etc/$FU_OLD_NAME}"
FU_OLD_STATEDIR="${FU_OLD_STATEDIR:-/var/lib/$FU_OLD_NAME}"
FU_STATEDIR="${FU_STATEDIR:-/var/lib/$FU_NAME}"
FU_SYSTEMD_ETC="${FU_SYSTEMD_ETC:-/etc/systemd/system}"

_fu_old_socket_link() {
	printf '%s/sockets.target.wants/%sd.socket\n' "$FU_SYSTEMD_ETC" "$FU_OLD_NAME"
}

# _fu_pam_has_old <service>
_fu_pam_has_old() {
	local file
	file="$(fu_pam_etc "$1")"
	[[ -r $file ]] && grep -q 'pam_plasma_face_unlock\.so' "$file"
}

# _fu_pam_drop_old <service>    (root)
_fu_pam_drop_old() {
	local file content
	file="$(fu_pam_etc "$1")"
	if head -n 3 "$file" | grep -qF "$FU_OLD_PAM_WRAPPER_MARK"; then
		rm -f -- "$file"
		return
	fi
	content="$(awk -v mark="$FU_OLD_PAM_MARK" '
		$0 == mark { next }
		/pam_plasma_face_unlock\.so/ { next }
		{ print }
	' "$file")" || return 1
	_fu_pam_replace "$file" "$content"
}

# fu_migrate_system    (root)
# The system settings, the faces, sudo and admin prompts and the daemon's
# socket, as plasma-face-unlock left them.
fu_migrate_system() {
	local service link

	if [[ -d $FU_OLD_SYSDIR ]]; then
		if [[ -f $FU_OLD_SYSDIR/config && ! -e $FU_SYSCONFIG ]]; then
			install -D -m 0644 "$FU_OLD_SYSDIR/config" "$FU_SYSCONFIG" || return 1
		fi
		rm -rf -- "$FU_OLD_SYSDIR"
	fi

	if [[ -d $FU_OLD_STATEDIR ]]; then
		# Faces already set up under the new name win.
		if [[ -d $FU_OLD_STATEDIR/users ]] && ! compgen -G "$FU_STATEDIR/users/*" > /dev/null; then
			install -d -m 0700 "$FU_STATEDIR" || return 1
			rm -rf -- "$FU_STATEDIR/users"
			mv -- "$FU_OLD_STATEDIR/users" "$FU_STATEDIR/users" || return 1
		fi
		rm -rf -- "$FU_OLD_STATEDIR"
	fi

	for service in "${FU_PAM_SERVICES[@]}"; do
		_fu_pam_has_old "$service" || continue
		_fu_pam_drop_old "$service" && fu_pam_enable "$service"
	done

	link="$(_fu_old_socket_link)"
	if [[ -L $link ]]; then
		systemctl disable --now "${FU_OLD_NAME}d.socket" > /dev/null 2>&1 || true
		rm -f -- "$link"
		systemctl enable --now "$FU_UNIT_SOCKET" > /dev/null 2>&1 || true
	fi
	return 0
}

# fu_migrate_pending
# Whether something needs fu_migrate_system.
fu_migrate_pending() {
	local service
	[[ -d $FU_OLD_SYSDIR || -d $FU_OLD_STATEDIR || -L $(_fu_old_socket_link) ]] && return 0
	for service in "${FU_PAM_SERVICES[@]}"; do
		_fu_pam_has_old "$service" && return 0
	done
	return 1
}

# fu_migrate_user
# This user's settings and the agent's service.
fu_migrate_user() {
	local old="$FU_XDG_CONFIG/$FU_OLD_NAME" link
	if [[ -d $old ]]; then
		if [[ -e $FU_CONFDIR ]]; then
			rm -rf -- "$old"
		else
			mv -- "$old" "$FU_CONFDIR"
		fi
	fi
	link="$FU_XDG_CONFIG/systemd/user/graphical-session.target.wants/$FU_OLD_NAME-agent.service"
	if [[ -L $link ]]; then
		systemctl --user disable --now "$FU_OLD_NAME-agent.service" > /dev/null 2>&1 || true
		rm -f -- "$link"
		fu_agent_available && fu_agent_enable
	fi
	return 0
}

# fu_migrate
fu_migrate() {
	fu_migrate_user
	fu_migrate_pending || return 0
	fu_say "  $(fu_msg "Moving your faces and settings over from plasma-face-unlock. That needs your password.")"
	fu_root migrate
}
