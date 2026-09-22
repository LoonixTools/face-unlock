// SPDX-License-Identifier: GPL-3.0-or-later

#include "liveness.h"

#include "cvcompat.h"

#include <algorithm>
#include <cmath>

namespace
{
// The eyes are levelled and scaled onto a fixed canvas before anything is
// measured, so the numbers below are in canvas pixels and mean the same at
// any distance and any head tilt. 64 pixels between the eyes.
constexpr int Canvas = 128;
constexpr float CanvasIod = 64.f;
constexpr int EyeY = Canvas / 2;
constexpr int RightEyeX = Canvas / 2 - int(CanvasIod / 2);
constexpr int LeftEyeX = Canvas / 2 + int(CanvasIod / 2);

// A slice through the middle of the eye, narrow enough to stay on the iris
// and tall enough to hold a fully open one (about 0.3 eye distances).
constexpr int BandHalfWidth = 7;
constexpr int BandHalfHeight = 10;
// Skin under the eye, clear of lashes and of the shadow below the lid.
constexpr int SkinTop = EyeY + 22;
constexpr int SkinBottom = EyeY + 32;
constexpr int SkinHalfWidth = 12;
// A row of the slice counts as dark below this share of the skin's
// brightness. Iris and pupil are well below it on every skin tone that
// was checked; a closed lid is skin and sits well above it.
constexpr float DarkShare = 0.7f;

// Glare: the Y floor and the chroma tolerance for "colourless and nearly
// white", in YCrCb.
constexpr int SpecularLuma = 235;
constexpr int SpecularChroma = 10;
constexpr int GlareGrid = 8;

cv::Mat levelledFace(const cv::Mat &bgr, const Face &face)
{
    const cv::Point2f mid = face.eyeMid();
    const cv::Point2f d = face.points[LeftEye] - face.points[RightEye];
    const double roll = std::atan2(d.y, d.x) * 180.0 / M_PI;
    const double scale = CanvasIod / std::max(1.f, face.interocular());

    cv::Mat m = cv::getRotationMatrix2D(mid, roll, scale);
    m.at<double>(0, 2) += Canvas / 2.0 - mid.x;
    m.at<double>(1, 2) += Canvas / 2.0 - mid.y;

    cv::Mat grey, out;
    cv::cvtColor(bgr, grey, cv::COLOR_BGR2GRAY);
    cv::warpAffine(grey, out, m, cv::Size(Canvas, Canvas), cv::INTER_LINEAR, cv::BORDER_REPLICATE);
    return out;
}

float eyeOpenness(const cv::Mat &canvas, int eyeX, float *skinOut)
{
    const cv::Rect skinRect(eyeX - SkinHalfWidth, SkinTop, 2 * SkinHalfWidth, SkinBottom - SkinTop);
    const float skin = float(cv::mean(canvas(skinRect))[0]);
    *skinOut = skin;
    if (skin < 20) {
        return -1;
    }

    const cv::Rect band(eyeX - BandHalfWidth, EyeY - BandHalfHeight, 2 * BandHalfWidth + 1, 2 * BandHalfHeight + 1);
    cv::Mat rows;
    cv::reduce(canvas(band), rows, 1, cv::REDUCE_AVG, CV_32F);

    int dark = 0;
    for (int y = 0; y < rows.rows; ++y) {
        if (rows.at<float>(y, 0) < DarkShare * skin) {
            ++dark;
        }
    }
    return float(dark) / float(rows.rows);
}

bool insidePolygon(const std::vector<cv::Point> &poly, cv::Point2f p)
{
    return cv::pointPolygonTest(poly, p, false) >= 0;
}
} // namespace

