// SPDX-License-Identifier: GPL-3.0-or-later
//
// What the bubble shows, and for how long.
//
// Two things drive it: the lock screen (LockController, which runs its own
// scans) and the daemon, which tells the agent about every other scan (sudo,
// an admin prompt, a test from the menu). Both speak through the same few
// calls, so the bubble looks the same whatever asked for the scan.

#pragma once

#include <QObject>
#include <QTimer>

class UserConfig;

class BubbleController : public QObject
{
    Q_OBJECT
    // hidden, scanning, success, failure, lockout, busy (another program has
    // the camera)
    Q_PROPERTY(QString phase READ phase NOTIFY phaseChanged)
    // A short line under the face in the full style: a hint while
    // scanning, the reason after a failure.
    Q_PROPERTY(QString message READ message NOTIFY messageChanged)
    Q_PROPERTY(bool faceSeen READ faceSeen NOTIFY faceSeenChanged)
    Q_PROPERTY(QString style READ style NOTIFY styleChanged)
    // Whether the window should be on screen. Stays true after the phase
    // goes back to hidden until the bubble has finished closing.
    Q_PROPERTY(bool shown READ shown NOTIFY shownChanged)
    // The animation speed setting, as a multiple of each animation's length.
    Q_PROPERTY(qreal pace READ pace NOTIFY paceChanged)
    // The screen it shows on by output name, empty to follow the mouse.
    Q_PROPERTY(QString screen READ screen NOTIFY screenChanged)
public:
    explicit BubbleController(UserConfig *config, QObject *parent = nullptr);

    QString phase() const
    {
        return m_phase;
    }
    QString message() const
    {
        return m_message;
    }
    bool faceSeen() const
    {
        return m_faceSeen;
    }
    QString style() const;
    bool shown() const
    {
        return m_shown;
    }
    qreal pace() const;
    QString screen() const;

    // For the pictures in the windows that ask something: shows even with
    // the bubble turned off.
    void setPreview(bool preview)
    {
        m_preview = preview;
    }

    // Invokable for those pictures, which play a scan.
    Q_INVOKABLE void scanStarted();
    Q_INVOKABLE void faceFound();
    void hint(const QString &hint);
    Q_INVOKABLE void succeeded();
    // reason is the daemon's; lockout is seconds left when there is one.
    void failed(const QString &reason, qint64 lockout = 0);
    Q_INVOKABLE void dismiss();

    // An event from the daemon about a scan somebody else asked for.
    void daemonEvent(const QJsonObject &event);

    // The QML side calls this when the closing animation is over.
    Q_INVOKABLE void closed();

Q_SIGNALS:
    void phaseChanged();
    void messageChanged();
    void faceSeenChanged();
    void styleChanged();
    void shownChanged();
    void paceChanged();
    void screenChanged();

private:
    void setPhase(const QString &phase);
    void setMessage(const QString &message);
    bool enabled() const;

    UserConfig *m_config;
    QString m_phase = QStringLiteral("hidden");
    QString m_message;
    bool m_faceSeen = false;
    bool m_shown = false;
    bool m_preview = false;
    QTimer m_hide;
};
