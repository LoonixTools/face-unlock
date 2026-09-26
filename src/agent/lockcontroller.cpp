// SPDX-License-Identifier: GPL-3.0-or-later

#include "lockcontroller.h"

#include "bubblecontroller.h"
#include "daemonclient.h"
#include "lockscreencontroller.h"
#include "userconfig.h"

#include <QDBusConnection>
#include <QJsonObject>

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
// A camera needs a moment after the machine wakes before it delivers frames.
constexpr int ScanAfterWakeMs = 1000;
} // namespace

LockController::LockController(BubbleController *bubble, UserConfig *config, SessionLock *lock, LockScreenController *screen, QObject *parent)
    : QObject(parent)
    , m_bubble(bubble)
    , m_config(config)
    , m_screen(screen)
{
    if (lock) {
        m_lock.setOwnLock(lock);
    }
    if (screen) {
        connect(screen, &LockScreenController::input, &m_input, &InputWatcher::noteInput);
    }
    m_armTimer.setSingleShot(true);
    connect(&m_armTimer, &QTimer::timeout, this, [this] {
        arm(0);
    });
    connect(&m_input, &InputWatcher::input, this, &LockController::onResume);
    connect(&m_lock, &LockWatcher::lockedChanged, this, &LockController::onLockedChanged);

    QDBusConnection::systemBus().connect(QStringLiteral("org.freedesktop.login1"),
                                         QStringLiteral("/org/freedesktop/login1"),
                                         QStringLiteral("org.freedesktop.login1.Manager"),
                                         QStringLiteral("PrepareForSleep"),
                                         this,
                                         SLOT(onPrepareForSleep(bool)));
}

void LockController::onLockedChanged(bool locked)
{
    if (locked == m_locked) {
        return;
    }
    if (!locked) {
        m_locked = false;
        m_armTimer.stop();
        m_input.stop();
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

    const bool face = m_config->enabled() && m_config->lockScreen();
    if (m_screen) {
        m_screen->setFaceUnlock(face);
    }
    if (!face) {
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

void LockController::arm(int calmMs)
{
    if (!m_locked || m_stopped || !m_config->scanOnWake()) {
        return;
    }
    m_input.watch(calmMs);
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
    // A lock screen this cannot open (gtklock, waylock, a shell's own) asks
    // for the face itself, through PAM, when Enter is pressed.
    if (!m_locked || m_scan || m_stopped || !m_lock.canUnlock()) {
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
        // Straight away, the way a phone does it: the lock screen takes a
        // moment to go, and the bubble stays above the desktop, so the rings
        // and the tick play on over it.
        m_bubble->succeeded();
        m_lock.unlock();
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
    arm(CalmBeforeRetryMs);
}
