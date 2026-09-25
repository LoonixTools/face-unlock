// SPDX-License-Identifier: GPL-3.0-or-later
//
// The protocol: one JSON object per line, both ways. A client sends one
// request; the daemon answers with events and ends with one whose "event" is
// "result". "watch" never ends.
//
//   hello                       version
//   status [user]               faces, lockout, camera, settings
//   cameras                     every camera and which one is in use
//   verify user purpose [verbose]
//                               scan for that user. Events: started, face,
//                               hint, frame (verbose only), result
//   enroll [name]               set up a face for the caller. Asks polkit
//                               first. Events: authorizing, authorized,
//                               started, frame, pose, hint, captured, result.
//                               While it runs the client may send "finish"
//                               (stop once enough is done) or "cancel".
//   list [user]                 the caller's faces
//   remove id | rename id name | enable id | disable id | clear
//                               change the caller's faces (polkit)
//   watch                       every scan for the caller, as it happens,
//                               for the bubble
//   unlocked                    the caller's session was unlocked some other
//                               way, which ends a lockout
//   cancel                      stop this connection's scan or setup
//
// Who may do what:
//   - anybody may ask about themselves and scan for themselves. The answer
//     to a scan is yes or no, which tells a process nothing it could use.
//   - root may do all of it for anybody. That is sudo and the polkit helper,
//     through the PAM module.
//   - changing faces needs polkit on top, because a face is a way in.

#include "server.h"

#include "agentlink.h"
#include "camera.h"
#include "client.h"
#include "enrolljob.h"
#include "polkit.h"
#include "scanjob.h"
#include "store.h"
#include "system.h"
#include "userstate.h"

#include "buildconfig.h"

#include <QCoreApplication>
#include <QDateTime>
#include <QDir>
#include <QFile>
#include <QFileInfo>
#include <QJsonArray>
#include <QLocalSocket>
#include <QSocketNotifier>

#include <fcntl.h>
#include <sys/socket.h>
#include <unistd.h>

namespace
{
QJsonObject result(bool ok, const QString &reason = {})
{
    QJsonObject o{{QStringLiteral("event"), QStringLiteral("result")}, {QStringLiteral("ok"), ok}};
    if (!reason.isEmpty()) {
        o.insert(QStringLiteral("reason"), reason);
    }
    return o;
}

// A face unlock and the lock screen going away this soon after belong
// together.
constexpr qint64 UnlockPairSeconds = 30;

bool isFailureThatCounts(const QString &reason)
{
    // A face that did not match, a fake, or a match that never showed a sign
    // of life. An empty chair, a broken camera, or somebody looking away do
    // not count: none of them is anybody trying to get in.
    return reason == u"mismatch" || reason == u"spoof" || reason == u"liveness";
}
} // namespace

Server::Server(const ServerOptions &options, QObject *parent)
    : QObject(parent)
    , m_options(options)
{
    m_idle.setSingleShot(true);
    connect(&m_idle, &QTimer::timeout, this, [] {
        qInfo("idle, exiting");
        QCoreApplication::quit();
    });
}

Server::~Server()
{
    if (m_job) {
        m_job->cancel();
    }
    if (m_jobThread) {
        m_jobThread->wait();
    }
}

bool Server::start(QString *error)
{
    QDir().mkpath(m_options.stateDir);
    QFile::setPermissions(m_options.stateDir, QFileDevice::ReadOwner | QFileDevice::WriteOwner | QFileDevice::ExeOwner);

    if (m_options.systemdFd >= 0) {
        // Accepted here, not handed to QLocalServer: that deletes the socket
        // file when it closes, and the file is systemd's. Gone, nothing could
        // start the daemon again after it exits idle.
        const int fd = m_options.systemdFd;
        ::fcntl(fd, F_SETFD, FD_CLOEXEC);
        ::fcntl(fd, F_SETFL, ::fcntl(fd, F_GETFL) | O_NONBLOCK);
        auto *notifier = new QSocketNotifier(fd, QSocketNotifier::Read, this);
        connect(notifier, &QSocketNotifier::activated, this, &Server::onSystemdConnection);
    } else {
        QDir().mkpath(QFileInfo(m_options.socketPath).absolutePath());
        QLocalServer::removeServer(m_options.socketPath);
        m_server.setSocketOptions(QLocalServer::WorldAccessOption);
        if (!m_server.listen(m_options.socketPath)) {
            *error = m_server.errorString();
            return false;
        }
        connect(&m_server, &QLocalServer::newConnection, this, &Server::onConnection);
    }

    // Loaded up front: whoever started the daemon is about to ask for a scan.
    QString visionError;
    if (!ensureVision(&visionError)) {
        qWarning("%s", qPrintable(visionError));
    }

    updateIdle();
    return true;
}

