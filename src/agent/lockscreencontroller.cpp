// SPDX-License-Identifier: GPL-3.0-or-later

#include "lockscreencontroller.h"

#include <KLocalizedString>

#include <QCoreApplication>
#include <QFile>
#include <QPointer>

#include <security/pam_appl.h>

#include <cstring>
#include <pwd.h>
#include <thread>
#include <unistd.h>

namespace
{
struct Conversation {
    QByteArray password;
};

// Answers every hidden question with the password and every other one with
// nothing, as a lock screen that only asks for the password has to.
int converse(int count, const pam_message **messages, pam_response **responses, void *data)
{
    auto *conversation = static_cast<Conversation *>(data);
    auto *answers = static_cast<pam_response *>(calloc(size_t(count), sizeof(pam_response)));
    if (!answers) {
        return PAM_BUF_ERR;
    }
    for (int i = 0; i < count; ++i) {
        if (messages[i]->msg_style == PAM_PROMPT_ECHO_OFF) {
            answers[i].resp = strdup(conversation->password.constData());
        } else if (messages[i]->msg_style == PAM_PROMPT_ECHO_ON) {
            answers[i].resp = strdup("");
        }
    }
    *responses = answers;
    return PAM_SUCCESS;
}

QByteArray serviceName()
{
    static const char *const services[] = {"face-unlock-lock", "password-auth", "common-auth", "login"};
    static const char *const dirs[] = {"/etc/pam.d/", "/usr/lib/pam.d/", "/usr/share/pam.d/"};
    for (const char *service : services) {
        for (const char *dir : dirs) {
            if (QFile::exists(QString::fromLatin1(dir) + QString::fromLatin1(service))) {
                return service;
            }
        }
    }
    return "login";
}

bool checkPassword(const QByteArray &user, const QByteArray &password)
{
    Conversation conversation{password};
    const pam_conv conv{converse, &conversation};
    pam_handle_t *pamh = nullptr;
    // For development only, like the daemon's --socket: a directory of PAM
    // files of its own, so a test never counts as a wrong password for the
    // real account.
    const QByteArray confdir = qgetenv("FU_PAM_CONFDIR");
    int rc = confdir.isEmpty() ? pam_start(serviceName().constData(), user.constData(), &conv, &pamh)
                               : pam_start_confdir("face-unlock-lock", user.constData(), &conv, confdir.constData(), &pamh);
    if (rc == PAM_SUCCESS) {
        rc = pam_authenticate(pamh, 0);
        if (rc == PAM_SUCCESS) {
            pam_setcred(pamh, PAM_REFRESH_CRED);
        }
    }
    pam_end(pamh, rc);
    std::memset(conversation.password.data(), 0, size_t(conversation.password.size()));
    return rc == PAM_SUCCESS;
}

passwd *me()
{
    return getpwuid(getuid());
}
} // namespace

LockScreenController::LockScreenController(QObject *parent)
    : QObject(parent)
{
}

LockScreenController::~LockScreenController()
{
    clearPassword();
}

void LockScreenController::setPassword(const QString &password)
{
    if (m_password == password) {
        return;
    }
    m_password = password;
    Q_EMIT passwordChanged();
    if (!m_message.isEmpty()) {
        m_message.clear();
        Q_EMIT messageChanged();
    }
}

void LockScreenController::setFaceUnlock(bool on)
{
    if (m_faceUnlock != on) {
        m_faceUnlock = on;
        Q_EMIT faceUnlockChanged();
    }
}

void LockScreenController::setBlur(bool blur)
{
    if (m_blur != blur) {
        m_blur = blur;
        Q_EMIT blurChanged();
    }
}

QString LockScreenController::userName() const
{
    const passwd *pw = me();
    if (!pw) {
        return {};
    }
    // The full name from the comment field, else the login name.
    const QString full = QString::fromLocal8Bit(pw->pw_gecos).section(u',', 0, 0).trimmed();
    return full.isEmpty() ? QString::fromLocal8Bit(pw->pw_name) : full;
}

void LockScreenController::submit()
{
    if (m_busy || m_password.isEmpty()) {
        return;
    }
    const passwd *pw = me();
    if (!pw) {
        return;
    }
    m_busy = true;
    Q_EMIT busyChanged();

    const QByteArray user(pw->pw_name);
    QByteArray password = m_password.toUtf8();
    QPointer<LockScreenController> self(this);
    std::thread([self, user, password]() mutable {
        const bool ok = checkPassword(user, password);
        std::memset(password.data(), 0, size_t(password.size()));
        QMetaObject::invokeMethod(qApp, [self, ok] {
            if (self) {
                self->finish(ok);
            }
        });
    }).detach();
}

void LockScreenController::finish(bool ok)
{
    m_busy = false;
    Q_EMIT busyChanged();
    clearPassword();
    Q_EMIT passwordChanged();
    if (ok) {
        Q_EMIT authenticated();
        return;
    }
    m_message = i18n("Wrong password");
    Q_EMIT messageChanged();
    Q_EMIT rejected();
}

void LockScreenController::activity()
{
    Q_EMIT input();
}

void LockScreenController::clearPassword()
{
    // What is still in memory of it, as far as a QString lets that happen.
    m_password.fill(u' ');
    m_password.clear();
}

void LockScreenController::reset()
{
    clearPassword();
    Q_EMIT passwordChanged();
    if (!m_message.isEmpty()) {
        m_message.clear();
        Q_EMIT messageChanged();
    }
}
