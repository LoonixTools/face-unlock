// SPDX-License-Identifier: GPL-3.0-or-later
//
// The liveness cues against synthetic faces.
//
// There is no camera in a test, so the five landmarks are made the way a
// camera would make them: points of a head in millimetres, turned, projected
// through a pinhole the size of a laptop webcam, and jittered by the noise
// YuNet adds. A real head keeps its nose in front of its face. A photograph
// is the same five points printed flat on a card, and the card is turned and
// tilted instead of the head.
//
// What has to hold, over many random runs:
//   - a head that turns about 15 degrees each way confirms,
//   - a card turned twice as far never confirms, at any noise level a real
//     camera produces,
//   - a still head abstains (it is not a fake, there is just no evidence),
//   - a blink confirms, and the things that look like one (the whole picture
//     blurring as it moves, the light changing, one eye) do not.

#include "liveness.h"

#include <cmath>
#include <cstdio>
#include <functional>
#include <random>

namespace
{
constexpr double Focal = 600; // pixels, a 640x480 webcam with ~56 degree view
constexpr double Cx = 320, Cy = 240;
constexpr double Distance = 550; // millimetres from the camera
constexpr double Fps = 20;

struct P3 {
    double x, y, z;
};

// Person-right eye first, the way YuNet reports it. x to the image's right,
// y down, z towards the camera. Proportions of an average adult face: 63 mm
// between the eyes, the nose tip 28 mm in front of the eyes, the mouth corners
// a little in front of them too.
const std::array<P3, 5> Head = {{
    {-31.5, 0, 0},
    {31.5, 0, 0},
    {0, 42, 28},
    {-25, 72, 8},
    {25, 72, 8},
}};

P3 rotate(P3 p, double yaw, double pitch, double roll)
{
    // yaw about y (positive turns the nose to the image's right, which is the
    // person's left), pitch about x, roll about z.
    P3 a{p.x * std::cos(yaw) + p.z * std::sin(yaw), p.y, -p.x * std::sin(yaw) + p.z * std::cos(yaw)};
    P3 b{a.x, a.y * std::cos(pitch) - a.z * std::sin(pitch), a.y * std::sin(pitch) + a.z * std::cos(pitch)};
    return {b.x * std::cos(roll) - b.y * std::sin(roll), b.x * std::sin(roll) + b.y * std::cos(roll), b.z};
}

cv::Point2f project(P3 p, double offsetX, double offsetY, double distance)
{
    const double depth = distance - p.z;
    return {float(Cx + Focal * (p.x + offsetX) / depth), float(Cy + Focal * (p.y + offsetY) / depth)};
}

LivenessFrame frameFrom(const std::array<cv::Point2f, 5> &pts, double t)
{
    Face face;
    face.points = pts;
    LivenessFrame f;
    f.t = t;
    f.points = pts;
    f.interocular = face.interocular();
    f.pose = estimatePose(face);
    return f;
}

std::array<cv::Point2f, 5> noisy(std::array<cv::Point2f, 5> pts, double sigma, std::mt19937 &rng)
{
    std::normal_distribution<float> n(0.f, float(sigma));
    for (auto &p : pts) {
        p.x += n(rng);
        p.y += n(rng);
    }
    return pts;
}

// A real head over one scan: it turns from side to side by up to `amplitude`
// degrees, nods a little, drifts a little.
LivenessReading realHead(double amplitudeDeg, double sigma, std::mt19937 &rng, double seconds = 3.0)
{
    std::uniform_real_distribution<double> phase(0, 2 * M_PI);
    const double ph = phase(rng);
    LivenessAnalyzer an;
    for (int i = 0; i < int(seconds * Fps); ++i) {
        const double t = i / Fps;
        const double yaw = amplitudeDeg * M_PI / 180 * std::sin(2 * M_PI * 0.4 * t + ph);
        const double pitch = 3 * M_PI / 180 * std::sin(2 * M_PI * 0.3 * t);
        const double roll = 2 * M_PI / 180 * std::sin(2 * M_PI * 0.2 * t + 1);
        std::array<cv::Point2f, 5> pts;
        for (int k = 0; k < 5; ++k) {
            pts[k] = project(rotate(Head[k], yaw, pitch, roll), 8 * std::sin(t), 4 * std::cos(t), Distance);
        }
        an.add(frameFrom(noisy(pts, sigma, rng), t * 1000));
    }
    return an.reading();
}

// Heads are not all the same shape. This one draws a new face for every run:
// flatter and deeper noses, fuller lips, wider or narrower eyes, and turns it
// by a random amount between 12 and 22 degrees at a random noise level.
LivenessReading variedHead(std::mt19937 &rng)
{
    std::uniform_real_distribution<double> u(0, 1);
    const double eye = 29 + 5 * u(rng), noseZ = 18 + 17 * u(rng), noseY = 38 + 10 * u(rng);
    const double mouthZ = 15 * u(rng), mouthY = 65 + 15 * u(rng), mouthX = 22 + 6 * u(rng);
    const std::array<P3, 5> head = {{{-eye, 0, 0}, {eye, 0, 0}, {0, noseY, noseZ}, {-mouthX, mouthY, mouthZ}, {mouthX, mouthY, mouthZ}}};
    const double amplitude = 12 + 10 * u(rng), ph = 2 * M_PI * u(rng), sigma = 0.5 + 2.0 * u(rng);

    LivenessAnalyzer an;
    for (int i = 0; i < int(3.0 * Fps); ++i) {
        const double t = i / Fps;
        const double yaw = amplitude * M_PI / 180 * std::sin(2 * M_PI * 0.4 * t + ph);
        const double pitch = 4 * M_PI / 180 * std::sin(2 * M_PI * 0.3 * t);
        std::array<cv::Point2f, 5> pts;
        for (int k = 0; k < 5; ++k) {
            pts[k] = project(rotate(head[k], yaw, pitch, 0), 0, 0, Distance);
        }
        an.add(frameFrom(noisy(pts, sigma, rng), t * 1000));
    }
    return an.reading();
}

// A photograph of the same head, taken from straight ahead (or turned by
// `bakedYawDeg`), printed flat and then held up and turned by up to
// `amplitudeDeg`. `bend` curls the card around a vertical axis, in
// millimetres of sag at the edges, which puts some depth back into it.
LivenessReading photo(double amplitudeDeg, double sigma, std::mt19937 &rng, double bakedYawDeg = 0, double bend = 0,
                      double seconds = 3.0)
{
    std::array<P3, 5> card;
    for (int k = 0; k < 5; ++k) {
        const P3 r = rotate(Head[k], bakedYawDeg * M_PI / 180, 0, 0);
        // What the photographer's camera saw, scaled back to life size.
        const double depth = Distance - r.z;
        const double s = Distance / depth;
        card[k] = {r.x * s, r.y * s, 0};
        card[k].z = -bend * (card[k].x / 45.0) * (card[k].x / 45.0);
    }

    std::uniform_real_distribution<double> phase(0, 2 * M_PI);
    const double ph = phase(rng);
    LivenessAnalyzer an;
    for (int i = 0; i < int(seconds * Fps); ++i) {
        const double t = i / Fps;
        const double yaw = amplitudeDeg * M_PI / 180 * std::sin(2 * M_PI * 0.4 * t + ph);
        const double pitch = 0.4 * amplitudeDeg * M_PI / 180 * std::sin(2 * M_PI * 0.3 * t);
        const double roll = 4 * M_PI / 180 * std::sin(2 * M_PI * 0.2 * t);
        std::array<cv::Point2f, 5> pts;
        for (int k = 0; k < 5; ++k) {
            pts[k] = project(rotate(card[k], yaw, pitch, roll), 10 * std::sin(t), 5 * std::cos(t), Distance - 100);
        }
        an.add(frameFrom(noisy(pts, sigma, rng), t * 1000));
    }
    return an.reading();
}

// ---------------------------------------------------------------------------
// Blinks, as a series of eye measurements
// ---------------------------------------------------------------------------

struct EyeScript {
    std::function<float(int)> left;
    std::function<float(int)> right;
    std::function<float(int)> skin = [](int) { return 150.f; };
    std::function<cv::Point2f(int)> shift = [](int) { return cv::Point2f(0, 0); };
};

bool blinkRun(const EyeScript &script, std::mt19937 &rng, int frames = 60)
{
    std::normal_distribution<float> n(0.f, 0.03f);
    LivenessAnalyzer an;
    for (int i = 0; i < frames; ++i) {
        std::array<cv::Point2f, 5> pts;
        for (int k = 0; k < 5; ++k) {
            pts[k] = project(Head[k], 0, 0, Distance) + script.shift(i);
        }
        LivenessFrame f = frameFrom(pts, i * 1000.0 / 30.0);
        f.eyes.valid = true;
        f.eyes.left = std::max(0.f, script.left(i) + n(rng));
        f.eyes.right = std::max(0.f, script.right(i) + n(rng));
        f.eyes.openness = (f.eyes.left + f.eyes.right) / 2;
        f.eyes.skin = script.skin(i);
        an.add(f);
    }
    return an.reading().confirmedBy == u"blink";
}

int failures = 0;

// Printed, not judged: what a known limit looks like, so a change that moves
// it shows up in the output.
void report(const char *what, int hits, int runs)
{
    std::printf("info  %-58s %5.1f%%\n", what, 100.0 * hits / runs);
}

void expectRate(const char *what, int hits, int runs, double min, double max)
{
    const double rate = double(hits) / runs;
    const bool ok = rate >= min && rate <= max;
    std::printf("%s  %-58s %5.1f%%  (want %.0f%% to %.0f%%)\n", ok ? "ok  " : "FAIL", what, rate * 100, min * 100, max * 100);
    if (!ok) {
        ++failures;
    }
}
} // namespace

