// SPDX-License-Identifier: GPL-3.0-or-later
//
// Work that holds the camera: a scan or a setup. Only one runs at a time,
// on a thread of its own so the socket stays responsive (a cancel has to get
// through while a frame is being processed).

#pragma once

#include <QJsonObject>
#include <QObject>

#include <atomic>
#include <sys/types.h>

class Vision;

class Job : public QObject
{
    Q_OBJECT
public:
    Job(Vision *vision, uid_t uid)
        : m_vision(vision)
        , m_uid(uid)
    {
    }

    virtual void run() = 0;
    virtual QString kind() const = 0;

    void cancel()
    {
        m_cancel = true;
    }
    bool cancelled() const
    {
        return m_cancel;
    }
    uid_t uid() const
    {
        return m_uid;
    }

    QJsonObject result;

Q_SIGNALS:
    // Progress for whoever asked, and for the bubble.
    void event(const QJsonObject &event);

protected:
    Vision *m_vision;
    uid_t m_uid;
    std::atomic_bool m_cancel = false;
};
