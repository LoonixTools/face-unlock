// SPDX-License-Identifier: GPL-3.0-or-later
//
// The bubble's state on the session bus, for GNOME. GNOME lets no program
// draw above its windows or its lock screen; only a GNOME Shell extension
// may. face-unlock's extension (res/gnome-shell) draws the bubble from what
// this says: io.github.loonixtools.FaceUnlock, /io/github/loonixtools/FaceUnlock.

#pragma once

#include <QObject>
#include <QVariantMap>

class BubbleController;

class BubbleService : public QObject
{
    Q_OBJECT
    Q_CLASSINFO("D-Bus Interface", "io.github.loonixtools.FaceUnlock.Bubble")
public:
    explicit BubbleService(BubbleController *bubble, QObject *parent = nullptr);

    // phase, message, faceSeen, style, pace: what BubbleController has.
    Q_SCRIPTABLE QVariantMap GetState() const;

Q_SIGNALS:
    Q_SCRIPTABLE void StateChanged(const QVariantMap &state);

private:
    BubbleController *m_bubble;
};
