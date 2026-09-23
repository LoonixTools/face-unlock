// SPDX-License-Identifier: GPL-3.0-or-later

#include "camera.h"

#include <QDir>
#include <QFile>
#include <QFileInfo>

#include <opencv2/imgcodecs.hpp>
#include <opencv2/imgproc.hpp>

#include <fcntl.h>
#include <linux/videodev2.h>
#include <sys/ioctl.h>
#include <unistd.h>

#include <algorithm>
#include <thread>

using namespace std::chrono;

namespace
{
constexpr int Width = 640;
constexpr int Height = 480;
// Pictures stand in for a camera at this rate, each held for a while so the
// checks see a still face the way they would see a person holding still.
constexpr double ImageFrameMs = 1000.0 / 15.0;
constexpr int ImageRepeat = 12;

QString sysName(const QString &device)
{
    QFile file(QStringLiteral("/sys/class/video4linux/%1/name").arg(QFileInfo(device).fileName()));
    if (!file.open(QIODevice::ReadOnly)) {
        return {};
    }
    return QString::fromUtf8(file.readAll()).trimmed();
}

// UVC names a node "<device>: <function>", often twice the same or with the
// second cut short ("HP HD Camera: HP HD Camera"). The function tells a
// laptop's two cameras apart ("HP HD Camera: HP IR Camera"), so that part is
// kept unless it only repeats the first.
QString shortName(const QString &name)
{
    const qsizetype colon = name.indexOf(u": ");
    if (colon < 0) {
        return name;
    }
    const QString device = name.left(colon);
    const QString function = name.mid(colon + 2).trimmed();
    return function.isEmpty() || device.startsWith(function) ? device : function;
}

// Opens the node just long enough to ask what it is. Neither this nor the
// format list below starts streaming, so it does not switch the light on.
bool probe(const QString &path, CameraInfo *info)
{
    const int fd = ::open(QFile::encodeName(path).constData(), O_RDONLY | O_NONBLOCK | O_CLOEXEC);
    if (fd < 0) {
        return false;
    }

    v4l2_capability cap{};
    bool ok = ::ioctl(fd, VIDIOC_QUERYCAP, &cap) == 0;
    if (ok) {
        const quint32 caps = (cap.capabilities & V4L2_CAP_DEVICE_CAPS) ? cap.device_caps : cap.capabilities;
        ok = (caps & V4L2_CAP_VIDEO_CAPTURE) && !(caps & V4L2_CAP_META_CAPTURE);
    }

    bool colour = false;
    bool grey = false;
    if (ok) {
        for (quint32 i = 0;; ++i) {
            v4l2_fmtdesc fmt{};
            fmt.index = i;
            fmt.type = V4L2_BUF_TYPE_VIDEO_CAPTURE;
            if (::ioctl(fd, VIDIOC_ENUM_FMT, &fmt) != 0) {
                break;
            }
            switch (fmt.pixelformat) {
            case V4L2_PIX_FMT_GREY:
            case V4L2_PIX_FMT_Y10:
            case V4L2_PIX_FMT_Y12:
            case V4L2_PIX_FMT_Y16:
                grey = true;
                break;
            default:
                colour = true;
                break;
            }
        }
        ok = colour || grey;
    }
    ::close(fd);

    if (ok && info) {
        info->path = path;
        info->name = sysName(path);
        if (info->name.isEmpty()) {
            info->name = QString::fromUtf8(reinterpret_cast<const char *>(cap.card));
        }
        info->name = shortName(info->name);
        info->infrared = grey && !colour;
    }
    return ok;
}
} // namespace

Camera::Camera() = default;

Camera::~Camera()
{
    close();
}

QList<CameraInfo> Camera::list()
{
    QList<CameraInfo> result;
    const QDir dev(QStringLiteral("/dev"));
    QStringList nodes = dev.entryList({QStringLiteral("video*")}, QDir::System);
    std::sort(nodes.begin(), nodes.end(), [](const QString &a, const QString &b) {
        return a.mid(5).toInt() < b.mid(5).toInt();
    });
    for (const QString &node : std::as_const(nodes)) {
        CameraInfo info;
        if (probe(dev.filePath(node), &info)) {
            result.append(info);
        }
    }
    return result;
}

QString Camera::autoPath()
{
    const QList<CameraInfo> cameras = list();
    for (const CameraInfo &c : cameras) {
        if (!c.infrared) {
            return c.path;
        }
    }
    return cameras.isEmpty() ? QString() : cameras.first().path;
}

