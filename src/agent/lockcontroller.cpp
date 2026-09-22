// SPDX-License-Identifier: GPL-3.0-or-later

#include "lockcontroller.h"

#include "bubblecontroller.h"
#include "daemonclient.h"
#include "userconfig.h"

#include <KIdleTime>

#include <QDBusConnection>
#include <QDBusMessage>
#include <QDBusPendingCallWatcher>
#include <QDBusPendingReply>
#include <QJsonObject>
#include <QProcess>

namespace
{
// Locking with the keyboard is itself a key press, and the keys come back up
// a moment later. Input in this window after locking is not somebody coming
// back.
constexpr int GraceAfterLockMs = 1500;
// After a scan that did not get anybody in, the next one waits until the
// person has kept still for this long and then touched something again. That
// way typing the password does not start a scan with every key.
constexpr int CalmBeforeRetryMs = 2000;
// Show the tick before the screen goes: long enough to see, short enough
// not to feel like waiting.
constexpr int UnlockAfterSuccessMs = 450;
// A camera needs a moment after the machine wakes before it delivers frames.
constexpr int ScanAfterWakeMs = 1000;
} // namespace

LockController::LockController(BubbleController *bubble, UserConfig *config, QObject *parent)
    : QObject(parent)
    , m_bubble(bubble)
    , m_config(config)
{
    m_armTimer.setSingleShot(true);
    connect(&m_armTimer, &QTimer::timeout, this, &LockController::arm);

    QDBusConnection session = QDBusConnection::sessionBus();
    session.connect(QStringLiteral("org.freedesktop.ScreenSaver"),
                    QStringLiteral("/ScreenSaver"),
                    QStringLiteral("org.freedesktop.ScreenSaver"),
                    QStringLiteral("ActiveChanged"),
                    this,
                    SLOT(onActiveChanged(bool)));

    QDBusConnection::systemBus().connect(QStringLiteral("org.freedesktop.login1"),
                                         QStringLiteral("/org/freedesktop/login1"),
                                         QStringLiteral("org.freedesktop.login1.Manager"),
                                         QStringLiteral("PrepareForSleep"),
                                         this,
                                         SLOT(onPrepareForSleep(bool)));

    KIdleTime *idle = KIdleTime::instance();
    connect(idle, &KIdleTime::resumingFromIdle, this, &LockController::onResume);
    connect(idle, qOverload<int, int>(&KIdleTime::timeoutReached), this, [this, idle](int id, int) {
        if (id != m_idleId) {
            return;
        }
        idle->removeIdleTimeout(id);
        m_idleId = -1;
        arm();
    });

    // Started while the screen is already locked (the agent restarted).
    const QDBusMessage get = QDBusMessage::createMethodCall(QStringLiteral("org.freedesktop.ScreenSaver"),
                                                            QStringLiteral("/ScreenSaver"),
                                                            QStringLiteral("org.freedesktop.ScreenSaver"),
                                                            QStringLiteral("GetActive"));
    auto *watcher = new QDBusPendingCallWatcher(session.asyncCall(get), this);
    connect(watcher, &QDBusPendingCallWatcher::finished, this, [this, watcher] {
        watcher->deleteLater();
        const QDBusPendingReply<bool> reply = *watcher;
        if (reply.isValid() && reply.value()) {
            onActiveChanged(true);
        }
    });
}

void LockController::onActiveChanged(bool active)
{
    if (active == m_locked) {
        return;
    }
    KIdleTime *idle = KIdleTime::instance();

    if (!active) {
        m_locked = false;
        m_armTimer.stop();
        if (m_idleId >= 0) {
            idle->removeIdleTimeout(m_idleId);
            m_idleId = -1;
        }
        idle->stopCatchingResumeEvent();
        if (m_scan) {
            m_scan->abort();
            m_scan->deleteLater();
            m_scan = nullptr;
        }
        // The tick of our own unlock is still playing; anything else goes.
        if (m_bubble->phase() == u"scanning") {
            m_bubble->dismiss();
        }
        // However it was unlocked, it was the right person: a lockout after
        // failed scans ends here, the way a phone takes its code.
        auto *done = new DaemonRequest({{QStringLiteral("cmd"), QStringLiteral("unlocked")}}, this);
        connect(done, &DaemonRequest::finished, done, &QObject::deleteLater);
        return;
    }

    if (!m_config->lockScreen()) {
        return;
    }
    m_locked = true;
    m_stopped = false;
    m_lockedFor.start();
    warmUp();

    if (m_config->scanOnLock()) {
        QTimer::singleShot(400, this, [this] {
            startScan(QStringLiteral("lock"));
        });
    } else {
        m_armTimer.start(GraceAfterLockMs);
    }
}

