// SPDX-License-Identifier: GPL-3.0-or-later
//
// The user's own lock screen, rebuilt for its picture in the window that asks
// which lock screen to use (LockChoice.qml): what hyprlock draws from its
// config, with face-unlock's line at the top, or swaylock's background and
// ring. A screenshot would take locking the screen. LockPreview.qml draws
// what this returns.
//
// hyprlock places a widget from the corner or middle its halign and valign
// name, with y upwards, and takes font sizes as points. Colours and sizes
// here are hyprlang's: rgba(r, g, b, a), rgba(RRGGBBAA), 0xAARRGGBB, and
// "x, y" in pixels or percent of the screen.

#pragma once

#include <QColor>
#include <QSizeF>
#include <QString>
#include <QUrl>
#include <QVariantList>

namespace LockPreview
{
// The lock screen the user starts: the one their idle daemon or key runs,
// else one that is installed. hyprlock, swaylock, another name, or empty.
QString locker();

// That lock screen's widgets, bottom first, as maps for LockPreview.qml.
// output names the screen, for widgets meant for one monitor; wallpaper is
// the desktop's picture, for a background that is a screenshot.
QVariantList widgets(const QString &locker, const QSizeF &screen, const QString &output, const QUrl &wallpaper);

// The pieces, apart for the tests.
QVariantList hyprlock(const QString &configPath, const QSizeF &screen, const QString &output, const QUrl &wallpaper);
QVariantList swaylock(const QString &configPath, const QUrl &wallpaper);
QColor color(const QString &value);
}
