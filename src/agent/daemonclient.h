// SPDX-License-Identifier: GPL-3.0-or-later
//
// Talking to the daemon: one connection per request, the way the daemon
// wants it. The connection lives as long as the request does, so closing it
// is also how a scan gets cancelled.

#pragma once

#include <QJsonObject>
#include <QLocalSocket>
#include <QObject>

class DaemonRequest : public QObject
{
    Q_OBJECT
public:
    DaemonRequest(const QJsonObject &request, QObject *parent);
    ~DaemonRequest() override;

    // Something for the request that is already running ("cancel",
    // "finish").
    void send(const QJsonObject &message);
    // Stop without waiting for an answer. finished() is not emitted.
    void abort();

    static QString socketPath();

Q_SIGNALS:
    void event(const QJsonObject &event);
    // The last message: the daemon's result, or one made up here when the
    // daemon could not be reached or went away ("unreachable",
    // "disconnected").
    void finished(const QJsonObject &result);

private:
    void readLines();
    void end(const QJsonObject &result);

    QLocalSocket m_socket;
    QByteArray m_buffer;
    QJsonObject m_request;
    bool m_done = false;
};