bool Server::ensureVision(QString *error)
{
    if (!m_visionLoaded) {
        m_visionLoaded = m_vision.load(m_options.modelDir, error);
    }
    return m_visionLoaded;
}

Settings Server::settings() const
{
    return Settings::load(m_options.configPath);
}

void Server::onConnection()
{
    while (QLocalSocket *socket = m_server.nextPendingConnection()) {
        addClient(socket);
    }
    updateIdle();
}

void Server::onSystemdConnection()
{
    int fd;
    while ((fd = ::accept4(m_options.systemdFd, nullptr, nullptr, SOCK_CLOEXEC | SOCK_NONBLOCK)) >= 0) {
        auto *socket = new QLocalSocket;
        if (!socket->setSocketDescriptor(fd)) {
            ::close(fd);
            delete socket;
            continue;
        }
        addClient(socket);
    }
    updateIdle();
}

void Server::addClient(QLocalSocket *socket)
{
    auto *client = new Client(socket, this);
    if (!client->credentialsKnown()) {
        client->deleteLater();
        return;
    }
    connect(client, &Client::request, this, &Server::onRequest);
    connect(client, &Client::disconnected, this, &Server::onDisconnected);
    m_clients.append(client);
}

void Server::onDisconnected(Client *client)
{
    // Whoever asked for a scan has given up on it (the PAM module timed out,
    // the password was typed, the setup window was closed). The camera goes
    // off now rather than when the scan would have ended.
    if (m_job && m_jobOwner == client) {
        m_job->cancel();
    }
    m_clients.removeAll(client);
    client->deleteLater();
    updateIdle();
}

void Server::reply(Client *client, QJsonObject message, bool close)
{
    client->send(message);
    if (close) {
        client->close();
    }
}

void Server::fail(Client *client, const QString &reason, const QJsonObject &extra)
{
    QJsonObject o = extra;
    o.insert(QStringLiteral("event"), QStringLiteral("result"));
    o.insert(QStringLiteral("ok"), false);
    o.insert(QStringLiteral("reason"), reason);
    reply(client, o);
}

bool Server::targetUser(Client *client, const QJsonObject &request, uid_t *uid)
{
    const QString name = request.value(u"user").toString();
    if (name.isEmpty()) {
        *uid = client->uid();
        return true;
    }
    const std::optional<uid_t> target = System::uidOf(name);
    if (!target) {
        fail(client, QStringLiteral("unknown-user"));
        return false;
    }
    if (client->uid() != 0 && client->uid() != *target) {
        fail(client, QStringLiteral("denied"));
        return false;
    }
    *uid = *target;
    return true;
}

void Server::authorize(Client *client, std::function<void()> then)
{
    if (client->uid() == 0 || !m_options.askPolkit) {
        then();
        return;
    }
    client->send({{QStringLiteral("event"), QStringLiteral("authorizing")}});
    QPointer<Client> guard(client);
    ++m_manageChecks;
    Polkit::checkAuthorization(client->pid(), client->uid(), Polkit::ManageAction, this, [this, guard, then](bool ok) {
        --m_manageChecks;
        if (!guard) {
            return;
        }
        if (!ok) {
            fail(guard, QStringLiteral("denied"));
            return;
        }
        guard->send({{QStringLiteral("event"), QStringLiteral("authorized")}});
        then();
    });
}

