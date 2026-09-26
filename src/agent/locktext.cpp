// SPDX-License-Identifier: GPL-3.0-or-later

#include "locktext.h"

#include "bubblecontroller.h"
#include "lockers.h"
#include "userconfig.h"

#include <KLocalizedString>

#include <QDir>
#include <QFile>
#include <QFileInfo>
#include <QSaveFile>
#include <QStandardPaths>
#include <QTimer>

#include <csignal>

LockText::LockText(BubbleController *bubble, UserConfig *config, QObject *parent)
    : QObject(parent)
    , m_bubble(bubble)
    , m_config(config)
{
    connect(m_bubble, &BubbleController::phaseChanged, this, &LockText::update);
    connect(m_bubble, &BubbleController::messageChanged, this, &LockText::update);
    connect(m_config, &UserConfig::changed, this, &LockText::update);
    // Nothing left over from an agent before this one.
    write({});
}

LockText::~LockText()
{
    QFile::remove(path());
}

QString LockText::path()
{
    return QStandardPaths::writableLocation(QStandardPaths::RuntimeLocation) + QStringLiteral("/face-unlock/lock-text");
}

QString LockText::text(const QString &phase, const QString &message)
{
    if (phase == u"scanning") {
        return message.isEmpty() ? i18n("Looking for your face…") : message;
    }
    if (phase == u"success") {
        return i18n("Face recognized");
    }
    // failure and lockout say why; hidden says nothing.
    return phase == u"hidden" ? QString() : message;
}

void LockText::update()
{
    const bool on = m_config->lockScreenStyle() == u"yours";
    const QString now = on ? text(m_bubble->phase(), m_bubble->message()) : QString();
    if (now != m_text) {
        write(now);
        // Left alone by those who did not pick it: their hyprlock has no
        // label to read it.
        m_tries = 0;
        refresh();
    }
}

void LockText::refresh()
{
    // A hyprlock that is only starting reads the file once it is up, but
    // may have read it just before this changed: asked again then.
    if (Lockers::signal(SIGUSR2, "hyprlock") > 0 && ++m_tries < 20) {
        QTimer::singleShot(250, this, &LockText::refresh);
    }
}

void LockText::write(const QString &text)
{
    m_text = text;
    QDir().mkpath(QFileInfo(path()).absolutePath());
    QSaveFile file(path());
    // hyprlock reads labels as Pango markup.
    if (file.open(QIODevice::WriteOnly)) {
        file.write(text.toHtmlEscaped().toUtf8());
        file.commit();
    }
}
