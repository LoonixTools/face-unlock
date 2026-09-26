// SPDX-License-Identifier: GPL-3.0-or-later
//
// A lock screen gets a signal only once it handles it. Before that, the
// signal would kill it and leave the screen locked for good. The lock
// screen here is a child of the test that calls itself hyprlock, on a
// display of its own, so no real one is ever found. The display is set by
// starting the test again: /proc shows the environment a program started
// with, not what it set later.

#include "lockers.h"

#include <QtGlobal>

#include <csignal>
#include <cstdlib>
#include <cstdio>
#include <poll.h>
#include <sys/prctl.h>
#include <sys/wait.h>
#include <unistd.h>

namespace
{
int failures = 0;

void check(bool ok, const char *what)
{
    std::printf("%s  %s\n", ok ? "ok  " : "FAIL", what);
    if (!ok) {
        ++failures;
    }
}

int toParent = -1;

void answer(int)
{
    [[maybe_unused]] const ssize_t n = write(toParent, "s", 1);
}

// A byte from the child, or 0 after a second without one.
char next(int from)
{
    pollfd p{from, POLLIN, 0};
    char c = 0;
    if (poll(&p, 1, 1000) == 1 && read(from, &c, 1) == 1) {
        return c;
    }
    return 0;
}

// A hyprlock that handles SIGUSR2 or not. Says "r" when it is set up, and
// "s" for every SIGUSR2.
pid_t fakeHyprlock(bool handles, int *fromChild)
{
    int fds[2];
    if (pipe(fds) != 0) {
        return -1;
    }
    const pid_t pid = fork();
    if (pid == 0) {
        close(fds[0]);
        toParent = fds[1];
        prctl(PR_SET_NAME, "hyprlock");
        if (handles) {
            std::signal(SIGUSR2, answer);
        }
        [[maybe_unused]] const ssize_t n = write(toParent, "r", 1);
        for (;;) {
            pause();
        }
    }
    close(fds[1]);
    *fromChild = fds[0];
    next(fds[0]);
    return pid;
}

bool alive(pid_t pid)
{
    return waitpid(pid, nullptr, WNOHANG) == 0;
}
} // namespace

int main(int, char **argv)
{
    const char *display = "fu-test-lockers";
    if (qgetenv("WAYLAND_DISPLAY") != display) {
        setenv("WAYLAND_DISPLAY", display, 1);
        execv("/proc/self/exe", argv);
        return 2;
    }

    int from = -1;
    pid_t pid = fakeHyprlock(false, &from);
    check(Lockers::find("hyprlock") == QList<pid_t>{pid}, "finds the lock screen on this display");
    check(Lockers::find("swaylock").isEmpty(), "and only by that name");
    check(!Lockers::ready(SIGUSR2), "not ready while it does not handle the signal");
    check(Lockers::signal(SIGUSR2, "hyprlock") == 1, "one not ready yet");
    usleep(200 * 1000);
    check(alive(pid), "and it was left alone, alive");
    kill(pid, SIGKILL);
    waitpid(pid, nullptr, 0);
    close(from);

    pid = fakeHyprlock(true, &from);
    check(Lockers::ready(SIGUSR2), "ready once it does");
    check(Lockers::signal(SIGUSR2, "hyprlock") == 0, "a ready one gets it");
    check(next(from) == 's', "and handles it");
    check(alive(pid), "and lives on");
    kill(pid, SIGKILL);
    waitpid(pid, nullptr, 0);
    close(from);

    std::printf("\n%s\n", failures ? "SOME CHECKS FAILED" : "all checks passed");
    return failures ? 1 : 0;
}