void Server::onRequest(Client *client, const QJsonObject &request)
{
    const QString cmd = request.value(u"cmd").toString();

    // Messages for a scan or setup that is already running on this
    // connection.
    if (cmd == u"cancel" || cmd == u"finish") {
        if (m_job && m_jobOwner == client) {
            if (cmd == u"cancel") {
                m_job->cancel();
            } else if (auto *enroll = qobject_cast<EnrollJob *>(m_job)) {
                enroll->finishEarly();
            }
        }
        return;
    }
    if (!client->role.isEmpty()) {
        // One request per connection.
        return;
    }
    client->role = cmd;

    if (cmd == u"hello") {
        reply(client, {{QStringLiteral("event"), QStringLiteral("result")}, {QStringLiteral("ok"), true}, {QStringLiteral("version"), QStringLiteral(FU_VERSION)}});
    } else if (cmd == u"status") {
        handleStatus(client);
    } else if (cmd == u"cameras") {
        handleCameras(client);
    } else if (cmd == u"verify") {
        handleVerify(client, request);
    } else if (cmd == u"enroll") {
        handleEnroll(client, request);
    } else if (cmd == u"list") {
        handleList(client, request);
    } else if (cmd == u"remove" || cmd == u"rename" || cmd == u"enable" || cmd == u"disable" || cmd == u"clear") {
        handleChange(client, request);
    } else if (cmd == u"watch") {
        handleWatch(client);
    } else if (cmd == u"unlocked") {
        handleUnlocked(client);
    } else {
        fail(client, QStringLiteral("bad-request"));
    }
}

// ---------------------------------------------------------------------------
// Questions
// ---------------------------------------------------------------------------

void Server::handleStatus(Client *client)
{
    const uid_t uid = client->uid();
    const Settings s = settings();
    const FaceStore store(m_options.stateDir);
    const QList<Identity> faces = store.load(uid);
    const UserState state = UserState::load(m_options.stateDir, uid);
    const qint64 now = QDateTime::currentSecsSinceEpoch();

    int enabled = 0;
    for (const Identity &id : faces) {
        enabled += id.enabled;
    }

    QString path = s.camera;
    if (path.isEmpty() || path == u"auto") {
        path = Camera::autoPath();
    }
    QString cameraName;
    bool present = false;
    if (path.startsWith(u"file:") || path.startsWith(u"images:")) {
        cameraName = path;
        present = true;
    } else {
        for (const CameraInfo &c : Camera::list()) {
            if (c.path == path) {
                cameraName = c.name;
                present = true;
            }
        }
    }

    QString modelError;
    reply(client,
          {{QStringLiteral("event"), QStringLiteral("result")},
           {QStringLiteral("ok"), true},
           {QStringLiteral("version"), QStringLiteral(FU_VERSION)},
           {QStringLiteral("user"), System::nameOf(uid)},
           {QStringLiteral("faces"), enabled},
           {QStringLiteral("identities"), int(faces.size())},
           {QStringLiteral("lockout"), state.lockoutLeft(now)},
           {QStringLiteral("lastUnlock"), state.lastUnlock},
           {QStringLiteral("lastPurpose"), state.lastPurpose},
           {QStringLiteral("camera"), s.camera},
           {QStringLiteral("cameraPath"), path},
           {QStringLiteral("cameraName"), cameraName},
           {QStringLiteral("cameraPresent"), present},
           {QStringLiteral("models"), ensureVision(&modelError)},
           {QStringLiteral("liveness"), Settings::livenessName(s.liveness)},
           {QStringLiteral("strictness"), Settings::strictnessName(s.strictness)},
           {QStringLiteral("attention"), s.attention},
           {QStringLiteral("scanSeconds"), s.scanSeconds},
           {QStringLiteral("busy"), m_job != nullptr}});
}

