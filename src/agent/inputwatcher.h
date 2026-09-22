// SPDX-License-Identifier: GPL-3.0-or-later
//
// Somebody touching the machine: a key, a click, the mouse, the touchpad.
//
// From ext_idle_notifier_v1, like KIdleTime, but with the input notification
// of version 2. KIdleTime's own honours idle inhibitors: with a video playing
// or an app keeping the screen on, no key reached it and the lock screen never
// scanned.

#pragma once

#include <QObject>

#include <memory>

class IdleNotifier;
class IdleNotification;

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

Q_SIGNALS:
    void input();

private:
    std::unique_ptr<IdleNotifier> m_notifier;
    std::unique_ptr<IdleNotification> m_notification;
};
