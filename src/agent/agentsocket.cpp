// SPDX-License-Identifier: GPL-3.0-or-later

#include "agentsocket.h"

#include <QDir>
#include <QFile>
#include <QFileInfo>
#include <QJsonDocument>
#include <QLocalSocket>
#include <QStandardPaths>

#include <sys/socket.h>
#include <unistd.h>

AgentSocket::AgentSocket(QObject *parent)
    : QObject(parent)
{
    connect(&m_server, &QLocalServer::newConnection, this, [this] {
        while (QLocalSocket *socket = m_server.nextPendingConnection()) {
            ucred cred{};
            socklen_t len = sizeof(cred);
            if (::getsockopt(int(socket->socketDescriptor()), SOL_SOCKET, SO_PEERCRED, &cred, &len) != 0
                || (cred.uid != 0 && cred.uid != ::getuid())) {
                socket->abort();
                socket->deleteLater();
                continue;
            }
            auto buffer = std::make_shared<QByteArray>();
            connect(socket, &QLocalSocket::readyRead, this, [this, socket, buffer, cred] {
                *buffer += socket->readAll();
                if (buffer->size() > 64 * 1024) {
                    socket->abort();
                    return;
                }
                qsizetype nl;
                while ((nl = buffer->indexOf('\n')) >= 0) {
                    const QJsonObject o = QJsonDocument::fromJson(buffer->left(nl)).object();
                    buffer->remove(0, nl + 1);
                    const QString what = o.value(u"event").toString();
                    if (what == u"scan") {
                        Q_EMIT scanEvent(o);
                    } else if (what == u"lock" && cred.uid == ::getuid()) {
                        m_waiting.append(socket);
                        Q_EMIT lockRequested();
                    }
                }
            });
            connect(socket, &QLocalSocket::disconnected, socket, &QObject::deleteLater);
        }
    });
}

QString AgentSocket::path()
{
    return QStandardPaths::writableLocation(QStandardPaths::RuntimeLocation) + QStringLiteral("/face-unlock/agent.socket");
}

bool AgentSocket::running()
{
    QLocalSocket probe;
    probe.connectToServer(path());
    return probe.waitForConnected(500);
}

bool AgentSocket::requestLock()
{
    QLocalSocket socket;
    socket.connectToServer(path());
    if (!socket.waitForConnected(500)) {
        return false;
    }
    socket.write(QJsonDocument(QJsonObject{{QStringLiteral("event"), QStringLiteral("lock")}}).toJson(QJsonDocument::Compact) + '\n');
    if (!socket.waitForBytesWritten(500)) {
        return false;
    }
    // The answer comes once the compositor has the lock, so that an idle
    // daemon that locks before sleep does not suspend an unlocked screen.
    QByteArray answer;
    while (!answer.contains('\n') && socket.waitForReadyRead(5000)) {
        answer += socket.readAll();
    }
    return QJsonDocument::fromJson(answer.left(answer.indexOf('\n'))).object().value(u"event").toString() == u"locked";
}

void AgentSocket::confirmLock()
{
    for (const QPointer<QLocalSocket> &socket : std::as_const(m_waiting)) {
        if (socket) {
            socket->write(QJsonDocument(QJsonObject{{QStringLiteral("event"), QStringLiteral("locked")}}).toJson(QJsonDocument::Compact) + '\n');
            socket->flush();
        }
    }
    m_waiting.clear();
}

bool AgentSocket::listen()
{
    const QString p = path();
    QDir().mkpath(QFileInfo(p).absolutePath());
    QFile::setPermissions(QFileInfo(p).absolutePath(), QFileDevice::ReadOwner | QFileDevice::WriteOwner | QFileDevice::ExeOwner);
    QLocalServer::removeServer(p);
    // Anybody may write to the socket itself, because the directory around it
    // lets nobody but this user in. The daemon gets through that directory with
    // CAP_DAC_READ_SEARCH, which allows passing through but not writing to
    // something that is not open to others. Who is on the other end is checked
    // on every connection anyway.
    m_server.setSocketOptions(QLocalServer::WorldAccessOption);
    if (!m_server.listen(p)) {
        qWarning("cannot listen on %s: %s", qPrintable(p), qPrintable(m_server.errorString()));
        return false;
    }
    return true;
}
