// SPDX-License-Identifier: GPL-3.0-or-later
//
// Telling a user's session about a scan it did not start itself (sudo, an
// admin prompt, a test from the menu), so the bubble can show it.
//
// The agent listens on /run/user/UID/plasma-face-unlock/agent.socket. That is
// a place the user controls, so nothing is taken for granted: the socket has
// to belong to the user, and so does the process that answers on it, checked
// by the kernel before a single byte is written. What is written is only
// where a scan is ("started", "success", ...), never anything about the face.

#pragma once

#include <QJsonObject>
#include <QList>
#include <QLocalSocket>
#include <QObject>

#include <sys/types.h>

class AgentLink : public QObject
{
    Q_OBJECT
public:
    AgentLink(uid_t uid, QObject *parent);

    void send(const QJsonObject &event);
    // Sends what is still queued, then goes away.
    void finish();

private:
    void flush();

    uid_t m_uid;
    QLocalSocket m_socket;
    QList<QByteArray> m_queue;
    bool m_trusted = false;
    bool m_finishing = false;
};