int main()
{
    std::mt19937 rng(20260922);
    constexpr int Runs = 300;

    auto depthRate = [&](auto &&make) {
        int hits = 0;
        for (int i = 0; i < Runs; ++i) {
            hits += make().confirmedBy == u"depth";
        }
        return hits;
    };

    std::printf("depth cue\n");
    expectRate("head turning 15 degrees, 1 px noise", depthRate([&] { return realHead(15, 1.0, rng); }), Runs, 0.9, 1.0);
    expectRate("head turning 15 degrees, 2 px noise", depthRate([&] { return realHead(15, 2.0, rng); }), Runs, 0.6, 1.0);
    expectRate("head turning 25 degrees, 2 px noise", depthRate([&] { return realHead(25, 2.0, rng); }), Runs, 0.9, 1.0);
    expectRate("heads of all shapes turning 12 to 22 degrees", depthRate([&] { return variedHead(rng); }), Runs, 0.9, 1.0);
    expectRate("head held still (2 degrees), 1 px noise", depthRate([&] { return realHead(2, 1.0, rng); }), Runs, 0.0, 0.05);
    expectRate("photo turned 30 degrees, 0.5 px noise", depthRate([&] { return photo(30, 0.5, rng); }), Runs, 0.0, 0.0);
    expectRate("photo turned 30 degrees, 1 px noise", depthRate([&] { return photo(30, 1.0, rng); }), Runs, 0.0, 0.01);
    expectRate("photo turned 30 degrees, 2 px noise", depthRate([&] { return photo(30, 2.0, rng); }), Runs, 0.0, 0.03);
    expectRate("photo turned 45 degrees, 1 px noise", depthRate([&] { return photo(45, 1.0, rng); }), Runs, 0.0, 0.01);
    expectRate("photo of a turned head, turned 30 degrees", depthRate([&] { return photo(30, 1.0, rng, 20); }), Runs, 0.0, 0.01);
    expectRate("photo curled by 5 mm, turned 30 degrees", depthRate([&] { return photo(30, 1.0, rng, 0, 5); }), Runs, 0.0, 0.05);
    // Curled this hard, a card has about a quarter of a real nose's depth, and
    // turned twice as far as a head would be it moves its "nose" as far. Five
    // points cannot tell that from a very flat face turned a little. Blink,
    // glare and the device edge are what is left against it.
    report("known limit: photo curled by 15 mm, turned 30 degrees", depthRate([&] { return photo(30, 1.0, rng, 0, 15); }), Runs);
    expectRate("photo turned 60 degrees, 3 px noise", depthRate([&] { return photo(60, 3.0, rng); }), Runs, 0.0, 0.05);
    expectRate("photo shaken for 6 seconds, 3 px noise", depthRate([&] { return photo(20, 3.0, rng, 0, 0, 6.0); }), Runs, 0.0, 0.05);
    expectRate("head turning 20 degrees, 3 px noise", depthRate([&] { return realHead(20, 3.0, rng); }), Runs, 0.6, 1.0);

    std::printf("\nblink cue\n");
    auto blinkRate = [&](const EyeScript &s) {
        int hits = 0;
        for (int i = 0; i < Runs; ++i) {
            hits += blinkRun(s, rng);
        }
        return hits;
    };
    const auto open = [](int) { return 0.45f; };
    const auto blink = [](int i) { return (i >= 30 && i < 34) ? 0.08f : 0.45f; };
    const auto slowClose = [](int i) { return (i >= 20 && i < 45) ? 0.08f : 0.45f; };

    expectRate("eyes open the whole time", blinkRate({open, open}), Runs, 0.0, 0.01);
    expectRate("a normal blink (130 ms)", blinkRate({blink, blink}), Runs, 0.95, 1.0);
    expectRate("eyes closed for most of a second", blinkRate({slowClose, slowClose}), Runs, 0.0, 0.01);
    expectRate("one eye only", blinkRate({blink, open}), Runs, 0.0, 0.01);
    expectRate("light dims at the same moment",
               blinkRate({blink, blink, [](int i) { return (i >= 30 && i < 34) ? 110.f : 150.f; }}), Runs, 0.0, 0.01);
    expectRate("picture jerked sideways at the same moment",
               blinkRate({blink, blink, [](int) { return 150.f; },
                          [](int i) { return i >= 32 ? cv::Point2f(25, 0) : cv::Point2f(0, 0); }}),
               Runs, 0.0, 0.01);

    std::printf("\n%s\n", failures ? "SOME CHECKS FAILED" : "all checks passed");
    return failures ? 1 : 0;
}
