// SPDX-License-Identifier: GPL-3.0-or-later

#include "agentlink.h"

#include <QFile>
#include <QJsonDocument>
#include <QTimer>

#include <sys/socket.h>
#include <sys/stat.h>

namespace
{
bool ownedBy(const QString &path, uid_t uid, bool socket)
{
    struct stat st;
    if (::lstat(QFile::encodeName(path).constData(), &st) != 0 || st.st_uid != uid) {
        return false;
    }
    return socket ? S_ISSOCK(st.st_mode) : S_ISDIR(st.st_mode);
}
} // namespace

AgentLink::AgentLink(uid_t uid, QObject *parent)
    : QObject(parent)
    , m_uid(uid)
{
    const QString dir = QStringLiteral("/run/user/%1/face-unlock").arg(uid);
    const QString path = dir + QStringLiteral("/agent.socket");
    if (!ownedBy(dir, uid, false) || !ownedBy(path, uid, true)) {
        // No agent in that session, or not one of this user's making.
        QTimer::singleShot(0, this, &QObject::deleteLater);
        return;
    }

    connect(&m_socket, &QLocalSocket::connected, this, [this] {
        ucred cred{};
        socklen_t len = sizeof(cred);
        if (::getsockopt(int(m_socket.socketDescriptor()), SOL_SOCKET, SO_PEERCRED, &cred, &len) != 0 || cred.uid != m_uid) {
            m_socket.abort();
            deleteLater();
            return;
        }
        m_trusted = true;
        flush();
    });
    connect(&m_socket, &QLocalSocket::errorOccurred, this, [this] {
        deleteLater();
    });
    m_socket.connectToServer(path);

    // However the scan ends, this does not outlive it by much.
    QTimer::singleShot(60 * 1000, this, &QObject::deleteLater);
}

void AgentLink::send(const QJsonObject &event)
{
    m_queue.append(QJsonDocument(event).toJson(QJsonDocument::Compact) + '\n');
    flush();
}

void AgentLink::finish()
{
    m_finishing = true;
    flush();
}

void AgentLink::flush()
{
    if (!m_trusted) {
        return;
    }
    for (const QByteArray &line : std::as_const(m_queue)) {
        m_socket.write(line);
    }
    m_queue.clear();
    m_socket.flush();
    if (m_finishing) {
        m_socket.disconnectFromServer();
        deleteLater();
    }
}
