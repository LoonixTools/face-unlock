//! Checking the password on face-unlock's own lock screen, through PAM.

use std::ffi::{CStr, CString, c_char, c_int, c_void};

#[repr(C)]
struct PamMessage {
    msg_style: c_int,
    msg: *const c_char,
}

#[repr(C)]
struct PamResponse {
    resp: *mut c_char,
    resp_retcode: c_int,
}

type ConvFn = extern "C" fn(c_int, *mut *const PamMessage, *mut *mut PamResponse, *mut c_void) -> c_int;

#[repr(C)]
struct PamConv {
    conv: ConvFn,
    appdata_ptr: *mut c_void,
}

#[link(name = "pam")]
unsafe extern "C" {
    fn pam_start(service: *const c_char, user: *const c_char, conv: *const PamConv, pamh: *mut *mut c_void) -> c_int;
    fn pam_start_confdir(service: *const c_char, user: *const c_char, conv: *const PamConv, confdir: *const c_char, pamh: *mut *mut c_void) -> c_int;
    fn pam_authenticate(pamh: *mut c_void, flags: c_int) -> c_int;
    fn pam_setcred(pamh: *mut c_void, flags: c_int) -> c_int;
    fn pam_end(pamh: *mut c_void, status: c_int) -> c_int;
}

const PAM_SUCCESS: c_int = 0;
const PAM_BUF_ERR: c_int = 5;
const PAM_PROMPT_ECHO_OFF: c_int = 1;
const PAM_PROMPT_ECHO_ON: c_int = 2;
const PAM_REFRESH_CRED: c_int = 0x10;

/// Answers every hidden question with the password and every other one
/// with nothing, as a lock screen that only asks for the password has to.
extern "C" fn converse(count: c_int, messages: *mut *const PamMessage, responses: *mut *mut PamResponse, data: *mut c_void) -> c_int {
    if count <= 0 {
        return PAM_BUF_ERR;
    }
    // SAFETY: PAM hands over `count` messages and the password we passed in.
    unsafe {
        let password = &*(data as *const CString);
        let answers = libc::calloc(count as usize, std::mem::size_of::<PamResponse>()) as *mut PamResponse;
        if answers.is_null() {
            return PAM_BUF_ERR;
        }
        for i in 0..count as usize {
            let m = &**messages.add(i);
            let a = &mut *answers.add(i);
            if m.msg_style == PAM_PROMPT_ECHO_OFF {
                a.resp = libc::strdup(password.as_ptr());
            } else if m.msg_style == PAM_PROMPT_ECHO_ON {
                a.resp = libc::strdup(c"".as_ptr());
            }
        }
        *responses = answers;
    }
    PAM_SUCCESS
}

fn service() -> &'static CStr {
    for (service, name) in [(c"face-unlock-lock", "face-unlock-lock"), (c"password-auth", "password-auth"), (c"common-auth", "common-auth"), (c"login", "login")] {
        for dir in ["/etc/pam.d/", "/usr/lib/pam.d/", "/usr/share/pam.d/"] {
            if std::path::Path::new(&format!("{dir}{name}")).exists() {
                return service;
            }
        }
    }
    c"login"
}

/// Whether `password` is this user's. Takes a while on a wrong one, so it
/// runs on a thread of its own. The password is wiped afterwards.
pub fn check(user: &str, password: Vec<u8>) -> bool {
    let Ok(user) = CString::new(user) else { return false };
    let mut password = password;
    let secret = match CString::new(std::mem::take(&mut password)) {
        Ok(s) => s,
        Err(e) => {
            let mut v = e.into_vec();
            v.fill(0);
            return false;
        }
    };
    let conv = PamConv { conv: converse, appdata_ptr: &secret as *const CString as *mut c_void };
    let mut pamh: *mut c_void = std::ptr::null_mut();
    // For development only: a directory of PAM files of its own, so a test
    // never counts as a wrong password for the real account.
    let confdir = std::env::var("FU_PAM_CONFDIR").ok().filter(|d| !d.is_empty()).and_then(|d| CString::new(d).ok());
    // SAFETY: plain calls into libpam with valid strings and a live conv.
    let ok = unsafe {
        let mut rc = match &confdir {
            Some(dir) => pam_start_confdir(c"face-unlock-lock".as_ptr(), user.as_ptr(), &conv, dir.as_ptr(), &mut pamh),
            None => pam_start(service().as_ptr(), user.as_ptr(), &conv, &mut pamh),
        };
        if rc == PAM_SUCCESS {
            rc = pam_authenticate(pamh, 0);
            if rc == PAM_SUCCESS {
                pam_setcred(pamh, PAM_REFRESH_CRED);
            }
        }
        if !pamh.is_null() {
            pam_end(pamh, rc);
        }
        rc == PAM_SUCCESS
    };
    let mut bytes = secret.into_bytes_with_nul();
    bytes.fill(0);
    ok
}

/// This user's login name, and the name to show: the full one from the
/// comment field, else the login name.
pub fn me() -> Option<(String, String)> {
    // SAFETY: getpwuid's answer is read at once, before anything else could
    // overwrite it.
    unsafe {
        let pw = libc::getpwuid(libc::getuid());
        if pw.is_null() {
            return None;
        }
        let login = CStr::from_ptr((*pw).pw_name).to_string_lossy().to_string();
        let gecos = if (*pw).pw_gecos.is_null() { String::new() } else { CStr::from_ptr((*pw).pw_gecos).to_string_lossy().to_string() };
        let full = gecos.split(',').next().unwrap_or("").trim().to_string();
        Some((login.clone(), if full.is_empty() { login } else { full }))
    }
}