bool Camera::open(const QString &spec, QString *error)
{
    close();
    m_start = steady_clock::now();
    m_next = m_start;

    if (spec.startsWith(u"images:")) {
        return openImages(spec.mid(7), error);
    }

    if (spec.startsWith(u"file:")) {
        const QString path = spec.mid(5);
        if (!m_capture.open(QFile::encodeName(path).toStdString(), cv::CAP_ANY) || !m_capture.isOpened()) {
            *error = QStringLiteral("cannot open video file %1").arg(path);
            return false;
        }
        m_paced = true;
        m_open = true;
        m_description = QFileInfo(path).fileName();
        return true;
    }

    QString path = spec;
    if (path.isEmpty() || path == u"auto") {
        path = autoPath();
        if (path.isEmpty()) {
            *error = QStringLiteral("no camera found");
            return false;
        }
    }

    CameraInfo info;
    if (!probe(path, &info)) {
        *error = QStringLiteral("%1 is not a camera").arg(path);
        return false;
    }

    if (!m_capture.open(QFile::encodeName(path).toStdString(), cv::CAP_V4L2) || !m_capture.isOpened()) {
        *error = QStringLiteral("cannot open %1 (in use by another program?)").arg(path);
        return false;
    }

    // MJPEG gets a webcam its full frame rate over USB 2; raw YUYV at 640x480
    // often tops out at 15 fps. A camera that has no MJPEG ignores the request.
    if (!info.infrared) {
        m_capture.set(cv::CAP_PROP_FOURCC, cv::VideoWriter::fourcc('M', 'J', 'P', 'G'));
    }
    m_capture.set(cv::CAP_PROP_FRAME_WIDTH, Width);
    m_capture.set(cv::CAP_PROP_FRAME_HEIGHT, Height);
    m_capture.set(cv::CAP_PROP_FPS, 30);
    // A stale frame in the driver's queue is a picture of whoever sat there a
    // moment ago. Keep the queue as short as the driver allows.
    m_capture.set(cv::CAP_PROP_BUFFERSIZE, 1);

    m_infrared = info.infrared;
    m_paced = false;
    m_open = true;
    m_description = QStringLiteral("%1 (%2)").arg(info.name, path);
    return true;
}

bool Camera::openImages(const QString &dir, QString *error)
{
    const QDir d(dir);
    const QStringList files = d.entryList({QStringLiteral("*.jpg"), QStringLiteral("*.jpeg"), QStringLiteral("*.png")},
                                          QDir::Files, QDir::Name);
    for (const QString &f : files) {
        cv::Mat img = cv::imread(QFile::encodeName(d.filePath(f)).toStdString(), cv::IMREAD_COLOR);
        if (img.empty()) {
            continue;
        }
        const double scale = double(Width) / std::max(img.cols, img.rows);
        if (scale < 1.0) {
            cv::resize(img, img, {}, scale, scale, cv::INTER_AREA);
        }
        m_images.append(img);
    }
    if (m_images.isEmpty()) {
        *error = QStringLiteral("no pictures in %1").arg(dir);
        return false;
    }
    m_imageIndex = 0;
    m_imageRepeat = 0;
    m_paced = true;
    m_open = true;
    m_description = QStringLiteral("pictures in %1").arg(dir);
    return true;
}

void Camera::close()
{
    if (m_capture.isOpened()) {
        m_capture.release();
    }
    m_images.clear();
    m_open = false;
    m_infrared = false;
}

bool Camera::isOpen() const
{
    return m_open;
}

void Camera::pace(double frameMs)
{
    m_next += duration_cast<steady_clock::duration>(duration<double, std::milli>(frameMs));
    const auto now = steady_clock::now();
    if (m_next > now) {
        std::this_thread::sleep_until(m_next);
    } else {
        m_next = now;
    }
}

bool Camera::readImages(cv::Mat &bgr)
{
    pace(ImageFrameMs);
    bgr = m_images.at(m_imageIndex).clone();
    if (++m_imageRepeat >= ImageRepeat) {
        m_imageRepeat = 0;
        m_imageIndex = (m_imageIndex + 1) % m_images.size();
    }
    return true;
}

bool Camera::read(cv::Mat &bgr, double *timestampMs)
{
    if (!m_open) {
        return false;
    }

    bool ok;
    if (!m_images.isEmpty()) {
        ok = readImages(bgr);
    } else {
        if (m_paced) {
            double fps = m_capture.get(cv::CAP_PROP_FPS);
            if (!(fps > 1 && fps < 240)) {
                fps = 30;
            }
            pace(1000.0 / fps);
        }
        ok = m_capture.read(bgr) && !bgr.empty();
    }
    if (!ok) {
        return false;
    }

    if (bgr.channels() == 1) {
        cv::cvtColor(bgr, bgr, cv::COLOR_GRAY2BGR);
    }
    if (timestampMs) {
        *timestampMs = duration<double, std::milli>(steady_clock::now() - m_start).count();
    }
    return true;
}
