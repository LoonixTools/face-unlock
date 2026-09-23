// SPDX-License-Identifier: GPL-3.0-or-later
//
// ~/.config/plasma-face-unlock/config: what this user wants from the agent.
// Written by the menu, read here, and read again whenever it changes.

#pragma once

#include <QFileSystemWatcher>
#include <QObject>

class UserConfig : public QObject
{
    Q_OBJECT
    Q_PROPERTY(bool bubble READ bubble NOTIFY changed)
    Q_PROPERTY(QString bubbleStyle READ bubbleStyle NOTIFY changed)
    Q_PROPERTY(qreal pace READ pace NOTIFY changed)
public:
    explicit UserConfig(QObject *parent = nullptr);

    // Unlock the lock screen by face.
    bool lockScreen() const
    {
        return m_lockScreen;
    }
    // Scan when somebody comes back to a locked screen (a key, the mouse,
    // opening the lid).
    bool scanOnWake() const
    {
        return m_scanOnWake;
    }
    // Scan right as the screen locks. Off by default: somebody who locks
    // their screen on purpose is usually still sitting in front of it.
    bool scanOnLock() const
    {
        return m_scanOnLock;
    }
    bool bubble() const
    {
        return m_bubble;
    }
    // "full" (the island with the face) or "minimal" (a small pill with a
    // lock).
    QString bubbleStyle() const
    {
        return m_styleOverride.isEmpty() ? m_bubbleStyle : m_styleOverride;
    }
    // For --demo --style: try a style without writing it down.
    void overrideStyle(const QString &style)
    {
        m_styleOverride = style == u"minimal" ? QStringLiteral("minimal") : QStringLiteral("full");
        Q_EMIT changed();
    }
    // Show the bubble for sudo and admin prompts, not only the lock screen.
    bool bubbleForPrompts() const
    {
        return m_bubbleForPrompts;
    }
    // How long the bubble's animations take, as a multiple of the durations
    // in the code: the animation speed setting (fast 1, normal 1.3,
    // slow 2).
    qreal pace() const
    {
        return m_pace;
    }

    static QString path();

Q_SIGNALS:
    void changed();

private:
    void load();

    QFileSystemWatcher m_watcher;
    bool m_lockScreen = true;
    bool m_scanOnWake = true;
    bool m_scanOnLock = false;
    bool m_bubble = true;
    QString m_bubbleStyle = QStringLiteral("full");
    bool m_bubbleForPrompts = true;
    qreal m_pace = 1.3;
    QString m_styleOverride;
};
