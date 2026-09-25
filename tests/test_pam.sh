#!/usr/bin/env bash
#
# The PAM file editing, against copies of what the distributions ship.
#
# Turning face unlock on for sudo edits /etc/pam.d/sudo, and a mistake there
# is a machine nobody can sudo on any more. So every layout this has to cope
# with is here: where the line lands, that it lands once, and that turning it
# off gives back the file exactly as it was.
#
# When the pam_harness from the build is there, the files that come out are
# also run through real PAM, with the module missing on purpose: the dash in
# front of the line has to make PAM skip it without a word.
#
# Last, moving over from plasma-face-unlock: its PAM lines, settings and faces.

# check() runs its second argument with eval, so that is quoted on purpose.
# shellcheck disable=SC2016

set -uo pipefail

here="$(cd -- "$(dirname -- "${BASH_SOURCE[0]}")/.." && pwd)"
tmp="$(mktemp -d)"
trap 'rm -rf -- "$tmp"' EXIT

export FU_PAM_ETC_DIR="$tmp/etc"
export FU_PAM_VENDOR_DIRS="$tmp/vendor"
FU_LIBDIR="$here/src/lib"
# shellcheck source=/dev/null
source "$FU_LIBDIR/common.sh"
# shellcheck source=/dev/null
source "$FU_LIBDIR/config.sh"
# shellcheck source=/dev/null
source "$FU_LIBDIR/pam.sh"
# shellcheck source=/dev/null
source "$FU_LIBDIR/system.sh"
# shellcheck source=/dev/null
source "$FU_LIBDIR/migrate.sh"

FU_PAM_MODULE="$tmp/lib/pam_face_unlock.so"
mkdir -p "$tmp/lib" "$tmp/etc" "$tmp/vendor"
: > "$FU_PAM_MODULE"

failures=0
check() {
	if eval "$2"; then
		printf 'ok    %s\n' "$1"
	else
		printf 'FAIL  %s\n' "$1"
		failures=$(( failures + 1 ))
	fi
}

# first_auth <file>: the first line that has anything to do with auth.
first_auth() {
	grep -m1 -E '^[[:space:]]*(-?auth[[:space:]]|@include[[:space:]]+common-auth)' "$1"
}

# ---------------------------------------------------------------------------
# The layouts
# ---------------------------------------------------------------------------

arch_sudo='#%PAM-1.0
auth		include		system-auth
account		include		system-auth
session		include		system-auth
session		optional	pam_systemd.so class=none'

debian_sudo='#%PAM-1.0

# Set up user limits from /etc/security/limits.conf.
session    required   pam_limits.so

session    required   pam_env.so readenv=1 user_readenv=0
session    required   pam_env.so readenv=1 envfile=/etc/default/locale user_readenv=0
@include common-auth
@include common-account
@include common-session-noninteractive'

fedora_sudo='#%PAM-1.0
auth       include      system-auth
account    include      system-auth
password   include      system-auth
session    optional     pam_keyinit.so revoke
session    required     pam_limits.so
session    include      system-auth'

arch_polkit='#%PAM-1.0

auth       include      system-auth
account    include      system-auth
password   include      system-auth
session    include      system-auth'

for layout in arch_sudo debian_sudo fedora_sudo; do
	printf '%s\n' "${!layout}" > "$tmp/etc/sudo"
	cp "$tmp/etc/sudo" "$tmp/original"

	fu_pam_enable sudo
	check "$layout: turned on" 'fu_pam_enabled sudo'
	check "$layout: the face comes before the password" '[[ "$(first_auth "$tmp/etc/sudo")" == *pam_face_unlock.so* ]]'
	check "$layout: with the dash and as sufficient" 'grep -qE "^-auth[[:space:]]+sufficient[[:space:]]+$FU_PAM_MODULE\$" "$tmp/etc/sudo"'
	check "$layout: the header stays first" '[[ "$(head -n1 "$tmp/etc/sudo")" == "#%PAM-1.0" ]]'

	fu_pam_enable sudo
	check "$layout: turning it on twice adds it once" '[[ $(grep -c pam_face_unlock "$tmp/etc/sudo") -eq 1 ]]'

	fu_pam_disable sudo
	check "$layout: turning it off gives back the same file" 'cmp -s "$tmp/etc/sudo" "$tmp/original"'
	check "$layout: turned off" '! fu_pam_enabled sudo'
	rm -f "$tmp/etc/sudo"
done

# Only the distribution's copy, in /usr/lib/pam.d.
printf '%s\n' "$arch_polkit" > "$tmp/vendor/polkit-1"
fu_pam_enable polkit-1
check "vendor polkit-1: a small file of our own in /etc" '[[ -f $tmp/etc/polkit-1 ]] && grep -qF "$FU_PAM_WRAPPER_MARK" "$tmp/etc/polkit-1"'
check "vendor polkit-1: it includes the distribution's file" 'grep -qE "^auth[[:space:]]+include[[:space:]]+$tmp/vendor/polkit-1\$" "$tmp/etc/polkit-1"'
check "vendor polkit-1: face first" '[[ "$(first_auth "$tmp/etc/polkit-1")" == *pam_face_unlock.so* ]]'
check "vendor polkit-1: the distribution's file is left alone" '[[ "$(cat "$tmp/vendor/polkit-1")" == "$arch_polkit" ]]'
fu_pam_disable polkit-1
check "vendor polkit-1: turning it off removes our file" '[[ ! -e $tmp/etc/polkit-1 ]]'

# No configuration at all.
check "an unknown service is refused" '! fu_pam_enable nosuchservice 2>/dev/null'

# ---------------------------------------------------------------------------
# Moving over from plasma-face-unlock
# ---------------------------------------------------------------------------

