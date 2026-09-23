// SPDX-License-Identifier: GPL-3.0-or-later
//
// The system settings, /etc/plasma-face-unlock/config.
//
// These are the ones that decide how hard it is to get in: which camera, how
// strict the match is, whether a photo is checked for. They belong to root
// for that reason. A setting a user process could change would be a setting
// any program running as that user could lower before asking sudo for help.

#pragma once

#include <QString>

enum class LivenessMode {
    Off,
    // Deny cues only: glare off a screen, the edge of a phone around the face.
    Light,
    // Deny cues, and a sign of life on top: a blink, or the nose moving like
    // a nose does when the head turns.
    Heavy,
};

enum class Strictness {
    Relaxed,
    Normal,
    Strict,
};

struct Settings {
    // A /dev/video path, "auto", or for testing "file:<video>" and
    // "images:<directory>".
    QString camera = QStringLiteral("auto");
    // Basic by default: screens and phones are caught without asking for a
    // blink. Strict adds the blink or head turn that stops a printed photo.
    LivenessMode liveness = LivenessMode::Light;
    Strictness strictness = Strictness::Normal;
    // Only a face that looks at the screen with its eyes open counts.
    bool attention = true;
    // How long one scan looks before it gives up.
    int scanSeconds = 5;
    // Failed scans in a row with a face in view before face unlock stops
    // until the password has been used, and how long that lasts at most.
    int maxFailures = 5;
    int lockoutMinutes = 15;
    // A laptop with the lid shut has its camera looking at the keyboard.
    bool skipLidClosed = true;
    // Take in a little of each confident unlock, so a new haircut or a pair
    // of glasses does not need a new setup. Face ID does the same.
    bool adapt = true;

    double threshold() const;

    static Settings load(const QString &path);

    static QString livenessName(LivenessMode mode);
    static QString strictnessName(Strictness strictness);
};
