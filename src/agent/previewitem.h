// SPDX-License-Identifier: GPL-3.0-or-later
//
// The camera picture in the setup window, cut to a circle around the face.
// The daemon already mirrors it, the way a mirror would show it.

#pragma once

#include <QImage>
#include <QPointer>
#include <QQuickPaintedItem>
#include <QtQml/qqmlregistration.h>

class EnrollController;

class PreviewItem : public QQuickPaintedItem
{
    Q_OBJECT
    QML_ELEMENT
    Q_PROPERTY(QObject *controller READ controller WRITE setController NOTIFY controllerChanged)
public:
    explicit PreviewItem(QQuickItem *parent = nullptr);

    QObject *controller() const;
    void setController(QObject *controller);

    void paint(QPainter *painter) override;

Q_SIGNALS:
    void controllerChanged();

private:
    void onFrame();

    QPointer<EnrollController> m_controller;
    QImage m_frame;
    // Where the circle is centred in the picture, eased towards the face so
    // the crop does not jump with every detection.
    QPointF m_centre{0.5, 0.5};
    qreal m_scale = 1.0;
};
