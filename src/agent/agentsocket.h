// SPDX-License-Identifier: GPL-3.0-or-later
//
// Where the daemon tells this session about scans it did not ask for: sudo in
// a terminal, an admin prompt, a test from the menu. The daemon connects to
// $XDG_RUNTIME_DIR/face-unlock/agent.socket when such a scan starts,
// so neither side has to keep a connection open (and the daemon can exit when
// it is idle).
//
// Only root and this user may talk here, and all they can do is make the
// bubble move.

#pragma once

#include <QJsonObject>
#include <QLocalServer>
#include <QObject>

class AgentSocket : public QObject
{
    Q_OBJECT
public:
    explicit AgentSocket(QObject *parent = nullptr);
    bool listen();

    static QString path();

Q_SIGNALS:
    void scanEvent(const QJsonObject &event);

private:
    QLocalServer m_server;
};
