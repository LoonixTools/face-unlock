# shellcheck shell=bash
#
# Putting face unlock in front of sudo and polkit's admin prompts, and taking
# it out again.
#
# Two services and nothing else. The lock screen does not go through PAM at all
# (see src/agent/lockcontroller.h), and the login screen stays with the
# password: logging in is what unlocks the wallet, and a face has no password
# to hand it.
#
# The line that goes in:
#
#   -auth  sufficient  /usr/lib/security/pam_plasma_face_unlock.so
#
# "sufficient": a match lets the person in, anything else falls through to the
# lines below as if this one were not there. The dash makes PAM skip it
# quietly if the module ever goes missing, say because the package was removed
# without turning this off first. sudo keeps working either way.
#
# Where the service's file is:
#   - /etc/pam.d/<service> exists: the line goes in before its first auth
#     line, with a comment saying where it came from.
#   - only the distribution's copy in /usr/lib/pam.d exists (Arch keeps
#     polkit-1 there): a small /etc/pam.d/<service> is written that puts the
#     line first and includes the distribution's file for everything else, so
#     an update to that file still counts.
# Undoing takes out exactly those lines, or that file.

PFU_PAM_MARK="# plasma-face-unlock: the face first, the password if that does not work"
PFU_PAM_WRAPPER_MARK="# Written by plasma-face-unlock."
PFU_PAM_SERVICES=(sudo polkit-1)

# Overridable for the tests only; the root side never takes them from the
# environment (see the top of plasma-face-unlock).
PFU_PAM_ETC_DIR="${PFU_PAM_ETC_DIR:-/etc/pam.d}"
read -r -a PFU_PAM_VENDOR_DIRS <<< "${PFU_PAM_VENDOR_DIRS:-/usr/lib/pam.d /usr/share/pam.d /lib/pam.d}"

pfu_pam_etc() {
	printf '%s/%s\n' "$PFU_PAM_ETC_DIR" "$1"
}

# pfu_pam_vendor <service>
# The distribution's own copy, when it keeps one outside /etc.
pfu_pam_vendor() {
	local dir
	for dir in "${PFU_PAM_VENDOR_DIRS[@]}"; do
		if [[ -f $dir/$1 ]]; then
			printf '%s\n' "$dir/$1"
			return 0
		fi
	done
	return 1
}

pfu_pam_line() {
	printf -- '-auth       sufficient   %s\n' "$PFU_PAM_MODULE"
}

# pfu_pam_enabled <service>
pfu_pam_enabled() {
	local file
	file="$(pfu_pam_etc "$1")"
	[[ -r $file ]] && grep -q 'pam_plasma_face_unlock\.so' "$file"
}

# pfu_pam_insert <file>
# Prints the file with the line added before its first auth line. Debian and
# Ubuntu have none in sudo's file, only "@include common-auth", and that
# counts as one: after it the password has already been asked for. A file
# with neither (which would be odd for either service) gets it at the end.
pfu_pam_insert() {
	awk -v mark="$PFU_PAM_MARK" -v line="$(pfu_pam_line)" '
		!done && ($0 ~ /^[[:space:]]*-?auth[[:space:]]/ || $0 ~ /^[[:space:]]*@include[[:space:]]+common-auth([[:space:]]|$)/) {
			print mark
			print line
			done = 1
		}
		{ print }
		END {
			if (!done) {
				print mark
				print line
			}
		}
	' "$1"
}

# pfu_pam_remove_lines <file>
# Prints the file without our comment and our line.
pfu_pam_remove_lines() {
	awk -v mark="$PFU_PAM_MARK" '
		$0 == mark { next }
		/pam_plasma_face_unlock\.so/ { next }
		{ print }
	' "$1"
}

pfu_pam_wrapper() {
	local vendor="$1"
	printf '#%%PAM-1.0\n'
	printf '%s The face first, then everything\n' "$PFU_PAM_WRAPPER_MARK"
	printf '# %s does for this service. Removed again by\n' "$vendor"
	printf '# "plasma-face-unlock disable" or from its settings.\n\n'
	pfu_pam_line
	printf 'auth       include      %s\n' "$vendor"
	printf 'account    include      %s\n' "$vendor"
	printf 'password   include      %s\n' "$vendor"
	printf 'session    include      %s\n' "$vendor"
}

# _pfu_pam_replace <file> <content>
# Writes the new file next to the old one and swaps it in, with the old one's
# owner and mode. A half-written PAM file is a locked-out machine.
_pfu_pam_replace() {
	local file="$1" content="$2" tmp
	tmp="$(mktemp "${file}.pfu.XXXXXX")" || return 1
	printf '%s\n' "$content" > "$tmp" || { rm -f "$tmp"; return 1; }
	if [[ -e $file ]]; then
		chown --reference="$file" "$tmp" 2>/dev/null
		chmod --reference="$file" "$tmp" 2>/dev/null
	else
		chmod 0644 "$tmp"
	fi
	mv -f "$tmp" "$file"
}

# pfu_pam_enable <service>     (root)
pfu_pam_enable() {
	local service="$1" file vendor content
	file="$(pfu_pam_etc "$service")"

	[[ -e $PFU_PAM_MODULE ]] || { pfu_bad "$(pfu_msg "The PAM module is not installed at %s." "$PFU_PAM_MODULE")"; return 1; }
	pfu_pam_enabled "$service" && return 0

	if [[ -f $file ]]; then
		content="$(pfu_pam_insert "$file")" || return 1
	elif vendor="$(pfu_pam_vendor "$service")"; then
		content="$(pfu_pam_wrapper "$vendor")"
	else
		pfu_bad "$(pfu_msg "There is no PAM configuration for %s on this system." "$service")"
		return 1
	fi

	_pfu_pam_replace "$file" "$content"
}

# pfu_pam_disable <service>    (root)
pfu_pam_disable() {
	local service="$1" file content
	file="$(pfu_pam_etc "$service")"

	pfu_pam_enabled "$service" || return 0

	if head -n 3 "$file" | grep -qF "$PFU_PAM_WRAPPER_MARK"; then
		# Ours from the first line to the last. Without it the distribution's
		# own copy is in charge again.
		rm -f -- "$file"
		return
	fi

	content="$(pfu_pam_remove_lines "$file")" || return 1
	_pfu_pam_replace "$file" "$content"
}
