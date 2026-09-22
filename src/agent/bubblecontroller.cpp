// SPDX-License-Identifier: GPL-3.0-or-later

#include "bubblecontroller.h"

#include "userconfig.h"

#include <KLocalizedString>

#include <QJsonObject>

namespace
{
// How long a result stays up before the bubble closes: long enough to read
// (on success after the face has resolved, about a second), short enough not
// to be in the way.
constexpr int SuccessHoldMs = 1500;
constexpr int FailureHoldMs = 1700;
constexpr int LockoutHoldMs = 3000;
// A scan that never reports back (the daemon went away half way) must not
// leave the bubble up for good.
constexpr int ScanWatchdogMs = 30000;
} // namespace

BubbleController::BubbleController(UserConfig *config, QObject *parent)
    : QObject(parent)
    , m_config(config)
{
    m_hide.setSingleShot(true);
    connect(&m_hide, &QTimer::timeout, this, &BubbleController::dismiss);
    connect(m_config, &UserConfig::changed, this, &BubbleController::styleChanged);
    connect(m_config, &UserConfig::changed, this, &BubbleController::paceChanged);
}

qreal BubbleController::pace() const
{
    return m_config->pace();
}

QString BubbleController::style() const
{
    return m_config->bubbleStyle();
}

bool BubbleController::enabled() const
{
    return m_config->bubble();
}

void BubbleController::setPhase(const QString &phase)
{
    if (m_phase == phase) {
        return;
    }
    m_phase = phase;
    if (phase != u"hidden" && !m_shown) {
        m_shown = true;
        Q_EMIT shownChanged();
    }
    Q_EMIT phaseChanged();
}

void BubbleController::setMessage(const QString &message)
{
    if (m_message != message) {
        m_message = message;
        Q_EMIT messageChanged();
    }
}

void BubbleController::scanStarted()
{
    if (!enabled()) {
        return;
    }
    m_hide.start(ScanWatchdogMs);
    setMessage({});
    if (m_faceSeen) {
        m_faceSeen = false;
        Q_EMIT faceSeenChanged();
    }
    setPhase(QStringLiteral("scanning"));
}

void BubbleController::faceFound()
{
    if (m_phase == u"scanning" && !m_faceSeen) {
        m_faceSeen = true;
        Q_EMIT faceSeenChanged();
    }
}

void BubbleController::hint(const QString &hint)
{
    if (m_phase != u"scanning") {
        return;
    }
    if (hint == u"blink") {
        setMessage(i18n("Blink once"));
    } else if (hint == u"look") {
        setMessage(i18n("Look at the screen"));
    } else if (hint == u"closer") {
        setMessage(i18n("Move closer"));
    } else if (hint == u"light") {
        setMessage(i18n("Too dark"));
    }
}

void BubbleController::succeeded()
{
    if (m_phase == u"hidden") {
        return;
    }
    setMessage({});
    setPhase(QStringLiteral("success"));
    m_hide.start(int(SuccessHoldMs * pace()));
}

void BubbleController::failed(const QString &reason, qint64 lockout)
{
    if (m_phase == u"hidden") {
        return;
    }
    if (lockout > 0 || reason == u"lockout") {
        setMessage(i18n("Use your password"));
        setPhase(QStringLiteral("lockout"));
        m_hide.start(int(LockoutHoldMs * pace()));
        return;
    }
    if (reason == u"mismatch" || reason == u"spoof") {
        setMessage(i18n("Not recognized"));
    } else if (reason == u"liveness") {
        setMessage(i18n("Try again and blink"));
    } else if (reason == u"camera") {
        setMessage(i18n("Camera unavailable"));
    } else {
        // Nobody there, a head turned away, the scan cancelled because the
        // password was typed: nothing worth saying. The bubble just goes.
        setMessage({});
        setPhase(QStringLiteral("hidden"));
        m_hide.stop();
        return;
    }
    setPhase(QStringLiteral("failure"));
    m_hide.start(int(FailureHoldMs * pace()));
}

void BubbleController::dismiss()
{
    m_hide.stop();
    setPhase(QStringLiteral("hidden"));
}

void BubbleController::closed()
{
    if (m_phase == u"hidden" && m_shown) {
        m_shown = false;
        Q_EMIT shownChanged();
    }
}

void BubbleController::daemonEvent(const QJsonObject &event)
{
    if (!m_config->bubbleForPrompts()) {
        return;
    }
    const QString state = event.value(u"state").toString();
    if (state == u"start") {
        scanStarted();
    } else if (state == u"face") {
        faceFound();
    } else if (state == u"hint") {
        hint(event.value(u"hint").toString());
    } else if (state == u"success") {
        succeeded();
    } else if (state == u"failure") {
        failed(event.value(u"reason").toString(), qint64(event.value(u"lockout").toDouble()));
    }
}
