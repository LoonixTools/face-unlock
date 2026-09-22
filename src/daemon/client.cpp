// SPDX-License-Identifier: GPL-3.0-or-later

#include "client.h"

#include <QJsonDocument>

#include <sys/socket.h>

namespace
{
// A request is a line of JSON a few hundred bytes long. Anything that keeps
// talking without a newline for this long is not a client.
constexpr qsizetype MaxLine = 64 * 1024;
} // namespace

Client::Client(QLocalSocket *socket, QObject *parent)
    : QObject(parent)
    , m_socket(socket)
{
    m_socket->setParent(this);

    ucred cred{};
    socklen_t len = sizeof(cred);
    if (::getsockopt(int(m_socket->socketDescriptor()), SOL_SOCKET, SO_PEERCRED, &cred, &len) == 0) {
        m_uid = cred.uid;
        m_pid = cred.pid;
        m_known = true;
    }

    connect(m_socket, &QLocalSocket::readyRead, this, &Client::readLines);
    connect(m_socket, &QLocalSocket::disconnected, this, [this] {
        if (!m_gone) {
            m_gone = true;
            Q_EMIT disconnected(this);
        }
    });
}

Client::~Client() = default;

void Client::readLines()
{
    m_buffer += m_socket->readAll();
    if (m_buffer.size() > MaxLine && !m_buffer.contains('\n')) {
        close();
        return;
    }

    qsizetype nl;
    while ((nl = m_buffer.indexOf('\n')) >= 0) {
        const QByteArray line = m_buffer.left(nl).trimmed();
        m_buffer.remove(0, nl + 1);
        if (line.isEmpty()) {
            continue;
        }
        const QJsonDocument doc = QJsonDocument::fromJson(line);
        if (!doc.isObject()) {
            send({{QStringLiteral("event"), QStringLiteral("result")},
                  {QStringLiteral("ok"), false},
                  {QStringLiteral("reason"), QStringLiteral("bad-request")}});
            continue;
        }
        Q_EMIT request(this, doc.object());
    }
}

void Client::send(const QJsonObject &message)
{
    if (m_gone || m_socket->state() != QLocalSocket::ConnectedState) {
        return;
    }
    m_socket->write(QJsonDocument(message).toJson(QJsonDocument::Compact) + '\n');
    m_socket->flush();
}

void Client::close()
{
    if (m_socket->state() == QLocalSocket::ConnectedState) {
        m_socket->flush();
        m_socket->disconnectFromServer();
    }
}
