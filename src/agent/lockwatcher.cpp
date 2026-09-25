// SPDX-License-Identifier: GPL-3.0-or-later

#include "lockwatcher.h"

#include "wayland.h"

#include <QDBusConnection>
#include <QDBusMessage>
#include <QDBusObjectPath>
#include <QDBusPendingCallWatcher>
#include <QDBusPendingReply>
#include <QDBusVariant>
#include <QFile>
#include <QProcess>

#include <dirent.h>
#include <signal.h>
#include <sys/stat.h>
#include <unistd.h>

namespace
{
const QString Login1 = QStringLiteral("org.freedesktop.login1");
const QString SessionInterface = QStringLiteral("org.freedesktop.login1.Session");
const QString Properties = QStringLiteral("org.freedesktop.DBus.Properties");

constexpr int PollMs = 1000;

// The lock screens that unlock on SIGUSR1.
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
QList<pid_t> findLockers()
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
} // namespace

LockWatcher::LockWatcher(QObject *parent)
    : QObject(parent)
{
    QDBusConnection session = QDBusConnection::sessionBus();
    session.connect(QStringLiteral("org.freedesktop.ScreenSaver"),
                    QStringLiteral("/ScreenSaver"),
                    QStringLiteral("org.freedesktop.ScreenSaver"),
                    QStringLiteral("ActiveChanged"),
                    this,
                    SLOT(onScreenSaver(bool)));

    // Started while the screen is already locked (the agent restarted).
    QDBusMessage get = QDBusMessage::createMethodCall(QStringLiteral("org.freedesktop.ScreenSaver"),
                                                      QStringLiteral("/ScreenSaver"),
                                                      QStringLiteral("org.freedesktop.ScreenSaver"),
                                                      QStringLiteral("GetActive"));
    get.setAutoStartService(false);
    auto *watcher = new QDBusPendingCallWatcher(session.asyncCall(get), this);
    connect(watcher, &QDBusPendingCallWatcher::finished, this, [this, watcher] {
        watcher->deleteLater();
        const QDBusPendingReply<bool> reply = *watcher;
        if (reply.isValid() && reply.value()) {
            onScreenSaver(true);
        }
    });

    findSession();

    if (Wayland::hasGlobal("ext_session_lock_manager_v1")) {
        connect(&m_poll, &QTimer::timeout, this, &LockWatcher::pollLockers);
        m_poll.start(PollMs);
        // Not from here: nobody is connected yet to hear about a lock screen
        // that is already up.
        QTimer::singleShot(0, this, &LockWatcher::pollLockers);
    }
}

void LockWatcher::onScreenSaver(bool active)
{
    m_screenSaver = active;
    update();
}

void LockWatcher::findSession()
{
    // Niri and some others hand their session on to user services. Without
    // it, "auto" is the session on the display.
    const QString id = qEnvironmentVariable("XDG_SESSION_ID");
    if (!id.isEmpty()) {
        QDBusMessage call = QDBusMessage::createMethodCall(Login1,
                                                           QStringLiteral("/org/freedesktop/login1"),
                                                           QStringLiteral("org.freedesktop.login1.Manager"),
                                                           QStringLiteral("GetSession"));
        call << id;
        auto *watcher = new QDBusPendingCallWatcher(QDBusConnection::systemBus().asyncCall(call), this);
        connect(watcher, &QDBusPendingCallWatcher::finished, this, [this, watcher] {
            watcher->deleteLater();
            const QDBusPendingReply<QDBusObjectPath> reply = *watcher;
            watchSession(reply.isValid() ? reply.value().path() : QStringLiteral("/org/freedesktop/login1/session/auto"));
        });
        return;
    }
    QDBusMessage call = QDBusMessage::createMethodCall(Login1, QStringLiteral("/org/freedesktop/login1/session/auto"), Properties, QStringLiteral("Get"));
    call << SessionInterface << QStringLiteral("Id");
    auto *watcher = new QDBusPendingCallWatcher(QDBusConnection::systemBus().asyncCall(call), this);
    connect(watcher, &QDBusPendingCallWatcher::finished, this, [this, watcher] {
        watcher->deleteLater();
        const QDBusPendingReply<QDBusVariant> reply = *watcher;
        if (!reply.isValid()) {
            qWarning("logind knows no session for this agent: %s", qPrintable(reply.error().message()));
            return;
        }
        // The signals come from the session's real path, not from "auto".
        QDBusMessage call = QDBusMessage::createMethodCall(Login1,
                                                           QStringLiteral("/org/freedesktop/login1"),
                                                           QStringLiteral("org.freedesktop.login1.Manager"),
                                                           QStringLiteral("GetSession"));
        call << reply.value().variant().toString();
        auto *next = new QDBusPendingCallWatcher(QDBusConnection::systemBus().asyncCall(call), this);
        connect(next, &QDBusPendingCallWatcher::finished, this, [this, next] {
            next->deleteLater();
            const QDBusPendingReply<QDBusObjectPath> path = *next;
            if (path.isValid()) {
                watchSession(path.value().path());
            }
        });
    });
}