GlareSample measureGlare(const cv::Mat &bgr, const Face &face)
{
    GlareSample g;
    const float iod = face.interocular();
    const cv::Point2f centre = (face.eyeMid() + face.mouthMid()) * 0.5f;
    cv::Rect roi(int(centre.x - 1.1f * iod), int(centre.y - 1.3f * iod), int(2.2f * iod), int(2.4f * iod));
    roi &= cv::Rect(0, 0, bgr.cols, bgr.rows);
    if (roi.width < 16 || roi.height < 16) {
        return g;
    }

    cv::Mat ycc;
    cv::cvtColor(bgr(roi), ycc, cv::COLOR_BGR2YCrCb);

    // Glasses throw the screen back from right in front of the eyes, and
    // that is a big flat highlight too. It is not what is being looked for,
    // so the eyes are left out.
    cv::Mat mask(roi.size(), CV_8U, cv::Scalar(255));
    for (int e : {RightEye, LeftEye}) {
        const cv::Point2f p = face.points[e] - cv::Point2f(float(roi.x), float(roi.y));
        cv::circle(mask, p, int(0.38f * iod), cv::Scalar(0), cv::FILLED);
    }

    std::array<int, GlareGrid * GlareGrid> cells{};
    int total = 0;
    int considered = 0;
    for (int y = 0; y < ycc.rows; ++y) {
        const cv::Vec3b *row = ycc.ptr<cv::Vec3b>(y);
        const uchar *m = mask.ptr<uchar>(y);
        const int gy = std::min(GlareGrid - 1, y * GlareGrid / ycc.rows);
        for (int x = 0; x < ycc.cols; ++x) {
            if (!m[x]) {
                continue;
            }
            ++considered;
            const cv::Vec3b &p = row[x];
            if (p[0] < SpecularLuma || std::abs(p[1] - 128) > SpecularChroma || std::abs(p[2] - 128) > SpecularChroma) {
                continue;
            }
            ++total;
            const int gx = std::min(GlareGrid - 1, x * GlareGrid / ycc.cols);
            ++cells[gy * GlareGrid + gx];
        }
    }

    g.valid = considered > 0;
    g.fraction = considered ? float(total) / float(considered) : 0.f;
    // Too few pixels to say anything about their shape.
    g.cluster = total >= 24 ? float(*std::max_element(cells.begin(), cells.end())) / float(total) : 0.f;
    return g;
}

EyeSample measureEyes(const cv::Mat &bgr, const Face &face)
{
    EyeSample s;
    if (face.interocular() < 20.f) {
        return s;
    }
    const cv::Mat canvas = levelledFace(bgr, face);

    // "Right" is the person's right eye, which is on the canvas' left.
    float skinR = 0, skinL = 0;
    s.right = eyeOpenness(canvas, RightEyeX, &skinR);
    s.left = eyeOpenness(canvas, LeftEyeX, &skinL);
    if (s.right < 0 || s.left < 0) {
        return s;
    }
    s.openness = (s.left + s.right) / 2;
    s.skin = (skinL + skinR) / 2;
    s.valid = true;
    return s;
}

bool detectDevice(const cv::Mat &bgr, const Face &face)
{
    // Edges are looked for at half size. The bezel of a phone held up to the
    // camera is big, and small detail would only add rectangles that are not
    // one.
    const double scale = 320.0 / std::max(1, bgr.cols);
    cv::Mat small, grey, edges;
    cv::resize(bgr, small, cv::Size(), scale, scale, cv::INTER_AREA);
    cv::cvtColor(small, grey, cv::COLOR_BGR2GRAY);
    cv::GaussianBlur(grey, grey, cv::Size(5, 5), 0);
    cv::Canny(grey, edges, 40, 120);
    cv::dilate(edges, edges, cv::Mat(), cv::Point(-1, -1), 1);

    std::vector<std::vector<cv::Point>> contours;
    cv::findContours(edges, contours, cv::RETR_LIST, cv::CHAIN_APPROX_SIMPLE);

    const double frameArea = double(small.cols) * small.rows;
    const double faceArea = double(face.box.area()) * scale * scale;
    std::array<cv::Point2f, 5> points;
    for (int i = 0; i < 5; ++i) {
        points[i] = face.points[i] * float(scale);
    }

    for (const auto &c : contours) {
        const double area = std::abs(cv::contourArea(c));
        // A device held up to take somebody's place fills a good part of the
        // picture, and the face fills a good part of the device. A door or a
        // monitor far behind somebody's head does neither.
        if (area < 0.10 * frameArea || area > 0.95 * frameArea || area < 1.6 * faceArea || faceArea / area < 0.08) {
            continue;
        }
        std::vector<cv::Point> quad;
        cv::approxPolyDP(c, quad, 0.03 * cv::arcLength(c, true), true);
        if (quad.size() != 4 || !cv::isContourConvex(quad)) {
            continue;
        }
        const cv::RotatedRect r = cv::minAreaRect(quad);
        const float shortSide = std::min(r.size.width, r.size.height);
        const float longSide = std::max(r.size.width, r.size.height);
        if (longSide <= 0 || shortSide / longSide < 0.3f) {
            continue;
        }
        // It has to frame the face: every one of the five points inside.
        bool framed = true;
        for (const cv::Point2f &p : points) {
            framed = framed && insidePolygon(quad, p);
        }
        if (framed) {
            return true;
        }
    }
    return false;
}

