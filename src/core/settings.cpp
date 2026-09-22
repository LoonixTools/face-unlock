// SPDX-License-Identifier: GPL-3.0-or-later

#include "settings.h"
#include "keyvalue.h"

double Settings::threshold() const
{
    // Cosine similarity between two SFace embeddings. Photos of the same
    // person taken years apart land around 0.75, different people below 0.3.
    // OpenCV's own recommendation (0.363) is tuned for telling apart photos
    // in a data set; this guards a login, so the bar is higher.
    switch (strictness) {
    case Strictness::Relaxed:
        return 0.42;
    case Strictness::Normal:
        return 0.50;
    case Strictness::Strict:
        return 0.58;
    }
    return 0.50;
}

Settings Settings::load(const QString &path)
{
    Settings s;
    const KeyValueFile kv = KeyValueFile::load(path);

    s.camera = kv.value(QStringLiteral("Camera"), s.camera);

    const QString liveness = kv.value(QStringLiteral("Liveness")).toLower();
    if (liveness == u"off") {
        s.liveness = LivenessMode::Off;
    } else if (liveness == u"light") {
        s.liveness = LivenessMode::Light;
    } else if (liveness == u"heavy") {
        s.liveness = LivenessMode::Heavy;
    }

    const QString strictness = kv.value(QStringLiteral("Strictness")).toLower();
    if (strictness == u"relaxed") {
        s.strictness = Strictness::Relaxed;
    } else if (strictness == u"normal") {
        s.strictness = Strictness::Normal;
    } else if (strictness == u"strict") {
        s.strictness = Strictness::Strict;
    }

    s.attention = kv.boolean(QStringLiteral("Attention"), s.attention);
    s.scanSeconds = kv.integer(QStringLiteral("ScanSeconds"), s.scanSeconds, 2, 15);
    s.maxFailures = kv.integer(QStringLiteral("MaxFailures"), s.maxFailures, 1, 20);
    s.lockoutMinutes = kv.integer(QStringLiteral("LockoutMinutes"), s.lockoutMinutes, 1, 24 * 60);
    s.skipLidClosed = kv.boolean(QStringLiteral("SkipLidClosed"), s.skipLidClosed);
    s.adapt = kv.boolean(QStringLiteral("Adapt"), s.adapt);
    return s;
}

QString Settings::livenessName(LivenessMode mode)
{
    switch (mode) {
    case LivenessMode::Off:
        return QStringLiteral("off");
    case LivenessMode::Light:
        return QStringLiteral("light");
    case LivenessMode::Heavy:
        return QStringLiteral("heavy");
    }
    return {};
}

QString Settings::strictnessName(Strictness strictness)
{
    switch (strictness) {
    case Strictness::Relaxed:
        return QStringLiteral("relaxed");
    case Strictness::Normal:
        return QStringLiteral("normal");
    case Strictness::Strict:
        return QStringLiteral("strict");
    }
    return {};
}