void Server::handleCameras(Client *client)
{
    QJsonArray list;
    for (const CameraInfo &c : Camera::list()) {
        list.append(QJsonObject{{QStringLiteral("path"), c.path}, {QStringLiteral("name"), c.name}, {QStringLiteral("infrared"), c.infrared}});
    }
    QJsonObject o = result(true);
    o.insert(QStringLiteral("cameras"), list);
    o.insert(QStringLiteral("selected"), settings().camera);
    o.insert(QStringLiteral("auto"), Camera::autoPath());
    reply(client, o);
}

void Server::handleList(Client *client, const QJsonObject &request)
{
    uid_t uid;
    if (!targetUser(client, request, &uid)) {
        return;
    }
    const QList<Identity> faces = FaceStore(m_options.stateDir).load(uid);
    QJsonArray list;
    for (const Identity &id : faces) {
        list.append(QJsonObject{
            {QStringLiteral("id"), id.id},
            {QStringLiteral("name"), id.name},
            {QStringLiteral("created"), double(id.created)},
            {QStringLiteral("enabled"), id.enabled},
            {QStringLiteral("samples"), int(id.samples.size()) - id.adaptiveCount()},
            {QStringLiteral("adaptive"), id.adaptiveCount()},
        });
    }
    QJsonObject o = result(true);
    o.insert(QStringLiteral("faces"), list);
    reply(client, o);
}

void Server::handleWatch(Client *client)
{
    reply(client, {{QStringLiteral("event"), QStringLiteral("watching")}}, false);
}

void Server::handleUnlocked(Client *client)
{
    UserState state = UserState::load(m_options.stateDir, client->uid());

    // Our unlock goes through logind. The lock screen's password prompt is
    // cut off mid-question by it and counts that as a wrong password, so a
    // few face unlocks would lock the account (pam_faillock). Taken back here.
    const qint64 now = QDateTime::currentSecsSinceEpoch();
    if (geteuid() == 0 && state.lastPurpose == u"unlock" && now - state.lastUnlock <= UnlockPairSeconds) {
        System::resetFailedLogins(client->uid());
    }

    if (state.failures || state.lockedUntil) {
        state.failures = 0;
        state.lockedUntil = 0;
        state.save(m_options.stateDir, client->uid());
    }
    reply(client, result(true));
}

// ---------------------------------------------------------------------------
// Scanning
// ---------------------------------------------------------------------------

void Server::handleVerify(Client *client, const QJsonObject &request)
{
    uid_t uid;
    if (!targetUser(client, request, &uid)) {
        return;
    }
    if (m_job) {
        fail(client, QStringLiteral("busy"));
        return;
    }

    const Settings s = settings();
    const qint64 now = QDateTime::currentSecsSinceEpoch();
    const UserState state = UserState::load(m_options.stateDir, uid);
    if (const qint64 left = state.lockoutLeft(now)) {
        fail(client, QStringLiteral("lockout"), {{QStringLiteral("seconds"), left}});
        return;
    }
    if (s.skipLidClosed && System::lidClosed()) {
        fail(client, QStringLiteral("lid-closed"));
        return;
    }

    QList<Identity> faces = FaceStore(m_options.stateDir).load(uid);
    faces.erase(std::remove_if(faces.begin(), faces.end(), [](const Identity &id) {
                    return !id.enabled;
                }),
                faces.end());
    if (faces.isEmpty()) {
        fail(client, QStringLiteral("not-enrolled"));
        return;
    }

    QString error;
    if (!ensureVision(&error)) {
        fail(client, QStringLiteral("models"), {{QStringLiteral("message"), error}});
        return;
    }

    QString purpose = request.value(u"purpose").toString();
    static const QStringList purposes = {QStringLiteral("unlock"), QStringLiteral("sudo"), QStringLiteral("polkit"), QStringLiteral("test")};
    if (!purposes.contains(purpose)) {
        purpose = QStringLiteral("other");
    }
    // The password dialog that is open right now may be the one asking
    // whether faces may be changed. See m_manageChecks.
    if (m_manageChecks > 0 && purpose != u"unlock" && purpose != u"test") {
        fail(client, QStringLiteral("busy"));
        return;
    }

    auto *job = new ScanJob(&m_vision, uid, s, faces, purpose, request.value(u"verbose").toBool());
    // The lock screen's agent draws its own scans. Everybody else's it has
    // to be told about.
    if (purpose != u"unlock") {
        m_agentLink = new AgentLink(uid, this);
    }
    broadcast(uid, {{QStringLiteral("event"), QStringLiteral("scan")}, {QStringLiteral("state"), QStringLiteral("start")}, {QStringLiteral("purpose"), purpose}});
    startJob(job, client);
}

