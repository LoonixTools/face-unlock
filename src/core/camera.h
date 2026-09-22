// SPDX-License-Identifier: GPL-3.0-or-later
//
// Where frames come from.
//
// Normally a V4L2 device. For testing without a camera (a virtual machine, a
// build server) a video file or a folder of pictures stands in for one and is
// played back at the pace a camera would deliver it, so that anything timed in
// the liveness checks behaves the way it would with the real thing.

#pragma once

#include <QList>
#include <QString>

#include <opencv2/core.hpp>
#include <opencv2/videoio.hpp>

#include <chrono>
#include <memory>

struct CameraInfo {
    QString path;
    QString name;
    // Infrared cameras (the kind Windows Hello uses) only offer grey formats.
    bool infrared = false;
};

class Camera
{
public:
    Camera();
    ~Camera();

    // spec is a /dev/video path, "auto", "file:<video>" or "images:<dir>".
    bool open(const QString &spec, QString *error);
    void close();
    bool isOpen() const;

    // Blocks until the next frame. The timestamp is in milliseconds on a
    // monotonic clock and only means something relative to other frames.
    bool read(cv::Mat &bgr, double *timestampMs);

    bool isInfrared() const
    {
        return m_infrared;
    }
    QString description() const
    {
        return m_description;
    }

    // Every V4L2 device that can capture video. Metadata nodes, which every
    // UVC camera also exposes, are left out.
    static QList<CameraInfo> list();
    // What "auto" means on this machine: the first colour camera, else the
    // first camera at all.
    static QString autoPath();

private:
    bool openImages(const QString &dir, QString *error);
    bool readImages(cv::Mat &bgr);
    void pace(double frameMs);

    cv::VideoCapture m_capture;
    bool m_open = false;
    bool m_infrared = false;
    bool m_paced = false;
    QString m_description;

    QList<cv::Mat> m_images;
    qsizetype m_imageIndex = 0;
    int m_imageRepeat = 0;

    std::chrono::steady_clock::time_point m_start;
    std::chrono::steady_clock::time_point m_next;
};
