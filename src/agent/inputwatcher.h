// SPDX-License-Identifier: GPL-3.0-or-later
//
// Somebody touching the machine: a key, a click, the mouse, the touchpad.
//
// From ext_idle_notifier_v1, like KIdleTime, but with the input notification
// of version 2. KIdleTime's own honours idle inhibitors: with a video playing
// or an app keeping the screen on, no key reached it and the lock screen never
// scanned. GNOME has no ext_idle_notifier_v1; there it is Mutter's own
// IdleMonitor on the session bus, which knows nothing of inhibitors either.

#pragma once

#include <QElapsedTimer>
#include <QObject>

#include <memory>

class IdleNotifier;
class IdleNotification;
class MutterIdle;

class InputWatcher : public QObject
{
    Q_OBJECT
public:
    explicit InputWatcher(QObject *parent = nullptr);
    ~InputWatcher() override;

    // Emit input() once, for the first input after nothing was touched for
    // calmMs. 0: the next input. Replaces what was watched before.
    void watch(int calmMs);
    void stop();
    // A key or the pointer on face-unlock's own lock screen, which sees them
    // itself. Counts like input the compositor reports.
    void noteInput();

Q_SIGNALS:
    void input();

private:
    std::unique_ptr<IdleNotifier> m_notifier;
    std::unique_ptr<IdleNotification> m_notification;
    MutterIdle *m_mutter = nullptr;
    bool m_watching = false;
    int m_calmMs = 0;
    QElapsedTimer m_calm;
};
