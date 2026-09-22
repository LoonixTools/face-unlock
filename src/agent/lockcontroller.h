// SPDX-License-Identifier: GPL-3.0-or-later
//
// The lock screen.
//
// When the screen locks, this waits for somebody to come back: a key, the
// mouse, the lid opening, the machine waking from sleep. Then it scans, and
// when the face matches it asks logind to unlock the session, which is the
// same request `loginctl unlock-session` makes and which Plasma's screen
// locker has always honoured.
//
// Why not a PAM module in the lock screen, like fingerprints? Plasma runs its
// fingerprint stack in parallel with the password, but only starts it once per
// lock, gives up for good the first time it fails, and labels it "scan your
// fingerprint". Doing it from here means scanning again every time somebody
// sits back down, no wrong label, and a bubble that knows what is going on.
// It does not lower the bar either: any program running as this user can
// already unlock this user's session through logind. Face data and the
// decision stay with the daemon, which runs as root.

#pragma once

#include <QElapsedTimer>
#include <QObject>
#include <QPointer>
#include <QTimer>

class BubbleController;
class DaemonRequest;
class UserConfig;

class LockController : public QObject
{
    Q_OBJECT
public:
    LockController(BubbleController *bubble, UserConfig *config, QObject *parent = nullptr);

    bool locked() const
    {
        return m_locked;
    }

private Q_SLOTS:
    void onActiveChanged(bool active);
    void onPrepareForSleep(bool sleeping);

private:
    void arm();
    void onResume();
    void startScan(const QString &why);
    void onScanFinished(const QJsonObject &result);
    void unlock();
    void warmUp();

    BubbleController *m_bubble;
    UserConfig *m_config;
    bool m_locked = false;
    bool m_stopped = false;
    QElapsedTimer m_lockedFor;
    QPointer<DaemonRequest> m_scan;
    QTimer m_armTimer;
    int m_idleId = -1;
};
