// SPDX-License-Identifier: GPL-3.0-or-later
//
// The lock screen.
//
// When the screen locks, this waits for somebody to come back: a key, the
// mouse, the lid opening, the machine waking from sleep. Then it scans, and
// when the face matches it unlocks the session the way that desktop's lock
// screen wants it (see LockWatcher).
//
// Why not a PAM module in the lock screen, like fingerprints? Plasma runs its
// fingerprint stack in parallel with the password, but only starts it once per
// lock, gives up for good the first time it fails, and labels it "scan your
// fingerprint". GNOME's, hyprlock's and swaylock's only ask once the password
// is typed. Doing it from here means scanning again every time somebody sits
// back down, no wrong label, and a bubble that knows what is going on.

#pragma once

#include "inputwatcher.h"
#include "lockwatcher.h"

#include <QElapsedTimer>
#include <QObject>
#include <QPointer>
#include <QTimer>

class BubbleController;
class DaemonRequest;
class LockScreenController;
class SessionLock;
class UserConfig;

class LockController : public QObject
{
    Q_OBJECT
public:
    // lock and screen are face-unlock's own lock screen, where there is one.
    LockController(BubbleController *bubble, UserConfig *config, SessionLock *lock, LockScreenController *screen, QObject *parent = nullptr);

    bool locked() const
    {
        return m_locked;
    }

private Q_SLOTS:
    void onLockedChanged(bool locked);
    void onPrepareForSleep(bool sleeping);

private:
    // Scan at the next input after calmMs without any.
    void arm(int calmMs);
    void onResume();
    void startScan(const QString &why);
    void onScanFinished(const QJsonObject &result);
    void warmUp();

    BubbleController *m_bubble;
    UserConfig *m_config;
    LockScreenController *m_screen;
    bool m_locked = false;
    bool m_stopped = false;
    QElapsedTimer m_lockedFor;
    QPointer<DaemonRequest> m_scan;
    QTimer m_armTimer;
    InputWatcher m_input;
    LockWatcher m_lock;
};
