// SPDX-License-Identifier: GPL-3.0-or-later
//
// The window the bubble lives in.
//
// A layer-shell surface on the overlay layer, anchored to the top edge and
// transparent to input, so it floats above everything the way the island on a
// phone does and never takes a click or a key from what is under it.
//
// On top of that it asks KWin to keep showing it while the screen is locked
// (kde_lockscreen_overlay_v1). KWin only grants that to programs whose desktop
// file lists the interface, which the one installed with this does. The
// request has to be made for every new surface role, before it is mapped, so
// it is repeated each time the window is shown again.
//
// Other compositors have no such thing. On Hyprland and Niri the lock screen
// covers the bubble, which shows again with the tick once it is gone. GNOME
// has no layer-shell at all, so there is no bubble there (see main.cpp).

#pragma once

#include <QObject>
#include <QPointer>

#include <memory>

class QQuickView;
class QQmlEngine;
class BubbleController;
class LockscreenOverlay;

class BubbleWindow : public QObject
{
    Q_OBJECT
public:
    BubbleWindow(QQmlEngine *engine, BubbleController *controller, QObject *parent = nullptr);
    ~BubbleWindow() override;

private:
    void create();
    void update();
    void allowOverLockscreen();

    QQmlEngine *m_engine;
    BubbleController *m_controller;
    QPointer<QQuickView> m_view;
    std::unique_ptr<LockscreenOverlay> m_overlay;
};
