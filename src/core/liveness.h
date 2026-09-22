// SPDX-License-Identifier: GPL-3.0-or-later
//
// Telling a face from a picture of one.
//
// A webcam sees a flat image either way, so there is no single test that
// settles it. What is here follows the model Glance (the macOS face unlock)
// arrived at after trying and dropping a dozen weaker signals: a few cues,
// each one decisive on its own, split into two kinds.
//
// Deny cues are evidence of a fake. Either one fails the scan outright, and
// a match that already happened does not outvote it.
//
//   glare   a phone screen or a glossy print throws back one large, flat,
//           colourless highlight. Skin shines in small scattered spots.
//   device  the straight edges of a phone or a tablet around the face.
//
// Confirm cues are evidence of a real head. Any one of them is enough, and
// their absence is never held against anybody on its own: a person can sit
// still and not blink for a few seconds.
//
//   depth   when the head turns, the tip of the nose moves further than a
//           flat face would let it. The eyes and the corners of the mouth lie
//           close to one plane, so four of them predict exactly where the
//           nose of a flat picture has to go. A real nose, about a third of
//           an eye distance in front of that plane, misses the prediction by
//           about as much as the head turned.
//   blink   the dark of the eye shrinks to a line and comes back, within
//           the fraction of a second a blink takes, while the rest of the face
//           holds still.
//
// "Light" uses the deny cues. "Heavy" also needs one confirm cue before it
// lets anybody in.

#pragma once

#include "settings.h"
#include "vision.h"

#include <QString>

#include <deque>

struct GlareSample {
    bool valid = false;
    // Share of the face that is a colourless highlight.
    float fraction = 0;
    // Share of all highlight pixels that fall in the fullest of 8x8 cells.
    // One slab of glass reflects into one place; skin sparkles everywhere.
    float cluster = 0;
};

struct EyeSample {
    bool valid = false;
    // How much of the eye's height is dark (iris, pupil), per eye and on
    // average. Falls towards zero when the lid comes down.
    float left = 0;
    float right = 0;
    float openness = 0;
    // Brightness of the skin under the eyes, to tell a blink from a change
    // in the light.
    float skin = 0;
};

struct LivenessFrame {
    double t = 0;
    std::array<cv::Point2f, 5> points{};
    float interocular = 0;
    HeadPose pose;
    GlareSample glare;
    bool device = false;
    EyeSample eyes;
};

GlareSample measureGlare(const cv::Mat &bgr, const Face &face);
EyeSample measureEyes(const cv::Mat &bgr, const Face &face);
bool detectDevice(const cv::Mat &bgr, const Face &face);

LivenessFrame measureFrame(const cv::Mat &bgr, const Face &face, double timestampMs);

struct LivenessReading {
    int frames = 0;

    bool denied = false;
    QString deniedBy;

    bool confirmed = false;
    QString confirmedBy;

    // For the test screen: how close each cue is to firing, 0 to 1 and more.
    float glare = 0;
    float device = 0;
    float depth = 0;
    float blink = 0;
};

class LivenessAnalyzer
{
public:
    void reset();
    void add(const LivenessFrame &frame);
    LivenessReading reading() const;

    int frameCount() const
    {
        return int(m_frames.size());
    }

    // Tuning, in one place. See liveness.cpp for where each number comes
    // from.
    static constexpr double WindowMs = 4000;
    static constexpr float GlareFraction = 0.012f;
    static constexpr float GlareCluster = 0.55f;
    static constexpr float DepthMinTurn = 0.05f;
    static constexpr float DepthMinRange = 0.10f;
    static constexpr float DepthMinRatio = 0.65f;
    static constexpr float DepthFlattestNose = 0.30f;
    static constexpr float DepthShapeSlack = 0.02f;
    static constexpr float BlinkDip = 0.55f;
    static constexpr float BlinkOpen = 0.8f;

    // The depth cue's workings, for the test screen and the tests.
    float depthRatio() const;
    float depthRange() const;
    bool depthConsistent() const;

private:
    void addDepth(const LivenessFrame &frame);
    bool blinkSeen(float *strength) const;

    std::deque<LivenessFrame> m_frames;
    int m_total = 0;

    // The depth cue looks at the whole scan rather than the window, and is
    // worked out a frame at a time so it stays cheap.
    struct DepthPoint {
        std::array<cv::Point2f, 5> points;
        float yaw;
        float smoothYaw;
        float noseT;
        // Eye distance over eye to mouth distance: shrinks as the face turns
        // away from the camera, whatever the face is made of.
        float shape;
        float smoothShape;
    };
    std::vector<DepthPoint> m_depth;
    double m_depthNum = 0;
    double m_depthDen = 0;
    int m_depthPairs = 0;

    int m_glareHits = 0;
    int m_deviceHits = 0;
    bool m_depthFired = false;
    bool m_blinkFired = false;
};
