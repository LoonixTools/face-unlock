// SPDX-License-Identifier: GPL-3.0-or-later
//
// Finding a face and turning it into numbers.
//
// Two small networks from the OpenCV model zoo do the work. YuNet finds faces
// and five points on each (the eyes, the tip of the nose, the corners of the
// mouth). SFace turns an aligned crop of one face into 128 numbers, and two
// crops of the same person give numbers that point the same way. Both run on
// the CPU through OpenCV's own DNN module, in a few milliseconds each.
//
// Everything else here is plain geometry on those five points.

#pragma once

#include <QString>

#include <opencv2/core.hpp>
#include <opencv2/objdetect/face.hpp>

#include <array>
#include <vector>

// The five points, in the order YuNet reports them. "Right" and "left" are
// the person's own, so on an unmirrored picture the right eye is on the left.
enum Landmark {
    RightEye = 0,
    LeftEye = 1,
    NoseTip = 2,
    RightMouth = 3,
    LeftMouth = 4,
};

struct Face {
    cv::Rect2f box;
    std::array<cv::Point2f, 5> points;
    float score = 0;
    // YuNet's own row, which the recognizer wants back for its alignment.
    cv::Mat row;

    float interocular() const;
    cv::Point2f eyeMid() const;
    cv::Point2f mouthMid() const;
};

// Where the head points, read off the five points alone.
//
// yaw is the nose's sideways offset from the line through the middle of the
// eyes and the middle of the mouth, in interocular distances. A nose sits
// about half an interocular distance in front of the face, so this is close to
// 0.5 * sin(head yaw). Positive when the head turns to the person's left.
//
// noseT is how far down the nose tip sits between the eye line (0) and the
// mouth line (1). It changes with pitch, but where it sits for a level head is
// different for every face, so it only means something next to the same
// person's own resting value.
struct HeadPose {
    float roll = 0;
    float yaw = 0;
    float noseT = 0;
};

HeadPose estimatePose(const Face &face);

// Degrees, for people. The estimate is rough by nature.
float yawDegrees(float yaw);

struct FaceQuality {
    float brightness = 0;
    float sharpness = 0;
    bool tooSmall = false;
    bool tooDark = false;
    bool tooBright = false;
    bool blurry = false;

    bool ok() const
    {
        return !tooSmall && !tooDark && !tooBright && !blurry;
    }
};

FaceQuality assessQuality(const cv::Mat &bgr, const Face &face);

using Embedding = std::vector<float>;

class Vision
{
public:
    bool load(const QString &modelDir, QString *error);
    bool isLoaded() const
    {
        return m_detector && m_recognizer;
    }

    // Faces sorted by size, largest first.
    std::vector<Face> detect(const cv::Mat &bgr);

    // Unit length, so the similarity of two is their dot product.
    Embedding embed(const cv::Mat &bgr, const Face &face);

    static float similarity(const Embedding &a, const Embedding &b);

    static constexpr int EmbeddingSize = 128;

private:
    cv::Ptr<cv::FaceDetectorYN> m_detector;
    cv::Ptr<cv::FaceRecognizerSF> m_recognizer;
    cv::Size m_inputSize;
};
