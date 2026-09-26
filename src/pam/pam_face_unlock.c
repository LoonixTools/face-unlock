// SPDX-License-Identifier: GPL-3.0-or-later
//
// pam_face_unlock: face unlock for sudo, admin prompts and lock screens.
//
// All the vision is in the daemon. This asks it to scan for the user and
// turns the answer into a PAM result, and it is meant to sit first in a stack
// as "auth sufficient": a match lets the person in, anything else falls
// through to the password as if the module was not there.
//
// It refuses to be useful in exactly the places where a camera is the wrong
// witness:
//   - a remote login (SSH, a remote host in PAM_RHOST). Whoever is in front
//     of the camera is not the person typing.
//   - a user who is not sitting at the machine right now, with an active
//     session on a seat.
// In both cases it returns PAM_IGNORE without touching the camera.
//
// Options:
//   purpose=sudo|polkit|other  what the bubble says (default: from the
//                              service name)
//   lockscreen                 in a lock screen's stack (hyprlock, swaylock,
//                              gtklock...): see is_lock_starting()
//   socket=PATH                the daemon's socket (for development)
//   timeout=SECONDS            give up waiting for the daemon after this
//   debug                      log to the auth log what happened

#define _GNU_SOURCE
#define PAM_SM_AUTH

#include "buildconfig.h"

#include <security/pam_ext.h>
#include <security/pam_modules.h>

#include <errno.h>
#include <libintl.h>
#include <locale.h>
#include <poll.h>
#include <stdbool.h>
#include <stdio.h>
#include <stdlib.h>
#include <string.h>
#include <sys/socket.h>
#include <sys/un.h>
#include <syslog.h>
#include <time.h>
#include <unistd.h>

#ifdef HAVE_SYSTEMD
#include <pwd.h>
#include <systemd/sd-login.h>
#endif

#define DOMAIN FU_NAME
// Marks a message for translation. say() translates it.
#define _(s) (s)

struct options {
    const char *socket;
    const char *purpose;
    int timeout;
    bool lockscreen;
    bool debug;
};

static void parse_options(struct options *o, int argc, const char **argv)
{
    o->socket = FU_SOCKET;
    o->purpose = NULL;
    o->timeout = 25;
    o->lockscreen = false;
    o->debug = false;
    for (int i = 0; i < argc; ++i) {
        if (strncmp(argv[i], "socket=", 7) == 0) {
            o->socket = argv[i] + 7;
        } else if (strncmp(argv[i], "purpose=", 8) == 0) {
            o->purpose = argv[i] + 8;
        } else if (strncmp(argv[i], "timeout=", 8) == 0) {
            o->timeout = atoi(argv[i] + 8);
            if (o->timeout < 3 || o->timeout > 120) {
                o->timeout = 25;
            }
        } else if (strcmp(argv[i], "lockscreen") == 0) {
            o->lockscreen = true;
        } else if (strcmp(argv[i], "debug") == 0) {
            o->debug = true;
        }
    }
}

static long long now_ms(void)
{
    struct timespec ts;
    clock_gettime(CLOCK_MONOTONIC, &ts);
    return (long long)ts.tv_sec * 1000 + ts.tv_nsec / 1000000;
}

static bool nonempty(const char *s)
{
    return s && *s;
}

// hyprlock asks PAM the moment it starts, before anybody pressed a key.
// Scanning then would open the screen again for whoever just locked it on
// purpose while still sitting in front of it. So a lock screen that is less
// than two seconds old gets no scan; pressing Enter asks again, and the agent
// scans when somebody comes back.
static bool is_lock_starting(void)
{
    FILE *stat = fopen("/proc/self/stat", "r");
    FILE *uptime = fopen("/proc/uptime", "r");
    unsigned long long started = 0;
    double up = 0;
    bool ok = stat && uptime && fscanf(uptime, "%lf", &up) == 1;
    if (ok) {
        // After the name in brackets, which can hold anything, the start
        // time is the 20th field.
        char line[1024];
        ok = fgets(line, sizeof(line), stat) != NULL;
        const char *p = ok ? strrchr(line, ')') : NULL;
        ok = p && sscanf(p + 2, "%*c %*d %*d %*d %*d %*d %*u %*u %*u %*u %*u %*u %*u %*d %*d %*d %*d %*d %*d %llu", &started) == 1;
    }
    if (stat) {
        fclose(stat);
    }
    if (uptime) {
        fclose(uptime);
    }
    const long ticks = sysconf(_SC_CLK_TCK);
    return ok && ticks > 0 && up - (double)started / (double)ticks < 2.0;
}

