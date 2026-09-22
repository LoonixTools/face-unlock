// SPDX-License-Identifier: GPL-3.0-or-later
//
// The socket, and who may ask for what.

#pragma once

#include "settings.h"
#include "vision.h"

#include <QJsonObject>
#include <QList>
#include <QLocalServer>
#include <QObject>
#include <QPointer>
#include <QThread>
#include <QTimer>

#include <sys/types.h>

class AgentLink;
class Client;
class Job;

struct ServerOptions {
    QString socketPath;
    int systemdFd = -1;
    QString stateDir;
    QString configPath;
    QString modelDir;
    // Exit after this long with nothing to do. 0 stays up.
    int idleSeconds = 0;
    // Running as somebody other than root is only ever development, and only
    // touches that person's own test data. polkit is not asked then.
    bool askPolkit = true;
};

class Server : public QObject
{
    Q_OBJECT
public:
    explicit Server(const ServerOptions &options, QObject *parent = nullptr);
    ~Server() override;

    bool start(QString *error);

private:
    void onConnection();
    void onSystemdConnection();
    void addClient(QLocalSocket *socket);
    void onRequest(Client *client, const QJsonObject &request);
    void onDisconnected(Client *client);

    void handleStatus(Client *client);
    void handleCameras(Client *client);
    void handleVerify(Client *client, const QJsonObject &request);
    void handleEnroll(Client *client, const QJsonObject &request);
    void handleList(Client *client, const QJsonObject &request);
    void handleChange(Client *client, const QJsonObject &request);
    void handleWatch(Client *client);
    void handleUnlocked(Client *client);

    // The user a request is about: the caller, or for root whoever it names.
    // Returns false (and answers) when the caller may not act for them.
    bool targetUser(Client *client, const QJsonObject &request, uid_t *uid);
    void authorize(Client *client, std::function<void()> then);
    void reply(Client *client, QJsonObject message, bool close = true);
    void fail(Client *client, const QString &reason, const QJsonObject &extra = {});

    bool ensureVision(QString *error);
    Settings settings() const;

    void startJob(Job *job, Client *owner);
    void onJobEvent(const QJsonObject &event);
    void onJobDone();
    void broadcast(uid_t uid, const QJsonObject &event);

    void updateIdle();

    ServerOptions m_options;
    QLocalServer m_server;
    QList<Client *> m_clients;
    Vision m_vision;
    bool m_visionLoaded = false;

    Job *m_job = nullptr;
    QThread *m_jobThread = nullptr;
    QPointer<Client> m_jobOwner;
    // The session to tell about the running scan, when it is not the lock
    // screen's own (see agentlink.h).
    QPointer<AgentLink> m_agentLink;

    QTimer m_idle;

    // Asking polkit whether faces may be changed. While that question is
    // open, a scan for an admin prompt is refused: the answer to "may a face
    // be added" has to be the password, not a face.
    int m_manageChecks = 0;
};
