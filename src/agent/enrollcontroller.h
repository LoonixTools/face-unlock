// SPDX-License-Identifier: GPL-3.0-or-later
//
// The setup window's side of an enrollment: it starts one with the daemon,
// hands the camera pictures to the preview and turns the daemon's progress
// into what the ring and the text show.

#pragma once

#include <QImage>
#include <QObject>
#include <QPointF>
#include <QPointer>
#include <QRectF>
#include <QVariantList>

class DaemonRequest;

class EnrollController : public QObject
{
    Q_OBJECT
    // intro, authorizing, starting, center, circle, done, failed
    Q_PROPERTY(QString state READ state NOTIFY stateChanged)
    Q_PROPERTY(QString instruction READ instruction NOTIFY instructionChanged)
    Q_PROPERTY(QString error READ error NOTIFY stateChanged)
    Q_PROPERTY(QString name READ name WRITE setName NOTIFY nameChanged)
    // Eight booleans, clockwise from the top, as seen in the preview.
    Q_PROPERTY(QVariantList sectors READ sectors NOTIFY sectorsChanged)
    Q_PROPERTY(int sectorsDone READ sectorsDone NOTIFY sectorsChanged)
    Q_PROPERTY(bool canFinish READ canFinish NOTIFY sectorsChanged)
    // Where the head points, in the daemon's turn units (1.0 is a comfortable
    // turn), screen directions: x to the right, y up.
    Q_PROPERTY(QPointF pose READ pose NOTIFY poseChanged)
    Q_PROPERTY(bool faceVisible READ faceVisible NOTIFY poseChanged)
    // The face in the preview, as a share of the picture.
    Q_PROPERTY(QRectF faceRect READ faceRect NOTIFY frameChanged)
    Q_PROPERTY(bool hasFrame READ hasFrame NOTIFY frameChanged)
public:
    explicit EnrollController(QObject *parent = nullptr);

    QString state() const
    {
        return m_state;
    }
    QString instruction() const
    {
        return m_instruction;
    }
    QString error() const
    {
        return m_error;
    }
    QString name() const
    {
        return m_name;
    }
    void setName(const QString &name);
    QVariantList sectors() const;
    int sectorsDone() const;
    bool canFinish() const;
    QPointF pose() const
    {
        return m_pose;
    }
    bool faceVisible() const
    {
        return m_faceVisible;
    }
    QRectF faceRect() const
    {
        return m_faceRect;
    }
    bool hasFrame() const
    {
        return !m_frame.isNull();
    }
    QImage frame() const
    {
        return m_frame;
    }

    Q_INVOKABLE void start();
    Q_INVOKABLE void finish();
    Q_INVOKABLE void cancel();

Q_SIGNALS:
    void stateChanged();
    void instructionChanged();
    void nameChanged();
    void sectorsChanged();
    void poseChanged();
    void frameChanged();
    // Emitted once, when the window can go: done or given up.
    void completed(bool ok);

private:
    void setState(const QString &state);
    void setInstruction(const QString &text);
    void onEvent(const QJsonObject &event);
    void onFinished(const QJsonObject &result);
    QString hintText(const QString &hint) const;

    QPointer<DaemonRequest> m_request;
    QString m_state = QStringLiteral("intro");
    QString m_instruction;
    QString m_error;
    QString m_name;
    bool m_sectors[8] = {};
    QPointF m_pose;
    bool m_faceVisible = false;
    QRectF m_faceRect;
    QImage m_frame;
};