void Server::handleEnroll(Client *client, const QJsonObject &request)
{
    uid_t uid;
    if (!targetUser(client, request, &uid)) {
        return;
    }
    if (m_job) {
        fail(client, QStringLiteral("busy"));
        return;
    }

    QString name = request.value(u"name").toString().trimmed().left(64);
    authorize(client, [this, client, uid, name]() mutable {
        if (m_job) {
            fail(client, QStringLiteral("busy"));
            return;
        }
        QString error;
        if (!ensureVision(&error)) {
            fail(client, QStringLiteral("models"), {{QStringLiteral("message"), error}});
            return;
        }
        if (name.isEmpty()) {
            name = System::nameOf(uid);
        }
        startJob(new EnrollJob(&m_vision, uid, settings(), name), client);
    });
}

void Server::startJob(Job *job, Client *owner)
{
    m_job = job;
    m_jobOwner = owner;
    connect(job, &Job::event, this, &Server::onJobEvent, Qt::QueuedConnection);

    m_jobThread = QThread::create([job] {
        job->run();
    });
    connect(m_jobThread, &QThread::finished, this, &Server::onJobDone, Qt::QueuedConnection);
    m_jobThread->start();
    updateIdle();
}

void Server::onJobEvent(const QJsonObject &event)
{
    if (!m_job) {
        return;
    }
    if (m_jobOwner) {
        m_jobOwner->send(event);
    }

    // The bubble hears about scans, not about what the setup window sees.
    if (auto *scan = qobject_cast<ScanJob *>(m_job)) {
        const QString what = event.value(u"event").toString();
        if (what == u"face" || what == u"hint") {
            QJsonObject e{{QStringLiteral("event"), QStringLiteral("scan")},
                          {QStringLiteral("state"), what},
                          {QStringLiteral("purpose"), scan->purpose()}};
            if (what == u"hint") {
                e.insert(QStringLiteral("hint"), event.value(u"hint"));
            }
            broadcast(scan->uid(), e);
        }
    }
}

