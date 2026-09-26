// SPDX-License-Identifier: GPL-3.0-or-later
//
// The picture behind face-unlock's own lock screen. By default the one on
// the desktop. Hyprland, niri and the like leave the wallpaper to a program
// of its own, so it comes from whichever of the common ones runs: swaybg,
// awww (once swww), hyprpaper or wpaperd, each for every output.

#pragma once

#include <QHash>
#include <QString>
#include <QStringList>
#include <QUrl>

namespace Wallpaper
{
// The pictures for a lock by output name, "" for every other output.
// setting is LockWallpaper: empty for the desktop's, "none", a picture, or a
// folder to take one from at random.
QHash<QString, QUrl> forLock(const QString &setting);
// The one for this output.
QUrl pick(const QHash<QString, QUrl> &pictures, const QString &output);

// What the programs tell, by output name. Apart for the tests.
QHash<QString, QString> fromSwaybg(const QStringList &args);
// `awww query` and `swww query`.
QHash<QString, QString> fromAwww(const QString &text);
// `hyprctl hyprpaper listactive` and `wpaperctl all-wallpapers`.
QHash<QString, QString> fromList(const QString &text);
}
