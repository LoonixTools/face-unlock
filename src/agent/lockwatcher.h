// SPDX-License-Identifier: GPL-3.0-or-later
//
// Whether the screen is locked, and how to unlock it, on each desktop.
//
//   Plasma    org.freedesktop.ScreenSaver and logind's LockedHint. Unlocked
//             through logind.
//   GNOME     logind's LockedHint. Unlocked through logind.
//   Niri      logind's LockedHint, which niri sets for any lock screen.
//   Hyprland  sets no LockedHint, so the lock screen is looked for among this
//             user's processes (hyprlock, swaylock) once a second. Only on
//             compositors where the lock screen is a program of its own
//             (ext-session-lock).
//
// hyprlock and swaylock do not listen to logind. They unlock on SIGUSR1.
//
// None of this lowers the bar: any program running as this user can already
// ask logind to unlock the session, or send its own lock screen a signal. The
// face data and the decision stay with the daemon, which runs as root.

#pragma once

#include <QObject>
#include <QTimer>
#include <QVariantMap>

#include <sys/types.h>

class LockWatcher : public QObject
{
    Q_OBJECT
public:
    explicit LockWatcher(QObject *parent = nullptr);

    bool locked() const
    {
        return m_locked;
    }
    void unlock();

Q_SIGNALS:
    void lockedChanged(bool locked);

private Q_SLOTS:
    void onScreenSaver(bool active);
    void onSessionProperties(const QString &interface, const QVariantMap &changed, const QStringList &invalidated);

private:
    void findSession();
    void watchSession(const QString &path);
    void readLockedHint();
    void pollLockers();
    void update();

    bool m_screenSaver = false;
    bool m_lockedHint = false;
    bool m_lockerRunning = false;
    bool m_locked = false;
    QString m_session;
    QTimer m_poll;
};
