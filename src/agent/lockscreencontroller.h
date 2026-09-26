// SPDX-License-Identifier: GPL-3.0-or-later
//
// The own lock screen's side of things (see SessionLock): the password,
// whether it is being checked, what to say under it.
//
// The password is checked with PAM in a thread of its own, the way swaylock
// and hyprlock do it. The service is face-unlock-lock when an administrator
// has written one, else the distribution's plain password stack:
// password-auth (Fedora), common-auth (Debian, Ubuntu) or login. Not the face
// module: the face has its own way in here, the agent.

#pragma once

#include <QObject>
#include <QString>

class LockScreenController : public QObject
{
    Q_OBJECT
    Q_PROPERTY(QString password READ password WRITE setPassword NOTIFY passwordChanged)
    Q_PROPERTY(bool busy READ busy NOTIFY busyChanged)
    // After a wrong password, until the next key.
    Q_PROPERTY(QString message READ message NOTIFY messageChanged)
    // Whether the face is looked for, which changes what the screen says.
    Q_PROPERTY(bool faceUnlock READ faceUnlock NOTIFY faceUnlockChanged)
    Q_PROPERTY(QString userName READ userName CONSTANT)
    // Whether to blur the picture behind it.
    Q_PROPERTY(bool blur READ blur NOTIFY blurChanged)
public:
    explicit LockScreenController(QObject *parent = nullptr);
    ~LockScreenController() override;

    QString password() const
    {
        return m_password;
    }
    void setPassword(const QString &password);
    bool busy() const
    {
        return m_busy;
    }
    QString message() const
    {
        return m_message;
    }
    bool faceUnlock() const
    {
        return m_faceUnlock;
    }
    void setFaceUnlock(bool on);
    QString userName() const;
    bool blur() const
    {
        return m_blur;
    }
    void setBlur(bool blur);

    // Check the password typed so far.
    Q_INVOKABLE void submit();
    // A key or the pointer on the lock screen.
    Q_INVOKABLE void activity();

    // A fresh lock: no password, nothing said.
    void reset();

Q_SIGNALS:
    void passwordChanged();
    void busyChanged();
    void messageChanged();
    void faceUnlockChanged();
    void blurChanged();
    void authenticated();
    // A wrong password, for the field to shake.
    void rejected();
    void input();

private:
    void finish(bool ok);
    void clearPassword();

    QString m_password;
    QString m_message;
    bool m_busy = false;
    bool m_faceUnlock = true;
    bool m_blur = false;
};
