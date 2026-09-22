// SPDX-License-Identifier: GPL-3.0-or-later
//
// One connection to the daemon.
//
// The kernel says who is on the other end (SO_PEERCRED), and that is the only
// thing any decision here is based on. Nothing a client writes about itself is
// taken on trust.

#pragma once

#include <QJsonObject>
#include <QLocalSocket>
#include <QObject>

#include <sys/types.h>

class Client : public QObject
{
    Q_OBJECT
public:
    Client(QLocalSocket *socket, QObject *parent);
    ~Client() override;

    uid_t uid() const
    {
        return m_uid;
    }
    pid_t pid() const
    {
        return m_pid;
    }
    bool credentialsKnown() const
    {
        return m_known;
    }

    void send(const QJsonObject &message);
    void close();

    // What this connection is for, once it has said so. A connection makes
    // one request; "watch", "verify" and "enroll" keep it open.
    QString role;

Q_SIGNALS:
    void request(Client *client, const QJsonObject &message);
    void disconnected(Client *client);

private:
    void readLines();

    QLocalSocket *m_socket;
    QByteArray m_buffer;
    uid_t m_uid = uid_t(-1);
    pid_t m_pid = 0;
    bool m_known = false;
    bool m_gone = false;
};
