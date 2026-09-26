// SPDX-License-Identifier: GPL-3.0-or-later

#include "lockers.h"

#include <QFile>
#include <QString>

#include <csignal>
#include <dirent.h>
#include <sys/stat.h>
#include <unistd.h>

namespace
{
bool isLocker(const QByteArray &name)
{
    return name == "hyprlock" || name == "swaylock";
}

QByteArray readFile(const QString &path)
{
    QFile f(path);
    return f.open(QIODevice::ReadOnly) ? f.readAll() : QByteArray();
}

pid_t parentOf(pid_t pid)
{
    // The fields after the name in brackets, which can hold anything.
    const QByteArray stat = readFile(QStringLiteral("/proc/%1/stat").arg(pid));
    const qsizetype close = stat.lastIndexOf(')');
    return close < 0 ? 0 : pid_t(stat.mid(close + 2).split(' ').value(1).toInt());
}

// The lock screens of this user on this display. Another session of the same
// user has a display of its own, and its lock screen is left alone. A child
// of a lock screen (swaylock checks the password in one) is left out: killed
// by the signal before its parent unlocks, it would take the parent down
// with it, and the screen would stay locked.
QList<pid_t> findAll()
{
    QList<pid_t> found;
    DIR *proc = ::opendir("/proc");
    if (!proc) {
        return found;
    }
    QByteArray display = qgetenv("WAYLAND_DISPLAY");
    if (display.isEmpty()) {
        display = "wayland-0";
    }
    const uid_t me = ::getuid();
    while (dirent *entry = ::readdir(proc)) {
        const pid_t pid = pid_t(atoi(entry->d_name));
        struct stat st;
        if (pid <= 0 || ::fstatat(::dirfd(proc), entry->d_name, &st, 0) != 0 || st.st_uid != me) {
            continue;
        }
        const QString dir = QStringLiteral("/proc/%1/").arg(pid);
        if (!isLocker(readFile(dir + QStringLiteral("comm")).trimmed())) {
            continue;
        }
        bool sameDisplay = true;
        for (const QByteArray &var : readFile(dir + QStringLiteral("environ")).split('\0')) {
            if (var.startsWith("WAYLAND_DISPLAY=")) {
                sameDisplay = var.mid(16) == display;
                break;
            }
        }
        if (sameDisplay) {
            found.append(pid);
        }
    }
    ::closedir(proc);
    QList<pid_t> top;
    for (const pid_t pid : std::as_const(found)) {
        if (!found.contains(parentOf(pid))) {
            top.append(pid);
        }
    }
    return top;
}

// Whether pid catches sig, from the mask in /proc/<pid>/status.
bool catches(pid_t pid, int sig)
{
    for (const QByteArray &line : readFile(QStringLiteral("/proc/%1/status").arg(pid)).split('\n')) {
        if (line.startsWith("SigCgt:")) {
            bool ok = false;
            const qulonglong mask = line.mid(7).trimmed().toULongLong(&ok, 16);
            return ok && (mask >> (sig - 1)) & 1;
        }
    }
    return false;
}
} // namespace

QList<pid_t> Lockers::find(const QByteArray &name)
{
    QList<pid_t> found;
    for (const pid_t pid : findAll()) {
        if (name.isEmpty() || readFile(QStringLiteral("/proc/%1/comm").arg(pid)).trimmed() == name) {
            found.append(pid);
        }
    }
    return found;
}

int Lockers::signal(int sig, const QByteArray &name)
{
    int waiting = 0;
    for (const pid_t pid : find(name)) {
        if (catches(pid, sig)) {
            ::kill(pid, sig);
        } else {
            ++waiting;
        }
    }
    return waiting;
}