FU_OLD_SYSDIR="$tmp/old-etc"
FU_OLD_STATEDIR="$tmp/old-state"
FU_STATEDIR="$tmp/state"
FU_SYSCONFIG="$tmp/new-etc/config"
FU_SYSTEMD_ETC="$tmp/systemd"
old_module=/usr/lib/security/pam_plasma_face_unlock.so

# What plasma-face-unlock wrote is what this writes, under the old name.
make_old() {
	sed -i -e "s|^$FU_PAM_MARK\$|$FU_OLD_PAM_MARK|" -e "s|$FU_PAM_WRAPPER_MARK|$FU_OLD_PAM_WRAPPER_MARK|" \
		-e "s|$FU_PAM_MODULE|$old_module|" "$1"
}

for layout in arch_sudo debian_sudo fedora_sudo; do
	printf '%s\n' "${!layout}" > "$tmp/etc/sudo"
	cp "$tmp/etc/sudo" "$tmp/original"
	fu_pam_enable sudo
	make_old "$tmp/etc/sudo"
	check "$layout: the old line is found" '_fu_pam_has_old sudo && ! fu_pam_enabled sudo && fu_migrate_pending'

	fu_migrate_system
	check "$layout: moved over to the new line" 'fu_pam_enabled sudo && ! _fu_pam_has_old sudo'
	check "$layout: the old comment is gone" '! grep -qF "$FU_OLD_PAM_MARK" "$tmp/etc/sudo"'
	check "$layout: nothing is left over" '! fu_migrate_pending'

	fu_pam_disable sudo
	check "$layout: off again gives back the file from before" 'cmp -s "$tmp/etc/sudo" "$tmp/original"'
	rm -f "$tmp/etc/sudo"
done

fu_pam_enable polkit-1
make_old "$tmp/etc/polkit-1"
fu_migrate_system
check "vendor polkit-1: moved over to a file of the new name" 'grep -qF "$FU_PAM_WRAPPER_MARK" "$tmp/etc/polkit-1" && ! grep -q plasma "$tmp/etc/polkit-1"'
fu_pam_disable polkit-1

mkdir -p "$FU_OLD_SYSDIR" "$FU_OLD_STATEDIR/users"
echo 'Liveness=heavy' > "$FU_OLD_SYSDIR/config"
echo '{}' > "$FU_OLD_STATEDIR/users/1000.json"
check "old settings and faces are found" 'fu_migrate_pending'
fu_migrate_system
check "the system settings moved" '[[ $(cat "$FU_SYSCONFIG") == Liveness=heavy && ! -e $FU_OLD_SYSDIR ]]'
check "the faces moved" '[[ $(cat "$FU_STATEDIR/users/1000.json") == "{}" && ! -e $FU_OLD_STATEDIR ]]'

mkdir -p "$FU_OLD_STATEDIR/users"
echo old > "$FU_OLD_STATEDIR/users/1000.json"
fu_migrate_system
check "faces set up under the new name win" '[[ $(cat "$FU_STATEDIR/users/1000.json") == "{}" && ! -e $FU_OLD_STATEDIR ]]'
check "nothing is left over at the end" '! fu_migrate_pending'

# This user's side, with systemctl only written down.
# shellcheck disable=SC2329  # called by fu_migrate_user
systemctl() { printf '%s\n' "$*" >> "$tmp/systemctl"; }
FU_XDG_CONFIG="$tmp/home-config"
FU_CONFDIR="$FU_XDG_CONFIG/face-unlock"
mkdir -p "$FU_XDG_CONFIG/plasma-face-unlock" "$FU_XDG_CONFIG/systemd/user/graphical-session.target.wants"
echo 'Enabled=yes' > "$FU_XDG_CONFIG/plasma-face-unlock/config"
ln -s /nowhere "$FU_XDG_CONFIG/systemd/user/graphical-session.target.wants/plasma-face-unlock-agent.service"
XDG_RUNTIME_DIR="${XDG_RUNTIME_DIR:-$tmp}" fu_migrate_user
check "user settings moved" '[[ $(cat "$FU_CONFDIR/config") == Enabled=yes && ! -e $FU_XDG_CONFIG/plasma-face-unlock ]]'
check "the old agent is off and the new one on" 'grep -q "disable --now plasma-face-unlock-agent" "$tmp/systemctl" && grep -q "enable --now face-unlock-agent" "$tmp/systemctl"'
unset -f systemctl

# ---------------------------------------------------------------------------
# Through real PAM
# ---------------------------------------------------------------------------

harness="$here/build/pam_harness"
if [[ -x $harness ]]; then
	mkdir -p "$tmp/real/vendor" "$tmp/real/etc"
	printf 'auth required pam_permit.so\naccount required pam_permit.so\n' > "$tmp/real/vendor/svc"
	FU_PAM_ETC_DIR="$tmp/real/etc"
	FU_PAM_VENDOR_DIRS=("$tmp/real/vendor")
	FU_PAM_MODULE="$tmp/real/missing/pam_face_unlock.so"
	mkdir -p "$tmp/real/missing" && : > "$FU_PAM_MODULE"
	fu_pam_enable svc
	rm -f "$FU_PAM_MODULE"
	check "a missing module is skipped, the rest of the stack still decides" '"$harness" "$tmp/real/etc" svc "$(id -un)" > /dev/null 2>&1'

	printf 'auth required pam_deny.so\n' > "$tmp/real/vendor/svc"
	check "and a stack that says no still says no" '! "$harness" "$tmp/real/etc" svc "$(id -un)" > /dev/null 2>&1'
else
	printf 'skip  real PAM checks (build pam_harness first)\n'
fi

printf '\n%s\n' "$( (( failures )) && echo "SOME CHECKS FAILED" || echo "all checks passed")"
(( failures == 0 ))