LivenessFrame measureFrame(const cv::Mat &bgr, const Face &face, double timestampMs)
{
    LivenessFrame f;
    f.t = timestampMs;
    f.points = face.points;
    f.interocular = face.interocular();
    f.pose = estimatePose(face);
    f.glare = measureGlare(bgr, face);
    f.device = detectDevice(bgr, face);
    f.eyes = measureEyes(bgr, face);
    return f;
}

// ---------------------------------------------------------------------------
// The analyzer
// ---------------------------------------------------------------------------

void LivenessAnalyzer::reset()
{
    m_frames.clear();
    m_total = 0;
    m_glareHits = 0;
    m_deviceHits = 0;
    m_depthFired = false;
    m_blinkFired = false;
    m_depth.clear();
    m_depthNum = 0;
    m_depthDen = 0;
    m_depthPairs = 0;
}

void LivenessAnalyzer::add(const LivenessFrame &frame)
{
    m_frames.push_back(frame);
    while (!m_frames.empty() && frame.t - m_frames.front().t > WindowMs) {
        m_frames.pop_front();
    }

    // Deny cues count over the whole scan, not just the window: a phone that
    // was seen once does not stop having been a phone.
    ++m_total;
    if (frame.glare.valid && frame.glare.fraction >= GlareFraction && frame.glare.cluster >= GlareCluster) {
        ++m_glareHits;
    }
    if (frame.device) {
        ++m_deviceHits;
    }

    addDepth(frame);

    // Confirm cues latch once seen, for the rest of this scan.
    if (!m_depthFired) {
        m_depthFired = depthRange() >= DepthMinRange && m_depthPairs >= 6 && depthRatio() >= DepthMinRatio && depthConsistent();
    }
    if (!m_blinkFired) {
        float strength = 0;
        m_blinkFired = blinkSeen(&strength);
    }
}

// How far the nose misses the spot a flat face would put it, measured against
// how far the head turned.
//
// For a pair of frames far enough apart in yaw, the four points that lie close
// to one plane (eyes, mouth corners) give the homography between the two
// frames. A point on a flat picture lands exactly where it predicts. The real
// nose tip is about a third of an eye distance in front of that plane, so it
// lands off the prediction, sideways, by about as much as the yaw estimate
// changed: both are the same parallax, seen two ways. The slope of miss
// against yaw change is therefore near 1 for anything with a nose sticking
// out of it, and near 0 for paper or a screen.
//
// Two details decide whether that holds up against a real camera.
//
// The yaw a frame is compared at comes from its neighbours, never from the
// frame itself. Yaw and miss are both read off the same nose, so the nose's
// own jitter would push both the same way in every frame, and the slope of
// noise against itself is 1. That alone made a photo shaken at two pixels of
// jitter pass as a head. The neighbours are 50 ms away, so they see the same
// head position with jitter of their own.
//
// And the slope alone says nothing about depth, only that nose and plane
// disagree consistently. A card curled around a vertical axis has a little
// depth too, and its slope is near 1 as well. What it cannot do is move the
// nose off the midline by much, so the cue also needs the (smoothed) yaw to
// have covered DepthMinRange: about 12 degrees of a real head turning.
//
// A card that is curled hard and turned far enough still gets there. So the
// last check asks whether the face turned as far as its nose says it did.
// Turning narrows the eyes against the eye to mouth distance by 1 - cos(turn),
// whatever the face is made of. A nose as flat as a real one can be (0.3 eye
// distances) that moved DepthMinRange can only have turned so far, and a face
// that narrowed a lot more than that was turned a lot more than that: it has
// less nose than any head. See depthConsistent().
void LivenessAnalyzer::addDepth(const LivenessFrame &frame)
{
    constexpr size_t MaxFrames = 400;
    if (m_depth.size() >= MaxFrames) {
        return;
    }
    Face face;
    face.points = frame.points;
    const float emd = float(cv::norm(face.mouthMid() - face.eyeMid()));
    const float shape = emd > 1.f ? face.interocular() / emd : 0.f;
    m_depth.push_back({frame.points, frame.pose.yaw, 0.f, frame.pose.noseT, shape, 0.f});

    // The frame two back now has two neighbours on each side.
    const size_t n = m_depth.size();
    if (n < 5) {
        return;
    }
    const size_t j = n - 3;
    DepthPoint &b = m_depth[j];
    b.smoothYaw = (m_depth[j - 2].yaw + m_depth[j - 1].yaw + m_depth[j + 1].yaw + m_depth[j + 2].yaw) / 4.f;
    b.smoothShape = (m_depth[j - 2].shape + m_depth[j - 1].shape + b.shape + m_depth[j + 1].shape + m_depth[j + 2].shape) / 5.f;

    const cv::Point2f axis = b.points[LeftEye] - b.points[RightEye];
    const float axisLen = std::max(1e-3f, float(cv::norm(axis)));
    const cv::Point2f unit = axis * (1.f / axisLen);

    for (size_t i = 2; i < j; ++i) {
        const DepthPoint &a = m_depth[i];
        const float dy = b.smoothYaw - a.smoothYaw;
        if (std::abs(dy) < DepthMinTurn) {
            continue;
        }
        const cv::Point2f src[4] = {a.points[RightEye], a.points[LeftEye], a.points[LeftMouth], a.points[RightMouth]};
        const cv::Point2f dst[4] = {b.points[RightEye], b.points[LeftEye], b.points[LeftMouth], b.points[RightMouth]};
        const cv::Mat h = cv::getPerspectiveTransform(src, dst);
        if (h.empty()) {
            continue;
        }
        std::vector<cv::Point2f> in{a.points[NoseTip]}, out;
        cv::perspectiveTransform(in, out, h);
        const cv::Point2f miss = b.points[NoseTip] - out[0];
        const float rx = (miss.x * unit.x + miss.y * unit.y) / axisLen;
        m_depthNum += double(rx) * dy;
        m_depthDen += double(dy) * dy;
        ++m_depthPairs;
    }
}

