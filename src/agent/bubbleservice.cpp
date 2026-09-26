// SPDX-License-Identifier: GPL-3.0-or-later

#include "bubbleservice.h"

#include "bubblecontroller.h"

#include <QDBusConnection>
#include <QDBusError>

BubbleService::BubbleService(BubbleController *bubble, QObject *parent)
    : QObject(parent)
    , m_bubble(bubble)
{
    const auto changed = [this] {
        Q_EMIT StateChanged(GetState());
    };
    connect(bubble, &BubbleController::phaseChanged, this, changed);
    connect(bubble, &BubbleController::messageChanged, this, changed);
    connect(bubble, &BubbleController::faceSeenChanged, this, changed);
    connect(bubble, &BubbleController::styleChanged, this, changed);
    connect(bubble, &BubbleController::paceChanged, this, changed);

    QDBusConnection bus = QDBusConnection::sessionBus();
    if (!bus.registerService(QStringLiteral("io.github.loonixtools.FaceUnlock"))
        || !bus.registerObject(QStringLiteral("/io/github/loonixtools/FaceUnlock"), this, QDBusConnection::ExportScriptableContents)) {
        qWarning("cannot offer the bubble on the session bus: %s", qPrintable(bus.lastError().message()));
    }
}

QVariantMap BubbleService::GetState() const
{
    return {
        {QStringLiteral("phase"), m_bubble->phase()},
        {QStringLiteral("message"), m_bubble->message()},
        {QStringLiteral("faceSeen"), m_bubble->faceSeen()},
        {QStringLiteral("style"), m_bubble->style()},
        {QStringLiteral("pace"), m_bubble->pace()},
    };
}
