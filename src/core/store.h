// SPDX-License-Identifier: GPL-3.0-or-later
//
// The face data.
//
// One file per user under /var/lib/plasma-face-unlock/users, named after the
// numeric user id so a rename cannot hand one person's faces to another. Only
// root can read or write the directory. There are no pictures in it: every
// sample is the 128 numbers the recognizer made of one frame, and the frame
// itself was never written anywhere.

#pragma once

#include "vision.h"

#include <QList>
#include <QString>

struct FaceSample {
    Embedding embedding;
    // Which way the head pointed when it was taken: "center", one of the
    // eight directions ("up", "up-right", ...), or "adaptive" for one taken
    // in from a successful unlock.
    QString pose;
    qint64 time = 0;
};

struct Identity {
    QString id;
    QString name;
    qint64 created = 0;
    bool enabled = true;
    // Where this person's nose sits for a level head (see HeadPose), and how
    // open their eyes measure when they look at the camera. Both are what
    // the attention check compares against.
    float noseT = 0.55f;
    float eyes = 0;
    QList<FaceSample> samples;

    int adaptiveCount() const;
};

struct FaceMatch {
    float score = -1;
    int identity = -1;
    int sample = -1;
};

class FaceStore
{
public:
    explicit FaceStore(const QString &stateDir);

    QList<Identity> load(uint uid, QString *error = nullptr) const;
    bool save(uint uid, const QList<Identity> &faces, QString *error) const;
    bool remove(uint uid) const;

    static QString newId();

    // The best sample over every enabled identity.
    static FaceMatch bestMatch(const QList<Identity> &faces, const Embedding &e);

    // Adds what a confident unlock saw, if it adds anything, and keeps at most
    // MaxAdaptive of those per identity (the oldest go first). The samples
    // from the setup are never touched.
    static bool adapt(Identity &identity, const Embedding &e, qint64 now);

    static constexpr int MaxAdaptive = 12;

private:
    QString fileFor(uint uid) const;
    QString m_dir;
};
