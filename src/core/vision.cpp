// SPDX-License-Identifier: GPL-3.0-or-later

#include "vision.h"

#include <QDir>
#include <QFile>
#include <QFileInfo>

#include <opencv2/core/utils/logger.hpp>
#include <opencv2/imgproc.hpp>

#include <algorithm>
#include <cmath>
#include <numeric>

namespace
{
const char DetectorFile[] = "face_detection_yunet_2023mar.onnx";
const char RecognizerFile[] = "face_recognition_sface_2021dec.onnx";

// A face whose eyes are closer together than this is too far away for the
// recognizer to see detail in, and the liveness checks drown in noise.
constexpr float MinInterocular = 28.f;

cv::Point2f rotateAround(cv::Point2f p, cv::Point2f centre, float angle)
{
    const float c = std::cos(angle);
    const float s = std::sin(angle);
    const cv::Point2f d = p - centre;
    return {centre.x + d.x * c - d.y * s, centre.y + d.x * s + d.y * c};
}
} // namespace

float Face::interocular() const
{
    return float(cv::norm(points[LeftEye] - points[RightEye]));
}

cv::Point2f Face::eyeMid() const
{
    return (points[RightEye] + points[LeftEye]) * 0.5f;
}

cv::Point2f Face::mouthMid() const
{
    return (points[RightMouth] + points[LeftMouth]) * 0.5f;
}

HeadPose estimatePose(const Face &face)
{
    HeadPose pose;
    const cv::Point2f eyes = face.points[LeftEye] - face.points[RightEye];
    pose.roll = std::atan2(eyes.y, eyes.x);

    // Level the face first, so a tilted head does not read as a turned one.
    const cv::Point2f centre = face.eyeMid();
    std::array<cv::Point2f, 5> p;
    for (int i = 0; i < 5; ++i) {
        p[i] = rotateAround(face.points[i], centre, -pose.roll);
    }

    const cv::Point2f eyeMid = (p[RightEye] + p[LeftEye]) * 0.5f;
    const cv::Point2f mouthMid = (p[RightMouth] + p[LeftMouth]) * 0.5f;
    const float iod = std::max(1.f, float(cv::norm(p[LeftEye] - p[RightEye])));
    const float span = mouthMid.y - eyeMid.y;
    if (span < 1.f) {
        return pose;
    }

    const cv::Point2f nose = p[NoseTip];
    const float t = (nose.y - eyeMid.y) / span;
    const float midlineX = eyeMid.x + (mouthMid.x - eyeMid.x) * t;

    // The eyes are reported person-right first, which is image-left on an
    // unmirrored frame. A nose moving image-right is a head turning to the
    // person's left.
    pose.yaw = (nose.x - midlineX) / iod;
    pose.noseT = t;
    return pose;
}

float yawDegrees(float yaw)
{
    return float(std::asin(std::clamp(yaw / 0.5f, -1.f, 1.f)) * 180.0 / M_PI);
}

FaceQuality assessQuality(const cv::Mat &bgr, const Face &face)
{
    FaceQuality q;
    q.tooSmall = face.interocular() < MinInterocular;

    // The middle of the face, without hair and background at the edges.
    const float iod = face.interocular();
    const cv::Point2f c = (face.eyeMid() + face.mouthMid()) * 0.5f;
    cv::Rect roi(cv::Point(int(c.x - iod), int(c.y - iod)), cv::Size(int(2 * iod), int(2 * iod)));
    roi &= cv::Rect(0, 0, bgr.cols, bgr.rows);
    if (roi.width < 8 || roi.height < 8) {
        q.tooSmall = true;
        return q;
    }

    cv::Mat grey;
    cv::cvtColor(bgr(roi), grey, cv::COLOR_BGR2GRAY);
    q.brightness = float(cv::mean(grey)[0]);

    // Sharpness at a fixed scale, so a face far away and one up close are
    // judged the same.
    cv::Mat small, lap;
    cv::resize(grey, small, cv::Size(96, 96), 0, 0, cv::INTER_AREA);
    cv::Laplacian(small, lap, CV_32F);
    cv::Scalar mean, stddev;
    cv::meanStdDev(lap, mean, stddev);
    q.sharpness = float(stddev[0] * stddev[0]);

    q.tooDark = q.brightness < 40;
    q.tooBright = q.brightness > 230;
    q.blurry = q.sharpness < 12;
    return q;
}

bool Vision::load(const QString &modelDir, QString *error)
{
    // The new DNN engine in OpenCV 5 prints a warning for every network it
    // loads about targets it does not support yet. Nothing here asks for one.
    cv::utils::logging::setLogLevel(cv::utils::logging::LOG_LEVEL_ERROR);

    const QDir dir(modelDir);
    const QString detector = dir.filePath(QLatin1String(DetectorFile));
    const QString recognizer = dir.filePath(QLatin1String(RecognizerFile));
    for (const QString &f : {detector, recognizer}) {
        if (!QFileInfo::exists(f)) {
            *error = QStringLiteral("model missing: %1").arg(f);
            return false;
        }
    }

    try {
        m_inputSize = cv::Size(640, 480);
        m_detector = cv::FaceDetectorYN::create(QFile::encodeName(detector).toStdString(), "", m_inputSize, 0.75f, 0.3f, 20);
        m_recognizer = cv::FaceRecognizerSF::create(QFile::encodeName(recognizer).toStdString(), "");
    } catch (const cv::Exception &e) {
        *error = QString::fromStdString(e.what());
        m_detector.reset();
        m_recognizer.reset();
        return false;
    }
    return true;
}

std::vector<Face> Vision::detect(const cv::Mat &bgr)
{
    std::vector<Face> result;
    if (!m_detector || bgr.empty()) {
        return result;
    }
    if (bgr.size() != m_inputSize) {
        m_inputSize = bgr.size();
        m_detector->setInputSize(m_inputSize);
    }

    cv::Mat rows;
    try {
        m_detector->detect(bgr, rows);
    } catch (const cv::Exception &) {
        return result;
    }

    for (int i = 0; i < rows.rows; ++i) {
        const float *r = rows.ptr<float>(i);
        Face f;
        f.box = cv::Rect2f(r[0], r[1], r[2], r[3]);
        for (int k = 0; k < 5; ++k) {
            f.points[k] = cv::Point2f(r[4 + 2 * k], r[5 + 2 * k]);
        }
        f.score = r[14];
        f.row = rows.row(i).clone();
        result.push_back(std::move(f));
    }
    std::sort(result.begin(), result.end(), [](const Face &a, const Face &b) {
        return a.box.area() > b.box.area();
    });
    return result;
}

Embedding Vision::embed(const cv::Mat &bgr, const Face &face)
{
    Embedding out;
    if (!m_recognizer) {
        return out;
    }
    try {
        cv::Mat aligned, feature;
        m_recognizer->alignCrop(bgr, face.row, aligned);
        m_recognizer->feature(aligned, feature);
        feature = feature.reshape(1, 1);
        if (feature.cols != EmbeddingSize) {
            return out;
        }
        const double norm = cv::norm(feature);
        if (norm <= 0) {
            return out;
        }
        out.resize(EmbeddingSize);
        for (int i = 0; i < EmbeddingSize; ++i) {
            out[i] = float(feature.at<float>(0, i) / norm);
        }
    } catch (const cv::Exception &) {
        out.clear();
    }
    return out;
}

float Vision::similarity(const Embedding &a, const Embedding &b)
{
    if (a.size() != b.size() || a.empty()) {
        return -1.f;
    }
    return std::inner_product(a.begin(), a.end(), b.begin(), 0.f);
}