float LivenessAnalyzer::depthRatio() const
{
    return m_depthDen > 0 ? float(m_depthNum / m_depthDen) : 0.f;
}

// The spread of the smoothed yaw, from the 10th to the 90th percentile, so a
// single bad frame cannot stretch it.
float LivenessAnalyzer::depthRange() const
{
    if (m_depth.size() < 5) {
        return 0;
    }
    std::vector<float> ys;
    for (size_t k = 2; k + 2 < m_depth.size(); ++k) {
        ys.push_back(m_depth[k].smoothYaw);
    }
    if (ys.size() < 3) {
        return 0;
    }
    std::sort(ys.begin(), ys.end());
    const auto at = [&](double q) {
        return ys[size_t(q * double(ys.size() - 1))];
    };
    return at(0.9) - at(0.1);
}

bool LivenessAnalyzer::depthConsistent() const
{
    // Nodding also changes the eye to mouth distance, so only frames at the
    // head's usual pitch are compared. A card has no pitch of its own to read
    // (its nose is printed on), so none of its frames are left out.
    std::vector<float> noseTs;
    for (size_t k = 2; k + 2 < m_depth.size(); ++k) {
        noseTs.push_back(m_depth[k].noseT);
    }
    if (noseTs.size() < 3) {
        return false;
    }
    std::vector<float> sortedT = noseTs;
    std::sort(sortedT.begin(), sortedT.end());
    const float medianT = sortedT[sortedT.size() / 2];

    std::vector<float> shapes;
    for (size_t k = 2; k + 2 < m_depth.size(); ++k) {
        if (std::abs(m_depth[k].noseT - medianT) < 0.05f) {
            shapes.push_back(m_depth[k].smoothShape);
        }
    }
    if (shapes.size() < 3) {
        return false;
    }
    std::sort(shapes.begin(), shapes.end());
    const float widest = shapes[size_t(0.95 * double(shapes.size() - 1))];
    const float narrowest = shapes[size_t(0.10 * double(shapes.size() - 1))];
    if (widest <= 0) {
        return false;
    }
    const float narrowed = 1.f - narrowest / widest;

    const float sinTurn = std::min(1.f, depthRange() / DepthFlattestNose);
    const float allowed = 1.f - std::sqrt(1.f - sinTurn * sinTurn);
    return narrowed <= allowed + DepthShapeSlack;
}

