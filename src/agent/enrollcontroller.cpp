// SPDX-License-Identifier: GPL-3.0-or-later

#include "enrollcontroller.h"

#include "daemonclient.h"

#include <KLocalizedString>

#include <QJsonObject>

EnrollController::EnrollController(QObject *parent)
    : QObject(parent)
{
}

void EnrollController::setName(const QString &name)
{
    if (m_name != name) {
        m_name = name;
        Q_EMIT nameChanged();
    }
}

QVariantList EnrollController::sectors() const
{
    QVariantList list;
    for (bool s : m_sectors) {
        list.append(s);
    }
    return list;
}

int EnrollController::sectorsDone() const
{
    int n = 0;
    for (bool s : m_sectors) {
        n += s;
    }
    return n;
}

bool EnrollController::canFinish() const
{
    // The daemon's own minimum (EnrollJob::SectorsForEarlyFinish).
    return m_state == u"circle" && sectorsDone() >= 4 && sectorsDone() < 8;
}

void EnrollController::setState(const QString &state)
{
    if (m_state != state) {
        m_state = state;
        Q_EMIT stateChanged();
        Q_EMIT sectorsChanged();
    }
}

void EnrollController::setInstruction(const QString &text)
{
    if (m_instruction != text) {
        m_instruction = text;
        Q_EMIT instructionChanged();
    }
}

QString EnrollController::hintText(const QString &hint) const
{
    if (hint == u"no-face") {
        return i18n("Bring your face into the circle.");
    } else if (hint == u"one-face") {
        return i18n("Only one face, please.");
    } else if (hint == u"closer") {
        return i18n("Move a little closer.");
    } else if (hint == u"light") {
        return i18n("It is too dark. Turn on a light.");
    } else if (hint == u"bright") {
        return i18n("Too bright. Turn away from the light.");
    } else if (hint == u"still") {
        return i18n("Hold still for a moment.");
    } else if (hint == u"straight" || hint == u"hold") {
        return i18n("Look straight at the camera.");
    } else if (hint == u"circle") {
        return i18n("Move your head slowly to complete the circle.");
    }
    return {};
}

void EnrollController::start()
{
    if (m_request) {
        return;
    }
    for (bool &s : m_sectors) {
        s = false;
    }
    m_error.clear();
    m_frame = QImage();
    Q_EMIT frameChanged();
    setState(QStringLiteral("starting"));
    setInstruction(i18n("Starting the camera…"));

    m_request = new DaemonRequest({{QStringLiteral("cmd"), QStringLiteral("enroll")}, {QStringLiteral("name"), m_name}}, this);
    connect(m_request, &DaemonRequest::event, this, &EnrollController::onEvent);
    connect(m_request, &DaemonRequest::finished, this, &EnrollController::onFinished);
}

void EnrollController::finish()
{
    if (m_request) {
        m_request->send({{QStringLiteral("cmd"), QStringLiteral("finish")}});
    }
}

void EnrollController::cancel()
{
    if (m_request) {
        m_request->abort();
        m_request->deleteLater();
        m_request = nullptr;
    }
    Q_EMIT completed(m_state == u"done");
}

void EnrollController::onEvent(const QJsonObject &e)
{
    const QString what = e.value(u"event").toString();

    if (what == u"authorizing") {
        setState(QStringLiteral("authorizing"));
        setInstruction(i18n("Confirm with your password to add a face."));
    } else if (what == u"authorized") {
        setState(QStringLiteral("starting"));
        setInstruction(i18n("Starting the camera…"));
    } else if (what == u"started") {
        setState(QStringLiteral("center"));
        setInstruction(hintText(QStringLiteral("straight")));
    } else if (what == u"frame") {
        const QByteArray jpeg = QByteArray::fromBase64(e.value(u"jpeg").toString().toLatin1());
        QImage img;
        if (img.loadFromData(jpeg, "JPG")) {
            m_frame = img;
        }
        const QJsonObject f = e.value(u"face").toObject();
        if (!f.isEmpty()) {
            m_faceRect = QRectF(f.value(u"x").toDouble(), f.value(u"y").toDouble(), f.value(u"w").toDouble(), f.value(u"h").toDouble());
        }
        Q_EMIT frameChanged();
    } else if (what == u"pose") {
        m_faceVisible = e.value(u"face").toBool();
        if (m_faceVisible) {
            m_pose = QPointF(e.value(u"x").toDouble(), e.value(u"y").toDouble());
        }
        Q_EMIT poseChanged();
    } else if (what == u"hint") {
        const QString text = hintText(e.value(u"hint").toString());
        if (!text.isEmpty()) {
            setInstruction(text);
        }
    } else if (what == u"captured") {
        const QString pose = e.value(u"pose").toString();
        if (pose == u"center") {
            setState(QStringLiteral("circle"));
            setInstruction(hintText(QStringLiteral("circle")));
        } else {
            const int sector = e.value(u"sector").toInt(-1);
            if (sector >= 0 && sector < 8) {
                m_sectors[sector] = true;
                Q_EMIT sectorsChanged();
            }
        }
    }
}

void EnrollController::onFinished(const QJsonObject &result)
{
    if (m_request) {
        m_request->deleteLater();
        m_request = nullptr;
    }
    if (result.value(u"ok").toBool()) {
        for (bool &s : m_sectors) {
            s = true;
        }
        setState(QStringLiteral("done"));
        setInstruction(i18n("Face Unlock is set up."));
        return;
    }

    const QString reason = result.value(u"reason").toString();
    if (reason == u"cancelled") {
        return;
    }
    if (reason == u"denied") {
        m_error = i18n("A face can only be added with your password.");
    } else if (reason == u"camera") {
        m_error = i18n("The camera could not be used: %1", result.value(u"message").toString());
    } else if (reason == u"models") {
        m_error = i18n("The face recognition models are missing. Reinstall the package.");
    } else if (reason == u"timeout") {
        m_error = i18n("That took too long. Try again, in good light.");
    } else if (reason == u"unreachable") {
        m_error = i18n("The face unlock service is not running. Turn face unlock on first.");
    } else if (reason == u"busy") {
        m_error = i18n("The camera is busy with another scan. Try again in a moment.");
    } else {
        m_error = i18n("The face could not be added (%1).", reason);
    }
    setState(QStringLiteral("failed"));
    setInstruction(m_error);
}
