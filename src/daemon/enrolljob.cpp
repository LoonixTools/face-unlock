// SPDX-License-Identifier: GPL-3.0-or-later

#include "enrolljob.h"

#include "camera.h"
#include "liveness.h"
#include "vision.h"

#include <QDateTime>
#include <QElapsedTimer>

#include <opencv2/imgcodecs.hpp>
#include <opencv2/imgproc.hpp>

#include <array>
#include <cmath>

namespace
{
// A head turned about 15 degrees moves the nose this far (in eye
// distances), sideways for a turn and up or down for a nod. The ring is drawn
// in these units, so 1.0 is a comfortable turn.
constexpr float TurnUnit = 0.13f;
// Far enough out to count for a direction.
constexpr float CaptureAt = 0.8f;
// Straight enough to count as looking straight ahead.
constexpr float StraightYaw = 0.06f;
constexpr qint64 SampleSpacingMs = 150;
constexpr qint64 CentreSpacingMs = 250;
constexpr qint64 PreviewEveryMs = 66;
constexpr qint64 GiveUpAfterMs = 120 * 1000;
constexpr int PreviewWidth = 480;

float median(QList<float> v)
{
    if (v.isEmpty()) {
        return 0;
    }
    std::sort(v.begin(), v.end());
    return v.at(v.size() / 2);
}
} // namespace

EnrollJob::EnrollJob(Vision *vision, uid_t uid, const Settings &settings, const QString &name)
    : Job(vision, uid)
    , m_settings(settings)
    , m_name(name)
{
}

QString EnrollJob::sectorName(int sector)
{
    static const char *names[] = {"up", "up-right", "right", "down-right", "down", "down-left", "left", "up-left"};
    return QString::fromLatin1(names[((sector % 8) + 8) % 8]);
}

void EnrollJob::sendPreview(const cv::Mat &frame, const Face *face)
{
    cv::Mat mirrored, small;
    cv::flip(frame, mirrored, 1);
    const double scale = double(PreviewWidth) / mirrored.cols;
    cv::resize(mirrored, small, cv::Size(), scale, scale, cv::INTER_AREA);

    std::vector<uchar> jpeg;
    cv::imencode(".jpg", small, jpeg, {cv::IMWRITE_JPEG_QUALITY, 72});

    QJsonObject msg{
        {QStringLiteral("event"), QStringLiteral("frame")},
        {QStringLiteral("w"), small.cols},
        {QStringLiteral("h"), small.rows},
        {QStringLiteral("jpeg"), QString::fromLatin1(QByteArray(reinterpret_cast<const char *>(jpeg.data()), qsizetype(jpeg.size())).toBase64())},
    };
    if (face) {
        // In the mirrored picture, as a share of its size.
        const float w = float(frame.cols);
        const float h = float(frame.rows);
        msg.insert(QStringLiteral("face"),
                   QJsonObject{
                       {QStringLiteral("x"), double((w - face->box.x - face->box.width) / w)},
                       {QStringLiteral("y"), double(face->box.y / h)},
                       {QStringLiteral("w"), double(face->box.width / w)},
                       {QStringLiteral("h"), double(face->box.height / h)},
                   });
    }
    Q_EMIT event(msg);
}

