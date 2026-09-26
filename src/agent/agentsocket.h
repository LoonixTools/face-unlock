// SPDX-License-Identifier: GPL-3.0-or-later
//
// Where the daemon tells this session about scans it did not ask for: sudo in
// a terminal, an admin prompt, a test from the menu. The daemon connects to
// $XDG_RUNTIME_DIR/face-unlock/agent.socket when such a scan starts,
// so neither side has to keep a connection open (and the daemon can exit when
// it is idle).
//
// Only root and this user may talk here, and all they can do is make the
// bubble move, or (this user) ask for face-unlock's own lock screen.

#pragma once

#include <QJsonObject>
#include <QList>
#include <QLocalServer>
#include <QLocalSocket>
#include <QObject>
#include <QPointer>

class AgentSocket : public QObject
{
    Q_OBJECT
public:
    explicit AgentSocket(QObject *parent = nullptr);
    bool listen();

    // Whether another agent already listens here. Hyprland without a systemd
    // session starts the agent from its own config, and that must not make
    // two of them.
    static bool running();
    // Ask the running agent to lock the screen, and wait until it is. False
    // when there is no agent or the lock did not come.
    static bool requestLock();
    // Tell everybody waiting in requestLock() that the screen is locked.
    void confirmLock();
    static QString path();

Q_SIGNALS:
    void scanEvent(const QJsonObject &event);
    void lockRequested();

private:
    QLocalServer m_server;
    QList<QPointer<QLocalSocket>> m_waiting;
};
