// SPDX-License-Identifier: GPL-3.0-or-later

#include "scanjob.h"

#include "camera.h"
#include "liveness.h"
#include "vision.h"

#include <QElapsedTimer>
#include <QJsonArray>

#include <cmath>

namespace
{
// Two frames have to match. One could be a fluke of the light.
constexpr int MatchesNeeded = 2;
// Once somebody has matched, every frame spent on the recognizer is a frame
// the blink check does not see, and a blink is only a few frames long. So
// after a match the recognizer only runs on every fourth frame, which is
// still often enough to notice a different face.
constexpr int RecheckEvery = 4;
// A face that jumps further than this between two frames is treated as a
// different face, and everything learned about the previous one is dropped.
constexpr float JumpLimit = 0.6f;
// How long the deny cues watch before "light" lets anybody in.
constexpr int LightFramesNeeded = 5;
// Heavy: after a match, how long to wait for a sign of life before asking
// for one, and how much longer to wait once asked.
constexpr qint64 AskForLifeAfterMs = 1200;
constexpr qint64 ExtraTimeMs = 2500;
// Attention: a head turned further than this is not looking at the screen.
constexpr float MaxYawDegrees = 25;
constexpr float MaxPitchT = 0.16f;

double median(QList<float> v)
{
    if (v.isEmpty()) {
        return 0;
    }
    std::sort(v.begin(), v.end());
    return v.at(v.size() / 2);
}
} // namespace

ScanJob::ScanJob(Vision *vision, uid_t uid, const Settings &settings, const QList<Identity> &faces, const QString &purpose, bool verbose)
    : Job(vision, uid)
    , m_settings(settings)
    , m_faces(faces)
    , m_purpose(purpose)
    , m_verbose(verbose)
{
}

void ScanJob::finish(bool ok, const QString &reason, const QJsonObject &extra)
{
    result = extra;
    result.insert(QStringLiteral("event"), QStringLiteral("result"));
    result.insert(QStringLiteral("ok"), ok);
    result.insert(QStringLiteral("reason"), reason);
}

void ScanJob::hint(const QString &what)
{
    if (m_hinted.contains(what)) {
        return;
    }
    m_hinted.insert(what);
    Q_EMIT event({{QStringLiteral("event"), QStringLiteral("hint")}, {QStringLiteral("hint"), what}});
}

