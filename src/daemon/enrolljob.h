// SPDX-License-Identifier: GPL-3.0-or-later
//
// Setting up a face.
//
// Like the phone: look straight at the camera first, then move the head
// around in a circle while the ring fills up. The centre gives the samples
// most unlocks will match against and the level-head measurements the
// attention check needs; the eight directions around it are what still
// matches when somebody glances at the screen from the side.

#pragma once

#include "job.h"
#include "settings.h"
#include "store.h"

class EnrollJob : public Job
{
    Q_OBJECT
public:
    EnrollJob(Vision *vision, uid_t uid, const Settings &settings, const QString &name);

    void run() override;
    QString kind() const override
    {
        return QStringLiteral("enroll");
    }

    // Stop as soon as enough of the circle is done.
    void finishEarly()
    {
        m_finishEarly = true;
    }

    // Filled when the setup completed.
    Identity identity;

    // The eight directions, clockwise from straight up, as seen in the
    // mirrored preview.
    static QString sectorName(int sector);

    static constexpr int SamplesPerSector = 2;
    static constexpr int CentreSamples = 3;
    static constexpr int SectorsForEarlyFinish = 4;

private:
    void sendPreview(const cv::Mat &frame, const struct Face *face);

    Settings m_settings;
    QString m_name;
    std::atomic_bool m_finishEarly = false;
    qint64 m_lastPreview = -1000;
};
