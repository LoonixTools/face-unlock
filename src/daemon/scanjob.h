// SPDX-License-Identifier: GPL-3.0-or-later
//
// One attempt at recognising somebody.

#pragma once

#include "job.h"
#include "settings.h"
#include "store.h"

class ScanJob : public Job
{
    Q_OBJECT
public:
    ScanJob(Vision *vision, uid_t uid, const Settings &settings, const QList<Identity> &faces, const QString &purpose, bool verbose);

    void run() override;
    QString kind() const override
    {
        return QStringLiteral("verify");
    }

    QString purpose() const
    {
        return m_purpose;
    }

    // Set on success: which face matched and what the camera saw, so the
    // daemon can learn from it (see FaceStore::adapt).
    int matchedIdentity = -1;
    float matchedScore = 0;
    Embedding matchedEmbedding;

private:
    void finish(bool ok, const QString &reason, const QJsonObject &extra = {});
    void hint(const QString &what);

    Settings m_settings;
    QList<Identity> m_faces;
    QString m_purpose;
    bool m_verbose;
    QSet<QString> m_hinted;
};
