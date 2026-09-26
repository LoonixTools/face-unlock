// SPDX-License-Identifier: GPL-3.0-or-later

#include "lockwatcher.h"

#include "lockers.h"
#include "wayland.h"

#include <QDBusConnection>
#include <QDBusMessage>
#include <QDBusObjectPath>
#include <QDBusPendingCallWatcher>
#include <QDBusPendingReply>
#include <QDBusVariant>
#include <QProcess>

#include <csignal>
#include <unistd.h>

namespace
{
const QString Login1 = QStringLiteral("org.freedesktop.login1");
const QString SessionInterface = QStringLiteral("org.freedesktop.login1.Session");
const QString Properties = QStringLiteral("org.freedesktop.DBus.Properties");

constexpr int PollMs = 1000;

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

    m_ownLockers = Wayland::hasGlobal("ext_session_lock_manager_v1");
    if (m_ownLockers) {
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
    m_lockerRunning = !Lockers::find().isEmpty();
    update();
}

bool LockWatcher::canUnlock() const
{
    return !m_ownLockers || m_lockerRunning;
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
    Lockers::signal(SIGUSR1);

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