// Whoever types this is not whoever sits in front of the camera.
static bool is_remote(pam_handle_t *pamh)
{
    const void *rhost = NULL;
    if (pam_get_item(pamh, PAM_RHOST, &rhost) == PAM_SUCCESS && nonempty(rhost) && strcmp(rhost, "localhost") != 0) {
        return true;
    }
    static const char *const vars[] = {"SSH_CONNECTION", "SSH_CLIENT", "SSH_TTY"};
    for (size_t i = 0; i < sizeof(vars) / sizeof(vars[0]); ++i) {
        if (nonempty(getenv(vars[i])) || nonempty(pam_getenv(pamh, vars[i]))) {
            return true;
        }
    }
#ifdef HAVE_SYSTEMD
    char *session = NULL;
    if (sd_pid_get_session(0, &session) >= 0 && session) {
        const bool remote = sd_session_is_remote(session) > 0;
        free(session);
        if (remote) {
            return true;
        }
    }
#endif
    return false;
}

// The person has to be at the machine: an active session on a seat.
static bool is_at_seat(const char *user)
{
#ifdef HAVE_SYSTEMD
    struct passwd pw, *result = NULL;
    char buf[4096];
    if (getpwnam_r(user, &pw, buf, sizeof(buf), &result) != 0 || !result) {
        return false;
    }
    // The number of seats the user is active on, whatever they are called.
    return sd_uid_get_seats(result->pw_uid, 1, NULL) > 0;
#else
    (void)user;
    return true;
#endif
}

static int connect_daemon(const struct options *o, pam_handle_t *pamh)
{
    struct sockaddr_un addr = {.sun_family = AF_UNIX};
    if (strlen(o->socket) >= sizeof(addr.sun_path)) {
        return -1;
    }
    strcpy(addr.sun_path, o->socket);

    const int fd = socket(AF_UNIX, SOCK_STREAM | SOCK_CLOEXEC, 0);
    if (fd < 0) {
        return -1;
    }
    struct timeval tv = {.tv_sec = 3};
    setsockopt(fd, SOL_SOCKET, SO_SNDTIMEO, &tv, sizeof(tv));
    if (connect(fd, (struct sockaddr *)&addr, sizeof(addr)) != 0) {
        if (o->debug) {
            pam_syslog(pamh, LOG_DEBUG, "cannot reach the daemon at %s: %s", o->socket, strerror(errno));
        }
        close(fd);
        return -1;
    }

    // Only root may answer for root. When this runs as somebody else (the
    // tests), a daemon of that same user is no less trusted than the process
    // asking.
    struct ucred cred;
    socklen_t len = sizeof(cred);
    if (getsockopt(fd, SOL_SOCKET, SO_PEERCRED, &cred, &len) != 0 || (cred.uid != 0 && cred.uid != geteuid())) {
        pam_syslog(pamh, LOG_WARNING, "refusing a face unlock daemon that does not run as root");
        close(fd);
        return -1;
    }
    return fd;
}

// The daemon's answers are compact JSON objects with plain values. It runs as
// root and is the one party here that is trusted, so this only has to be
// correct for well formed input and safe for anything else.
static bool json_string(const char *line, const char *key, char *out, size_t size)
{
    char pattern[64];
    snprintf(pattern, sizeof(pattern), "\"%s\":\"", key);
    const char *p = strstr(line, pattern);
    if (!p || size == 0) {
        return false;
    }
    p += strlen(pattern);
    size_t n = 0;
    while (*p && *p != '"' && n + 1 < size) {
        if (*p == '\\' && p[1]) {
            ++p;
        }
        out[n++] = *p++;
    }
    out[n] = '\0';
    return true;
}

static bool json_true(const char *line, const char *key)
{
    char pattern[64];
    snprintf(pattern, sizeof(pattern), "\"%s\":true", key);
    return strstr(line, pattern) != NULL;
}

// Admin prompts run this in polkit's helper. It keeps LANG but never calls
// setlocale, so gettext would stay in English. Take the language from the
// environment for this thread while the message goes out.
static void say(pam_handle_t *pamh, int flags, const char *message)
{
    if (flags & PAM_SILENT) {
        return;
    }
    locale_t loc = (locale_t)0, old = (locale_t)0;
    const char *global = setlocale(LC_MESSAGES, NULL);
    if (!global || strcmp(global, "C") == 0 || strcmp(global, "POSIX") == 0) {
        loc = newlocale(LC_MESSAGES_MASK, "", (locale_t)0);
        if (loc) {
            old = uselocale(loc);
        }
    }
    pam_info(pamh, "%s", dgettext(DOMAIN, message));
    if (loc) {
        uselocale(old);
        freelocale(loc);
    }
}

