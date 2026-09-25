// SPDX-License-Identifier: GPL-3.0-or-later

#include "daemonclient.h"

#include "buildconfig.h"

#include <QJsonDocument>

DaemonRequest::DaemonRequest(const QJsonObject &request, QObject *parent)
    : QObject(parent)
    , m_request(request)
{
    connect(&m_socket, &QLocalSocket::connected, this, [this] {
        m_socket.write(QJsonDocument(m_request).toJson(QJsonDocument::Compact) + '\n');
    });
    connect(&m_socket, &QLocalSocket::readyRead, this, &DaemonRequest::readLines);
    connect(&m_socket, &QLocalSocket::errorOccurred, this, [this](QLocalSocket::LocalSocketError error) {
        if (error == QLocalSocket::PeerClosedError) {
            return;
        }
        end({{QStringLiteral("event"), QStringLiteral("result")},
             {QStringLiteral("ok"), false},
             {QStringLiteral("reason"), QStringLiteral("unreachable")},
             {QStringLiteral("message"), m_socket.errorString()}});
    });
    connect(&m_socket, &QLocalSocket::disconnected, this, [this] {
        readLines();
        end({{QStringLiteral("event"), QStringLiteral("result")}, {QStringLiteral("ok"), false}, {QStringLiteral("reason"), QStringLiteral("disconnected")}});
    });
    m_socket.connectToServer(socketPath());
}

DaemonRequest::~DaemonRequest()
{
    m_done = true;
    m_socket.abort();
}

QString DaemonRequest::socketPath()
{
    const QByteArray env = qgetenv("FU_SOCKET");
    return env.isEmpty() ? QStringLiteral(FU_SOCKET) : QString::fromLocal8Bit(env);
}

void DaemonRequest::send(const QJsonObject &message)
{
    if (m_socket.state() == QLocalSocket::ConnectedState) {
        m_socket.write(QJsonDocument(message).toJson(QJsonDocument::Compact) + '\n');
        m_socket.flush();
    }
}

void DaemonRequest::abort()
{
    m_done = true;
    m_socket.abort();
}

void DaemonRequest::readLines()
{
    if (m_socket.isOpen() && m_socket.bytesAvailable() > 0) {
        m_buffer += m_socket.readAll();
    }
    qsizetype nl;
    while (!m_done && (nl = m_buffer.indexOf('\n')) >= 0) {
        const QJsonObject o = QJsonDocument::fromJson(m_buffer.left(nl)).object();
        m_buffer.remove(0, nl + 1);
        if (o.value(u"event").toString() == u"result") {
            end(o);
            return;
        }
        Q_EMIT event(o);
    }
}

void DaemonRequest::end(const QJsonObject &result)
{
    if (m_done) {
        return;
    }
    m_done = true;
    Q_EMIT finished(result);
}