void ScanJob::run()
{
    Camera camera;
    QString error;
    if (!camera.open(m_settings.camera, &error)) {
        finish(false, QStringLiteral("camera"), {{QStringLiteral("message"), error}});
        return;
    }
    Q_EMIT event({{QStringLiteral("event"), QStringLiteral("started")}, {QStringLiteral("camera"), camera.description()}});

    // What the attention check compares against: this person's own level-head
    // nose position and open-eye measurement, from the setup.
    QList<float> noseTs, eyes;
    for (const Identity &id : std::as_const(m_faces)) {
        if (id.enabled) {
            noseTs.append(id.noseT);
            if (id.eyes > 0) {
                eyes.append(id.eyes);
            }
        }
    }
    const float restingNoseT = float(median(noseTs));
    const float openEyes = float(median(eyes));

    const double threshold = m_settings.threshold();
    const LivenessMode mode = m_settings.liveness;

    QElapsedTimer clock;
    clock.start();
    qint64 deadline = qint64(m_settings.scanSeconds) * 1000;
    bool extended = false;

    LivenessAnalyzer live;
    int matched = 0;
    int mismatched = 0;
    int attentionMisses = 0;
    int qualityMisses = 0;
    int sinceCheck = 0;
    bool lastCheckMatched = false;
    bool sawFace = false;
    int readFailures = 0;
    qint64 firstMatchAt = -1;
    cv::Point2f lastCentre(-1, -1);
    qint64 lastFaceAt = -1;
    float bestSeen = -1;

    const auto forgetMatch = [&] {
        matched = 0;
        lastCheckMatched = false;
        firstMatchAt = -1;
        matchedIdentity = -1;
        matchedScore = 0;
        matchedEmbedding.clear();
    };

    cv::Mat frame;
    while (!m_cancel) {
        const qint64 elapsed = clock.elapsed();
        if (elapsed > deadline) {
            // Heavy has matched and is only waiting for a sign of life: give
            // the person the time to blink that the hint just asked for.
            if (mode == LivenessMode::Heavy && matched >= MatchesNeeded && !extended) {
                deadline += ExtraTimeMs;
                extended = true;
                continue;
            }
            break;
        }

        double t = 0;
        if (!camera.read(frame, &t)) {
            if (++readFailures > 10) {
                finish(false, QStringLiteral("camera"), {{QStringLiteral("message"), QStringLiteral("the camera stopped delivering frames")}});
                return;
            }
            continue;
        }
        readFailures = 0;

        const std::vector<Face> faces = m_vision->detect(frame);
        if (faces.empty()) {
            continue;
        }
        const Face &face = faces.front();
        const float iod = face.interocular();
        const cv::Point2f centre = (face.eyeMid() + face.mouthMid()) * 0.5f;

        // A face that appeared somewhere else, or after a gap, may be a
        // different one. Whatever was concluded about the last one goes.
        const bool jumped = lastCentre.x >= 0 && float(cv::norm(centre - lastCentre)) > JumpLimit * iod;
        const bool gap = lastFaceAt >= 0 && elapsed - lastFaceAt > 400;
        if (jumped || gap) {
            live.reset();
            forgetMatch();
        }
        lastCentre = centre;
        lastFaceAt = elapsed;

        if (!sawFace) {
            sawFace = true;
            Q_EMIT event({{QStringLiteral("event"), QStringLiteral("face")}});
        }

        const FaceQuality quality = assessQuality(frame, face);
        const LivenessFrame lf = measureFrame(frame, face, t);
        live.add(lf);
        const LivenessReading reading = live.reading();

        if (mode != LivenessMode::Off && reading.denied) {
            finish(false, QStringLiteral("spoof"), {{QStringLiteral("cue"), reading.deniedBy}});
            return;
        }

        if (!quality.ok()) {
            ++qualityMisses;
            if (quality.tooSmall) {
                hint(QStringLiteral("closer"));
            } else if (quality.tooDark) {
                hint(QStringLiteral("light"));
            }
            continue;
        }

        // Whether this face looks at the screen with open eyes. Only asked of
        // a face that matches: a stranger is a stranger whichever way they
        // look, and should be counted as one.
        const auto attentive = [&] {
            if (!m_settings.attention) {
                return true;
            }
            const bool facing = std::abs(yawDegrees(lf.pose.yaw)) <= MaxYawDegrees
                && (noseTs.isEmpty() || std::abs(lf.pose.noseT - restingNoseT) <= MaxPitchT);
            // Eyes that measure as closed, next to how open they measured at
            // setup. Skipped for eyes the measure does not work on.
            const bool eyesOpen = !lf.eyes.valid || openEyes < 0.15f || lf.eyes.openness >= 0.45f * openEyes;
            if (!facing) {
                hint(QStringLiteral("look"));
            }
            return facing && eyesOpen;
        };

        float score = -1;
        bool checked = false;
        if (matched < MatchesNeeded || ++sinceCheck >= RecheckEvery) {
            sinceCheck = 0;
            checked = true;
            const Embedding e = m_vision->embed(frame, face);
            const FaceMatch m = FaceStore::bestMatch(m_faces, e);
            score = m.score;
            bestSeen = std::max(bestSeen, m.score);
            if (m.identity >= 0 && m.score >= threshold) {
                if (!attentive()) {
                    ++attentionMisses;
                    continue;
                }
                ++matched;
                lastCheckMatched = true;
                if (firstMatchAt < 0) {
                    firstMatchAt = elapsed;
                }
                if (m.score > matchedScore) {
                    matchedScore = m.score;
                    matchedIdentity = m.identity;
                    matchedEmbedding = e;
                }
            } else {
                ++mismatched;
                // The face that matched before does not match now. Whatever
                // it did to look alive was done by somebody else's face.
                if (lastCheckMatched) {
                    live.reset();
                    forgetMatch();
                }
                lastCheckMatched = false;
            }
        } else if (!attentive()) {
            // Between checks: a matched face that looks away or closes its
            // eyes (a blink does both for a moment) is not counted as looking.
            ++attentionMisses;
            continue;
        }

        if (m_verbose) {
            QJsonObject info{
                {QStringLiteral("event"), QStringLiteral("frame")},
                {QStringLiteral("t"), qint64(t)},
                {QStringLiteral("yaw"), double(yawDegrees(lf.pose.yaw))},
                {QStringLiteral("eyes"), double(lf.eyes.openness)},
                {QStringLiteral("glare"), double(reading.glare)},
                {QStringLiteral("device"), double(reading.device)},
                {QStringLiteral("depth"), double(reading.depth)},
                {QStringLiteral("blink"), double(reading.blink)},
                {QStringLiteral("matched"), matched},
            };
            if (checked) {
                info.insert(QStringLiteral("score"), double(score));
            }
            Q_EMIT event(info);
        }

        if (matched < MatchesNeeded || !lastCheckMatched) {
            continue;
        }

        bool alive = true;
        switch (mode) {
        case LivenessMode::Off:
            break;
        case LivenessMode::Light:
            alive = live.frameCount() >= LightFramesNeeded;
            break;
        case LivenessMode::Heavy:
            alive = reading.confirmed;
            if (!alive && elapsed - firstMatchAt > AskForLifeAfterMs) {
                hint(QStringLiteral("blink"));
            }
            break;
        }

        if (alive) {
            const Identity &id = m_faces.at(matchedIdentity);
            finish(true, QStringLiteral("ok"),
                   {{QStringLiteral("name"), id.name},
                    {QStringLiteral("id"), id.id},
                    {QStringLiteral("score"), double(matchedScore)},
                    {QStringLiteral("liveness"), reading.confirmedBy},
                    {QStringLiteral("ms"), clock.elapsed()}});
            return;
        }
    }

    if (m_cancel) {
        finish(false, QStringLiteral("cancelled"));
    } else if (matched >= MatchesNeeded) {
        finish(false, QStringLiteral("liveness"));
    } else if (mismatched >= 3) {
        finish(false, QStringLiteral("mismatch"), {{QStringLiteral("score"), double(bestSeen)}});
    } else if (attentionMisses > 0) {
        finish(false, QStringLiteral("attention"));
    } else if (qualityMisses > 0) {
        finish(false, QStringLiteral("quality"));
    } else {
        finish(false, QStringLiteral("no-face"));
    }
}