__attribute__((visibility("default"))) PAM_EXTERN int pam_sm_authenticate(pam_handle_t *pamh, int flags, int argc, const char **argv)
{
    struct options o;
    parse_options(&o, argc, argv);
    bindtextdomain(DOMAIN, FU_LOCALEDIR);

    const char *user = NULL;
    if (pam_get_user(pamh, &user, NULL) != PAM_SUCCESS || !nonempty(user)) {
        return PAM_IGNORE;
    }

    const void *service = NULL;
    pam_get_item(pamh, PAM_SERVICE, &service);
    const char *purpose = o.purpose;
    if (!purpose && o.lockscreen) {
        purpose = "lockscreen";
    }
    if (!purpose) {
        purpose = !service                                ? "other"
            : strncmp(service, "sudo", 4) == 0            ? "sudo"
            : strcmp(service, "polkit-1") == 0            ? "polkit"
                                                          : "other";
    }

    if (o.lockscreen && is_lock_starting()) {
        if (o.debug) {
            pam_syslog(pamh, LOG_DEBUG, "the lock screen has only just started, not scanning for %s", user);
        }
        return PAM_IGNORE;
    }
    if (is_remote(pamh)) {
        if (o.debug) {
            pam_syslog(pamh, LOG_DEBUG, "remote session, not scanning for %s", user);
        }
        return PAM_IGNORE;
    }
    if (!is_at_seat(user)) {
        if (o.debug) {
            pam_syslog(pamh, LOG_DEBUG, "%s is not at the machine, not scanning", user);
        }
        return PAM_IGNORE;
    }

    const int fd = connect_daemon(&o, pamh);
    if (fd < 0) {
        return PAM_IGNORE;
    }

    char request[512];
    const int len = snprintf(request, sizeof(request), "{\"cmd\":\"verify\",\"user\":\"%s\",\"purpose\":\"%s\"}\n", user, purpose);
    if (len <= 0 || (size_t)len >= sizeof(request) || strpbrk(user, "\"\\") || write(fd, request, (size_t)len) != len) {
        close(fd);
        return PAM_IGNORE;
    }

    const long long deadline = now_ms() + (long long)o.timeout * 1000;
    char buf[8192];
    size_t used = 0;
    int rc = PAM_AUTHINFO_UNAVAIL;
    bool done = false;
    bool told = false;

    while (!done) {
        const long long left = deadline - now_ms();
        if (left <= 0) {
            break;
        }
        struct pollfd pfd = {.fd = fd, .events = POLLIN};
        const int ready = poll(&pfd, 1, (int)left);
        if (ready < 0 && errno == EINTR) {
            // Ctrl+C in sudo. Stop the camera and go on to the password.
            break;
        }
        if (ready <= 0) {
            break;
        }
        const ssize_t got = read(fd, buf + used, sizeof(buf) - 1 - used);
        if (got <= 0) {
            break;
        }
        used += (size_t)got;
        buf[used] = '\0';

        char *line = buf;
        char *nl;
        while ((nl = strchr(line, '\n'))) {
            *nl = '\0';
            char event[32] = "", value[128] = "";
            json_string(line, "event", event, sizeof(event));

            if (strcmp(event, "started") == 0 && !told) {
                told = true;
                say(pamh, flags, _("Look at the camera to unlock."));
            } else if (strcmp(event, "hint") == 0 && json_string(line, "hint", value, sizeof(value))) {
                if (strcmp(value, "blink") == 0) {
                    say(pamh, flags, _("Blink, or turn your head a little."));
                } else if (strcmp(value, "look") == 0) {
                    say(pamh, flags, _("Look at the screen."));
                } else if (strcmp(value, "closer") == 0) {
                    say(pamh, flags, _("Move closer to the camera."));
                } else if (strcmp(value, "light") == 0) {
                    say(pamh, flags, _("It is too dark to see your face."));
                }
            } else if (strcmp(event, "result") == 0) {
                done = true;
                json_string(line, "reason", value, sizeof(value));
                if (json_true(line, "ok")) {
                    rc = PAM_SUCCESS;
                    pam_syslog(pamh, LOG_NOTICE, "face recognised for %s (%s)", user, purpose);
                } else if (strcmp(value, "lockout") == 0 || strstr(line, "\"lockout\":")) {
                    rc = PAM_AUTH_ERR;
                    say(pamh, flags, _("Face unlock is paused after too many tries. Use your password."));
                } else if (strcmp(value, "mismatch") == 0 || strcmp(value, "spoof") == 0 || strcmp(value, "liveness") == 0) {
                    rc = PAM_AUTH_ERR;
                    pam_syslog(pamh, LOG_NOTICE, "face not recognised for %s (%s)", user, value);
                    say(pamh, flags, _("Face not recognised."));
                } else if (strcmp(value, "no-face") == 0 || strcmp(value, "attention") == 0 || strcmp(value, "quality") == 0) {
                    rc = PAM_AUTH_ERR;
                } else {
                    // Not set up, no camera, lid shut, busy: face unlock is
                    // simply not available right now.
                    rc = PAM_AUTHINFO_UNAVAIL;
                    if (o.debug) {
                        pam_syslog(pamh, LOG_DEBUG, "face unlock unavailable for %s: %s", user, value);
                    }
                }
                break;
            }
            line = nl + 1;
        }
        // Keep what is left of an unfinished line.
        used = strlen(line);
        memmove(buf, line, used + 1);
        if (used >= sizeof(buf) - 1) {
            break;
        }
    }

    close(fd);
    return rc;
}

__attribute__((visibility("default"))) PAM_EXTERN int pam_sm_setcred(pam_handle_t *pamh, int flags, int argc, const char **argv)
{
    (void)pamh;
    (void)flags;
    (void)argc;
    (void)argv;
    return PAM_IGNORE;
}