void Server::onJobDone()
{
    Job *job = m_job;
    m_job = nullptr;
    m_jobThread->deleteLater();
    m_jobThread = nullptr;
    QPointer<Client> owner = m_jobOwner;
    m_jobOwner = nullptr;

    QJsonObject res = job->result;
    const qint64 now = QDateTime::currentSecsSinceEpoch();

    if (auto *scan = qobject_cast<ScanJob *>(job)) {
        const Settings s = settings();
        UserState state = UserState::load(m_options.stateDir, scan->uid());
        const QString reason = res.value(u"reason").toString();

        if (res.value(u"ok").toBool()) {
            state.failures = 0;
            state.lockedUntil = 0;
            state.lastUnlock = now;
            state.lastPurpose = scan->purpose();

            // Learn from a confident match, the way Face ID keeps up with a
            // beard growing in. Only well clear of the threshold, so the
            // samples cannot creep towards somebody else one borderline
            // unlock at a time.
            if (s.adapt && scan->matchedScore >= s.threshold() + 0.08 && !scan->matchedEmbedding.empty()) {
                const FaceStore store(m_options.stateDir);
                QList<Identity> faces = store.load(scan->uid());
                const QString id = res.value(u"id").toString();
                for (Identity &identity : faces) {
                    if (identity.id == id && FaceStore::adapt(identity, scan->matchedEmbedding, now)) {
                        QString error;
                        if (!store.save(scan->uid(), faces, &error)) {
                            qWarning("could not save what was learned: %s", qPrintable(error));
                        }
                        break;
                    }
                }
            }
        } else if (isFailureThatCounts(reason)) {
            if (++state.failures >= s.maxFailures) {
                state.failures = 0;
                state.lockedUntil = now + qint64(s.lockoutMinutes) * 60;
                res.insert(QStringLiteral("lockout"), state.lockoutLeft(now));
            }
        }
        state.save(m_options.stateDir, scan->uid());

        QJsonObject e{{QStringLiteral("event"), QStringLiteral("scan")},
                      {QStringLiteral("state"), res.value(u"ok").toBool() ? QStringLiteral("success") : QStringLiteral("failure")},
                      {QStringLiteral("reason"), reason},
                      {QStringLiteral("purpose"), scan->purpose()}};
        if (res.contains(u"lockout")) {
            e.insert(QStringLiteral("lockout"), res.value(u"lockout"));
        }
        broadcast(scan->uid(), e);

        qInfo("scan for %s (%s): %s%s", qPrintable(System::nameOf(scan->uid())), qPrintable(scan->purpose()),
              res.value(u"ok").toBool() ? "recognised" : "not recognised, ", res.value(u"ok").toBool() ? "" : qPrintable(reason));
    } else if (auto *enroll = qobject_cast<EnrollJob *>(job)) {
        if (res.value(u"ok").toBool()) {
            const FaceStore store(m_options.stateDir);
            QList<Identity> faces = store.load(enroll->uid());
            faces.append(enroll->identity);
            QString error;
            if (!store.save(enroll->uid(), faces, &error)) {
                res = result(false, QStringLiteral("store"));
                res.insert(QStringLiteral("message"), error);
            } else {
                qInfo("new face \"%s\" for %s", qPrintable(enroll->identity.name), qPrintable(System::nameOf(enroll->uid())));
            }
        }
    }

    if (owner) {
        reply(owner, res);
    }
    job->deleteLater();
    updateIdle();
}

void Server::broadcast(uid_t uid, const QJsonObject &event)
{
    if (m_agentLink) {
        m_agentLink->send(event);
        const QString state = event.value(u"state").toString();
        if (state == u"success" || state == u"failure") {
            m_agentLink->finish();
            m_agentLink = nullptr;
        }
    }
    for (Client *c : std::as_const(m_clients)) {
        if (c->role == u"watch" && c->uid() == uid) {
            c->send(event);
        }
    }
}

// ---------------------------------------------------------------------------
// Changing faces
// ---------------------------------------------------------------------------

void Server::handleChange(Client *client, const QJsonObject &request)
{
    uid_t uid;
    if (!targetUser(client, request, &uid)) {
        return;
    }
    const QString cmd = request.value(u"cmd").toString();
    const QString id = request.value(u"id").toString();
    const QString name = request.value(u"name").toString().trimmed().left(64);

    authorize(client, [this, client, uid, cmd, id, name] {
        const FaceStore store(m_options.stateDir);
        QList<Identity> faces = store.load(uid);
        bool found = cmd == u"clear";

        if (cmd == u"clear") {
            faces.clear();
        }
        for (qsizetype i = 0; i < faces.size(); ++i) {
            if (faces[i].id != id) {
                continue;
            }
            found = true;
            if (cmd == u"remove") {
                faces.removeAt(i);
            } else if (cmd == u"rename" && !name.isEmpty()) {
                faces[i].name = name;
            } else if (cmd == u"enable") {
                faces[i].enabled = true;
            } else if (cmd == u"disable") {
                faces[i].enabled = false;
            }
            break;
        }
        if (!found) {
            fail(client, QStringLiteral("unknown-face"));
            return;
        }
        QString error;
        if (!store.save(uid, faces, &error)) {
            fail(client, QStringLiteral("store"), {{QStringLiteral("message"), error}});
            return;
        }
        reply(client, result(true));
    });
}

// ---------------------------------------------------------------------------

void Server::updateIdle()
{
    if (m_options.idleSeconds > 0 && m_clients.isEmpty() && !m_job) {
        m_idle.start(m_options.idleSeconds * 1000);
    } else {
        m_idle.stop();
    }
}
