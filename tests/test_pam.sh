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

set -uo pipefail

here="$(cd -- "$(dirname -- "${BASH_SOURCE[0]}")/.." && pwd)"
tmp="$(mktemp -d)"
trap 'rm -rf -- "$tmp"' EXIT

export PFU_PAM_ETC_DIR="$tmp/etc"
export PFU_PAM_VENDOR_DIRS="$tmp/vendor"
PFU_LIBDIR="$here/src/lib"
# shellcheck source=/dev/null
source "$PFU_LIBDIR/common.sh"
# shellcheck source=/dev/null
source "$PFU_LIBDIR/config.sh"
# shellcheck source=/dev/null
source "$PFU_LIBDIR/pam.sh"

PFU_PAM_MODULE="$tmp/lib/pam_plasma_face_unlock.so"
mkdir -p "$tmp/lib" "$tmp/etc" "$tmp/vendor"
: > "$PFU_PAM_MODULE"

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

	pfu_pam_enable sudo
	check "$layout: turned on" 'pfu_pam_enabled sudo'
	check "$layout: the face comes before the password" '[[ "$(first_auth "$tmp/etc/sudo")" == *pam_plasma_face_unlock.so* ]]'
	check "$layout: with the dash and as sufficient" 'grep -qE "^-auth[[:space:]]+sufficient[[:space:]]+$PFU_PAM_MODULE\$" "$tmp/etc/sudo"'
	check "$layout: the header stays first" '[[ "$(head -n1 "$tmp/etc/sudo")" == "#%PAM-1.0" ]]'

	pfu_pam_enable sudo
	check "$layout: turning it on twice adds it once" '[[ $(grep -c pam_plasma_face_unlock "$tmp/etc/sudo") -eq 1 ]]'

	pfu_pam_disable sudo
	check "$layout: turning it off gives back the same file" 'cmp -s "$tmp/etc/sudo" "$tmp/original"'
	check "$layout: turned off" '! pfu_pam_enabled sudo'
	rm -f "$tmp/etc/sudo"
done

# Only the distribution's copy, in /usr/lib/pam.d.
printf '%s\n' "$arch_polkit" > "$tmp/vendor/polkit-1"
pfu_pam_enable polkit-1
check "vendor polkit-1: a small file of our own in /etc" '[[ -f $tmp/etc/polkit-1 ]] && grep -qF "$PFU_PAM_WRAPPER_MARK" "$tmp/etc/polkit-1"'
check "vendor polkit-1: it includes the distribution's file" 'grep -qE "^auth[[:space:]]+include[[:space:]]+$tmp/vendor/polkit-1\$" "$tmp/etc/polkit-1"'
check "vendor polkit-1: face first" '[[ "$(first_auth "$tmp/etc/polkit-1")" == *pam_plasma_face_unlock.so* ]]'
check "vendor polkit-1: the distribution's file is left alone" '[[ "$(cat "$tmp/vendor/polkit-1")" == "$arch_polkit" ]]'
pfu_pam_disable polkit-1
check "vendor polkit-1: turning it off removes our file" '[[ ! -e $tmp/etc/polkit-1 ]]'

# No configuration at all.
check "an unknown service is refused" '! pfu_pam_enable nosuchservice 2>/dev/null'

# ---------------------------------------------------------------------------
# Through real PAM
# ---------------------------------------------------------------------------

harness="$here/build/pam_harness"
if [[ -x $harness ]]; then
	mkdir -p "$tmp/real/vendor" "$tmp/real/etc"
	printf 'auth required pam_permit.so\naccount required pam_permit.so\n' > "$tmp/real/vendor/svc"
	PFU_PAM_ETC_DIR="$tmp/real/etc"
	PFU_PAM_VENDOR_DIRS=("$tmp/real/vendor")
	PFU_PAM_MODULE="$tmp/real/missing/pam_plasma_face_unlock.so"
	mkdir -p "$tmp/real/missing" && : > "$PFU_PAM_MODULE"
	pfu_pam_enable svc
	rm -f "$PFU_PAM_MODULE"
	check "a missing module is skipped, the rest of the stack still decides" '"$harness" "$tmp/real/etc" svc "$(id -un)" > /dev/null 2>&1'

	printf 'auth required pam_deny.so\n' > "$tmp/real/vendor/svc"
	check "and a stack that says no still says no" '! "$harness" "$tmp/real/etc" svc "$(id -un)" > /dev/null 2>&1'
else
	printf 'skip  real PAM checks (build pam_harness first)\n'
fi

printf '\n%s\n' "$( (( failures )) && echo "SOME CHECKS FAILED" || echo "all checks passed")"
(( failures == 0 ))
