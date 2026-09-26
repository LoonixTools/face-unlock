// SPDX-License-Identifier: GPL-3.0-or-later
//
// The bubble as a line of text, for a lock screen that covers it: hyprlock.
// When the user keeps their own lock screen (LockScreenStyle=yours), the
// menu puts a label into hyprlock's config that shows the file at path()
// (see src/lib/system.sh). This writes what the bubble shows into it, and
// SIGUSR2 makes hyprlock read it again.

#pragma once

#include <QObject>
#include <QString>

class BubbleController;
class UserConfig;

class LockText : public QObject
{
    Q_OBJECT
public:
    LockText(BubbleController *bubble, UserConfig *config, QObject *parent = nullptr);
    ~LockText() override;

    static QString path();
    // The line for a phase and message of the bubble.
    static QString text(const QString &phase, const QString &message);

private:
    void update();
    void refresh();
    void write(const QString &text);

    BubbleController *m_bubble;
    UserConfig *m_config;
    QString m_text;
    int m_tries = 0;
};
