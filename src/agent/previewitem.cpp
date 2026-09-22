// SPDX-License-Identifier: GPL-3.0-or-later

#include "previewitem.h"

#include "enrollcontroller.h"

#include <QPainter>
#include <QPainterPath>

PreviewItem::PreviewItem(QQuickItem *parent)
    : QQuickPaintedItem(parent)
{
    setAntialiasing(true);
}

QObject *PreviewItem::controller() const
{
    return m_controller;
}

void PreviewItem::setController(QObject *controller)
{
    auto *c = qobject_cast<EnrollController *>(controller);
    if (c == m_controller) {
        return;
    }
    if (m_controller) {
        disconnect(m_controller, nullptr, this, nullptr);
    }
    m_controller = c;
    if (m_controller) {
        connect(m_controller, &EnrollController::frameChanged, this, &PreviewItem::onFrame);
    }
    Q_EMIT controllerChanged();
}

void PreviewItem::onFrame()
{
    if (!m_controller) {
        return;
    }
    m_frame = m_controller->frame();
    const QRectF face = m_controller->faceRect();
    if (face.isValid() && !m_frame.isNull()) {
        // The face fills a little over half of the circle, whatever the
        // distance, so a small face does not end up as a dot in the middle.
        const qreal aspect = qreal(m_frame.width()) / m_frame.height();
        const qreal faceSide = std::max(face.width() * aspect, face.height());
        const qreal targetScale = std::clamp(0.55 / std::max(0.05, faceSide), 1.0, 2.2);
        const QPointF target = face.center();
        m_centre += (target - m_centre) * 0.25;
        m_scale += (targetScale - m_scale) * 0.2;
    }
    update();
}

void PreviewItem::paint(QPainter *painter)
{
    const QRectF bounds = boundingRect();
    QPainterPath circle;
    circle.addEllipse(bounds);
    painter->setRenderHint(QPainter::Antialiasing);
    painter->setRenderHint(QPainter::SmoothPixmapTransform);
    painter->setClipPath(circle);
    painter->fillRect(bounds, QColor(0x1e, 0x1e, 0x1e));

    if (m_frame.isNull()) {
        return;
    }

    // Cover the circle with the picture: its shorter side spans the circle,
    // zoomed by m_scale and centred on the face.
    const qreal fw = m_frame.width();
    const qreal fh = m_frame.height();
    const qreal side = std::min(fw, fh) / m_scale;
    QRectF source(m_centre.x() * fw - side / 2, m_centre.y() * fh - side / 2, side, side);
    if (source.left() < 0) {
        source.moveLeft(0);
    }
    if (source.top() < 0) {
        source.moveTop(0);
    }
    if (source.right() > fw) {
        source.moveRight(fw);
    }
    if (source.bottom() > fh) {
        source.moveBottom(fh);
    }
    painter->drawImage(bounds, m_frame, source);
}