bool LivenessAnalyzer::blinkSeen(float *strength) const
{
    *strength = 0;
    std::vector<const LivenessFrame *> s;
    for (const LivenessFrame &f : m_frames) {
        if (f.eyes.valid) {
            s.push_back(&f);
        }
    }
    if (s.size() < 6) {
        return false;
    }

    std::vector<float> values;
    for (const LivenessFrame *f : s) {
        values.push_back(f->eyes.openness);
    }
    std::vector<float> sorted = values;
    std::sort(sorted.begin(), sorted.end());
    const float baseline = sorted[size_t(0.7 * double(sorted.size() - 1))];
    // An eye this narrow is not one the measure works on (heavy lids, very
    // dark eyes on dark skin, a camera that is too soft). Better to abstain
    // than to guess.
    if (baseline < 0.15f) {
        return false;
    }
    *strength = std::clamp(1.f - sorted.front() / baseline, 0.f, 1.f) / (1.f - BlinkDip);

    const size_t n = s.size();
    for (size_t i = 1; i + 1 < n; ++i) {
        if (values[i] >= BlinkDip * baseline) {
            continue;
        }
        // The run of closed frames around this one.
        size_t a = i, b = i;
        while (a > 0 && values[a - 1] < BlinkOpen * baseline) {
            --a;
        }
        while (b + 1 < n && values[b + 1] < BlinkOpen * baseline) {
            ++b;
        }
        if (a == 0 || b + 1 >= n) {
            continue;
        }
        // Open right before and right after, within the time a blink takes
        // (a real one is 100 to 400 ms; slower is somebody closing their
        // eyes, or a picture being tilted away and back).
        const LivenessFrame *before = s[a - 1];
        const LivenessFrame *after = s[b + 1];
        if (after->t - before->t > 600) {
            continue;
        }
        // The rest of the face held still. A picture being moved blurs and
        // shifts everything at once; a blink only moves the lids.
        const cv::Point2f moved = (after->points[NoseTip] - before->points[NoseTip]);
        const float iod = std::max(1.f, before->interocular);
        if (float(cv::norm(moved)) > 0.15f * iod) {
            continue;
        }
        if (std::abs(after->interocular - before->interocular) > 0.08f * iod) {
            continue;
        }
        bool steadyLight = true;
        for (size_t k = a; k <= b; ++k) {
            const float ratio = s[k]->eyes.skin / std::max(1.f, before->eyes.skin);
            steadyLight = steadyLight && ratio > 0.9f && ratio < 1.1f;
        }
        if (!steadyLight) {
            continue;
        }
        // Both eyes together. One eye going dark on its own is a shadow or a
        // hand, not a blink.
        bool both = false;
        for (size_t k = a; k <= b && !both; ++k) {
            both = s[k]->eyes.left < 0.7f * baseline && s[k]->eyes.right < 0.7f * baseline;
        }
        if (both) {
            return true;
        }
    }
    return false;
}

LivenessReading LivenessAnalyzer::reading() const
{
    LivenessReading r;
    r.frames = int(m_frames.size());

    const int seen = std::max(1, m_total);
    const float glareShare = float(m_glareHits) / float(seen);
    const float deviceShare = float(m_deviceHits) / float(seen);
    r.glare = std::min(glareShare / 0.3f, float(m_glareHits) / 3.f);
    r.device = std::min(deviceShare / 0.3f, float(m_deviceHits) / 3.f);

    if (m_glareHits >= 3 && glareShare >= 0.3f) {
        r.denied = true;
        r.deniedBy = QStringLiteral("glare");
    } else if (m_deviceHits >= 3 && deviceShare >= 0.3f) {
        r.denied = true;
        r.deniedBy = QStringLiteral("device");
    }

    r.depth = m_depthFired ? 1.f
                           : std::clamp(depthRatio() / DepthMinRatio, 0.f, 1.f) * std::clamp(depthRange() / DepthMinRange, 0.f, 1.f);

    float strength = 0;
    blinkSeen(&strength);
    r.blink = m_blinkFired ? 1.f : std::min(strength, 0.99f);

    if (m_depthFired) {
        r.confirmed = true;
        r.confirmedBy = QStringLiteral("depth");
    } else if (m_blinkFired) {
        r.confirmed = true;
        r.confirmedBy = QStringLiteral("blink");
    }
    return r;
}