void LockWatcher::watchSession(const QString &path)
{
    m_session = path;
    QDBusConnection::systemBus().connect(Login1,
                                         path,
                                         Properties,
                                         QStringLiteral("PropertiesChanged"),
                                         this,
                                         SLOT(onSessionProperties(QString, QVariantMap, QStringList)));
    readLockedHint();
}

void LockWatcher::readLockedHint()
{
    QDBusMessage call = QDBusMessage::createMethodCall(Login1, m_session, Properties, QStringLiteral("Get"));
    call << SessionInterface << QStringLiteral("LockedHint");
    auto *watcher = new QDBusPendingCallWatcher(QDBusConnection::systemBus().asyncCall(call), this);
    connect(watcher, &QDBusPendingCallWatcher::finished, this, [this, watcher] {
        watcher->deleteLater();
        const QDBusPendingReply<QDBusVariant> reply = *watcher;
        if (reply.isValid()) {
            m_lockedHint = reply.value().variant().toBool();
            update();
        }
    });
}

void LockWatcher::onSessionProperties(const QString &interface, const QVariantMap &changed, const QStringList &invalidated)
{
    if (interface != SessionInterface) {
        return;
    }
    if (changed.contains(QStringLiteral("LockedHint"))) {
        m_lockedHint = changed.value(QStringLiteral("LockedHint")).toBool();
        update();
    } else if (invalidated.contains(QStringLiteral("LockedHint"))) {
        readLockedHint();
    }
}

void LockWatcher::pollLockers()
{
    m_lockerRunning = !findLockers().isEmpty();
    update();
}

void LockWatcher::update()
{
    const bool locked = m_screenSaver || m_lockedHint || m_lockerRunning;
    if (locked != m_locked) {
        m_locked = locked;
        Q_EMIT lockedChanged(locked);
    }
}

void LockWatcher::unlock()
{
    // Looked up again rather than taken from the last poll: a second is
    // long enough for a pid to belong to something else.
    for (const pid_t pid : findLockers()) {
        ::kill(pid, SIGUSR1);
    }

    const QDBusMessage call = QDBusMessage::createMethodCall(Login1,
                                                             m_session.isEmpty() ? QStringLiteral("/org/freedesktop/login1/session/auto") : m_session,
                                                             SessionInterface,
                                                             QStringLiteral("Unlock"));
    auto *watcher = new QDBusPendingCallWatcher(QDBusConnection::systemBus().asyncCall(call), this);
    connect(watcher, &QDBusPendingCallWatcher::finished, this, [watcher] {
        watcher->deleteLater();
        const QDBusPendingReply<> reply = *watcher;
        if (reply.isError()) {
            qWarning("logind would not unlock: %s; trying loginctl", qPrintable(reply.error().message()));
            const QString id = qEnvironmentVariable("XDG_SESSION_ID");
            QStringList args{QStringLiteral("unlock-session")};
            if (!id.isEmpty()) {
                args << id;
            }
            QProcess::startDetached(QStringLiteral("loginctl"), args);
        }
    });
}
