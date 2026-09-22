// SPDX-License-Identifier: GPL-3.0-or-later
//
// Recognition on real pictures, for trying things out without a camera:
//
//   test_images MODELDIR REFERENCE.jpg OTHER.jpg...
//
// Takes the face in REFERENCE as the enrolled one and prints how every other
// picture compares, with the pose and quality the daemon would see.
//
//   test_images --enroll STATEDIR UID NAME MODELDIR PICTURE...
//
// Writes the faces in the pictures straight into a face store, as if they had
// been set up, so a development daemon has somebody to recognise.

#include "liveness.h"
#include "store.h"
#include "vision.h"

#include <QDateTime>

#include <QFile>
#include <QString>

#include <opencv2/imgcodecs.hpp>
#include <opencv2/imgproc.hpp>

#include <cstdio>

namespace
{
cv::Mat load(const char *path)
{
    cv::Mat img = cv::imread(path, cv::IMREAD_COLOR);
    if (!img.empty()) {
        const double s = 640.0 / std::max(img.cols, img.rows);
        if (s < 1) {
            cv::resize(img, img, {}, s, s, cv::INTER_AREA);
        }
    }
    return img;
}
} // namespace

int enroll(int argc, char **argv)
{
    if (argc < 7) {
        std::fprintf(stderr, "usage: %s --enroll STATEDIR UID NAME MODELDIR PICTURE...\n", argv[0]);
        return 2;
    }
    Vision vision;
    QString error;
    if (!vision.load(QString::fromLocal8Bit(argv[5]), &error)) {
        std::fprintf(stderr, "%s\n", qPrintable(error));
        return 1;
    }
    Identity id;
    id.id = FaceStore::newId();
    id.name = QString::fromLocal8Bit(argv[4]);
    id.created = QDateTime::currentSecsSinceEpoch();
    QList<float> noseT, eyes;
    for (int i = 6; i < argc; ++i) {
        const cv::Mat img = load(argv[i]);
        const std::vector<Face> faces = vision.detect(img);
        if (faces.empty()) {
            std::fprintf(stderr, "no face in %s\n", argv[i]);
            continue;
        }
        id.samples.append({vision.embed(img, faces.front()), QStringLiteral("center"), id.created});
        noseT.append(estimatePose(faces.front()).noseT);
        const EyeSample e = measureEyes(img, faces.front());
        if (e.valid) {
            eyes.append(e.openness);
        }
    }
    if (id.samples.isEmpty()) {
        return 1;
    }
    std::sort(noseT.begin(), noseT.end());
    std::sort(eyes.begin(), eyes.end());
    id.noseT = noseT.at(noseT.size() / 2);
    id.eyes = eyes.isEmpty() ? 0.f : eyes.at(eyes.size() / 2);

    const FaceStore store(QString::fromLocal8Bit(argv[2]));
    const uint uid = QString::fromLocal8Bit(argv[3]).toUInt();
    QList<Identity> all = store.load(uid);
    all.append(id);
    if (!store.save(uid, all, &error)) {
        std::fprintf(stderr, "%s\n", qPrintable(error));
        return 1;
    }
    std::printf("enrolled %s with %d samples\n", qPrintable(id.name), int(id.samples.size()));
    return 0;
}

int main(int argc, char **argv)
{
    if (argc > 1 && QString::fromLocal8Bit(argv[1]) == u"--enroll") {
        return enroll(argc, argv);
    }
    if (argc < 4) {
        std::fprintf(stderr, "usage: %s MODELDIR REFERENCE OTHER...\n", argv[0]);
        return 2;
    }
    Vision vision;
    QString error;
    if (!vision.load(QString::fromLocal8Bit(argv[1]), &error)) {
        std::fprintf(stderr, "%s\n", qPrintable(error));
        return 1;
    }

    const cv::Mat ref = load(argv[2]);
    const std::vector<Face> refFaces = vision.detect(ref);
    if (refFaces.empty()) {
        std::fprintf(stderr, "no face in %s\n", argv[2]);
        return 1;
    }
    const Embedding reference = vision.embed(ref, refFaces.front());

    for (int i = 3; i < argc; ++i) {
        const cv::Mat img = load(argv[i]);
        const std::vector<Face> faces = vision.detect(img);
        if (faces.empty()) {
            std::printf("%-50s no face\n", argv[i]);
            continue;
        }
        const Face &f = faces.front();
        const HeadPose pose = estimatePose(f);
        const FaceQuality q = assessQuality(img, f);
        const EyeSample eyes = measureEyes(img, f);
        const GlareSample glare = measureGlare(img, f);
        std::printf("%-50s similarity %.3f  yaw %5.1f  noseT %.2f  iod %3.0f  light %3.0f  sharp %5.0f  eyes %.2f/%.2f  glare %.3f/%.2f  device %d\n",
                    argv[i], Vision::similarity(reference, vision.embed(img, f)), yawDegrees(pose.yaw), pose.noseT, f.interocular(),
                    q.brightness, q.sharpness, eyes.left, eyes.right, glare.fraction, glare.cluster, int(detectDevice(img, f)));
    }
    return 0;
}