void EnrollJob::run()
{
    Camera camera;
    QString error;
    if (!camera.open(m_settings.camera, &error)) {
        result = {{QStringLiteral("event"), QStringLiteral("result")},
                  {QStringLiteral("ok"), false},
                  {QStringLiteral("reason"), QStringLiteral("camera")},
                  {QStringLiteral("message"), error}};
        return;
    }
    Q_EMIT event({{QStringLiteral("event"), QStringLiteral("started")}, {QStringLiteral("camera"), camera.description()}});

    QElapsedTimer clock;
    clock.start();

    QList<float> centreNoseT;
    QList<float> centreEyes;
    QList<FaceSample> samples;
    int centreCount = 0;
    qint64 lastCentreSample = -CentreSpacingMs;
    bool centreDone = false;
    float restingNoseT = 0.55f;

    std::array<int, 8> counts{};
    std::array<qint64, 8> lastAt;
    lastAt.fill(-SampleSpacingMs);
    QString lastHint;

    const auto hint = [&](const QString &what) {
        if (what != lastHint) {
            lastHint = what;
            Q_EMIT event({{QStringLiteral("event"), QStringLiteral("hint")}, {QStringLiteral("hint"), what}});
        }
    };
    const auto sectorsDone = [&] {
        return int(std::count(counts.begin(), counts.end(), SamplesPerSector));
    };

    cv::Mat frame;
    int readFailures = 0;
    while (!m_cancel && clock.elapsed() < GiveUpAfterMs) {
        double t = 0;
        if (!camera.read(frame, &t)) {
            if (++readFailures > 10) {
                result = {{QStringLiteral("event"), QStringLiteral("result")},
                          {QStringLiteral("ok"), false},
                          {QStringLiteral("reason"), QStringLiteral("camera")},
                          {QStringLiteral("message"), QStringLiteral("the camera stopped delivering frames")}};
                return;
            }
            continue;
        }
        readFailures = 0;
        const qint64 now = clock.elapsed();

        const std::vector<Face> faces = m_vision->detect(frame);
        const Face *face = faces.empty() ? nullptr : &faces.front();
        if (now - m_lastPreview >= PreviewEveryMs) {
            m_lastPreview = now;
            sendPreview(frame, face);
        }

        if (!face) {
            Q_EMIT event({{QStringLiteral("event"), QStringLiteral("pose")}, {QStringLiteral("face"), false}});
            hint(QStringLiteral("no-face"));
            continue;
        }
        // Somebody else in the picture, close enough to be taken for the
        // person setting up.
        if (faces.size() > 1 && faces[1].box.area() > 0.5f * face->box.area()) {
            hint(QStringLiteral("one-face"));
            continue;
        }

        const FaceQuality quality = assessQuality(frame, *face);
        if (!quality.ok()) {
            hint(quality.tooSmall ? QStringLiteral("closer")
                 : quality.tooDark ? QStringLiteral("light")
                 : quality.tooBright ? QStringLiteral("bright")
                                     : QStringLiteral("still"));
            continue;
        }

        const HeadPose pose = estimatePose(*face);

        if (!centreDone) {
            const bool straight = std::abs(pose.yaw) <= StraightYaw;
            Q_EMIT event({{QStringLiteral("event"), QStringLiteral("pose")},
                          {QStringLiteral("face"), true},
                          {QStringLiteral("phase"), QStringLiteral("center")},
                          {QStringLiteral("x"), double(-pose.yaw / TurnUnit)},
                          {QStringLiteral("y"), 0.0}});
            if (!straight) {
                hint(QStringLiteral("straight"));
                continue;
            }
            hint(QStringLiteral("hold"));

            centreNoseT.append(pose.noseT);
            const EyeSample eyes = measureEyes(frame, *face);
            if (eyes.valid) {
                centreEyes.append(eyes.openness);
            }
            if (centreCount < CentreSamples && now - lastCentreSample >= CentreSpacingMs) {
                const Embedding e = m_vision->embed(frame, *face);
                if (!e.empty()) {
                    samples.append({e, QStringLiteral("center"), QDateTime::currentSecsSinceEpoch()});
                    ++centreCount;
                    lastCentreSample = now;
                }
            }
            if (centreCount >= CentreSamples && centreNoseT.size() >= 6) {
                restingNoseT = median(centreNoseT);
                centreDone = true;
                Q_EMIT event({{QStringLiteral("event"), QStringLiteral("captured")}, {QStringLiteral("pose"), QStringLiteral("center")}});
                hint(QStringLiteral("circle"));
            }
            continue;
        }

        // Screen directions in the mirrored preview: turning to one's own left
        // moves the face to the left of the screen, looking up moves it up.
        const float x = -pose.yaw / TurnUnit;
        const float y = -(pose.noseT - restingNoseT) / TurnUnit;
        const float reach = std::hypot(x, y);
        double angle = std::atan2(x, y) * 180.0 / M_PI;
        if (angle < 0) {
            angle += 360;
        }
        Q_EMIT event({{QStringLiteral("event"), QStringLiteral("pose")},
                      {QStringLiteral("face"), true},
                      {QStringLiteral("phase"), QStringLiteral("circle")},
                      {QStringLiteral("x"), double(x)},
                      {QStringLiteral("y"), double(y)},
                      {QStringLiteral("angle"), angle},
                      {QStringLiteral("reach"), double(reach)}});

        if (reach >= CaptureAt) {
            const int sector = int(std::lround(angle / 45.0)) % 8;
            if (counts[sector] < SamplesPerSector && now - lastAt[sector] >= SampleSpacingMs) {
                const Embedding e = m_vision->embed(frame, *face);
                if (!e.empty()) {
                    samples.append({e, sectorName(sector), QDateTime::currentSecsSinceEpoch()});
                    lastAt[sector] = now;
                    if (++counts[sector] == SamplesPerSector) {
                        Q_EMIT event({{QStringLiteral("event"), QStringLiteral("captured")},
                                      {QStringLiteral("pose"), sectorName(sector)},
                                      {QStringLiteral("sector"), sector},
                                      {QStringLiteral("done"), sectorsDone()}});
                    }
                }
            }
        }

        if (sectorsDone() == 8 || (m_finishEarly && sectorsDone() >= SectorsForEarlyFinish)) {
            break;
        }
    }

    if (m_cancel) {
        result = {{QStringLiteral("event"), QStringLiteral("result")}, {QStringLiteral("ok"), false}, {QStringLiteral("reason"), QStringLiteral("cancelled")}};
        return;
    }
    if (!centreDone || sectorsDone() < SectorsForEarlyFinish) {
        result = {{QStringLiteral("event"), QStringLiteral("result")}, {QStringLiteral("ok"), false}, {QStringLiteral("reason"), QStringLiteral("timeout")}};
        return;
    }

    identity.id = FaceStore::newId();
    identity.name = m_name;
    identity.created = QDateTime::currentSecsSinceEpoch();
    identity.noseT = restingNoseT;
    identity.eyes = median(centreEyes);
    identity.samples = samples;

    result = {{QStringLiteral("event"), QStringLiteral("result")},
              {QStringLiteral("ok"), true},
              {QStringLiteral("reason"), QStringLiteral("ok")},
              {QStringLiteral("id"), identity.id},
              {QStringLiteral("name"), identity.name},
              {QStringLiteral("samples"), int(samples.size())}};
}