void LockController::onPrepareForSleep(bool sleeping)
{
    if (sleeping) {
        if (m_scan) {
            m_scan->abort();
            m_scan->deleteLater();
            m_scan = nullptr;
            m_bubble->dismiss();
        }
        return;
    }
    // Waking up is somebody coming back, lid or no lid.
    if (m_locked && !m_stopped && m_config->scanOnWake()) {
        QTimer::singleShot(ScanAfterWakeMs, this, [this] {
            startScan(QStringLiteral("resume"));
        });
    }
}

void LockController::arm()
{
    if (!m_locked || m_stopped || !m_config->scanOnWake()) {
        return;
    }
    KIdleTime::instance()->catchNextResumeEvent();
}

void LockController::onResume()
{
    if (m_locked && !m_scan) {
        startScan(QStringLiteral("wake"));
    }
}

void LockController::warmUp()
{
    // The daemon is started by its socket and loads the networks when it
    // starts. Doing that now, while nobody is waiting, takes it off the first
    // scan.
    auto *hello = new DaemonRequest({{QStringLiteral("cmd"), QStringLiteral("hello")}}, this);
    connect(hello, &DaemonRequest::finished, hello, &QObject::deleteLater);
}

void LockController::startScan(const QString &why)
{
    if (!m_locked || m_scan || m_stopped) {
        return;
    }
    qInfo("scanning (%s)", qPrintable(why));
    m_bubble->scanStarted();

    m_scan = new DaemonRequest({{QStringLiteral("cmd"), QStringLiteral("verify")}, {QStringLiteral("purpose"), QStringLiteral("unlock")}}, this);
    connect(m_scan, &DaemonRequest::event, this, [this](const QJsonObject &e) {
        const QString what = e.value(u"event").toString();
        if (what == u"face") {
            m_bubble->faceFound();
        } else if (what == u"hint") {
            m_bubble->hint(e.value(u"hint").toString());
        }
    });
    connect(m_scan, &DaemonRequest::finished, this, &LockController::onScanFinished);
}

void LockController::onScanFinished(const QJsonObject &result)
{
    if (m_scan) {
        m_scan->deleteLater();
        m_scan = nullptr;
    }
    if (!m_locked) {
        return;
    }

    const QString reason = result.value(u"reason").toString();
    if (result.value(u"ok").toBool()) {
        m_bubble->succeeded();
        QTimer::singleShot(UnlockAfterSuccessMs, this, &LockController::unlock);
        return;
    }

    m_bubble->failed(reason, qint64(result.value(u"lockout").toDouble()));

    if (reason == u"lockout" || result.contains(u"lockout") || reason == u"not-enrolled" || reason == u"models" || reason == u"denied"
        || reason == u"unreachable") {
        // Nothing another scan could change before the next lock.
        m_stopped = true;
        return;
    }
    if (reason == u"busy") {
        m_armTimer.start(GraceAfterLockMs);
        return;
    }
    if (m_idleId < 0) {
        m_idleId = KIdleTime::instance()->addIdleTimeout(CalmBeforeRetryMs);
    }
}

void LockController::unlock()
{
    if (!m_locked) {
        return;
    }
    // "auto" is the caller's own session, or for a program outside any
    // session (this one runs as a user service) the session on the display.
    const QDBusMessage call = QDBusMessage::createMethodCall(QStringLiteral("org.freedesktop.login1"),
                                                             QStringLiteral("/org/freedesktop/login1/session/auto"),
                                                             QStringLiteral("org.freedesktop.login1.Session"),
                                                             QStringLiteral("Unlock"));
    auto *watcher = new QDBusPendingCallWatcher(QDBusConnection::systemBus().asyncCall(call), this);
    connect(watcher, &QDBusPendingCallWatcher::finished, this, [watcher] {
        watcher->deleteLater();
        const QDBusPendingReply<> reply = *watcher;
        if (reply.isError()) {
            qWarning("logind would not unlock: %s; trying loginctl", qPrintable(reply.error().message()));
            const QString id = qEnvironmentVariable("XDG_SESSION_ID");
            QStringList args{QStringLiteral("unlock-session")};
            if (!id.isEmpty()) {
                args << id;
            }
            QProcess::startDetached(QStringLiteral("loginctl"), args);
        }
    });
}
