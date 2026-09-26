// SPDX-License-Identifier: GPL-3.0-or-later
//
// The desktop's picture for face-unlock's own lock screen, from what the
// wallpaper programs say (real output of swaybg 1.2, awww 0.12, swww 0.9,
// hyprpaper 0.7 and 0.8, wpaperd 1.3), and what the setting picks.

#include "wallpaper.h"

#include <QDir>
#include <QFile>
#include <QTemporaryDir>

#include <cstdio>

namespace
{
int failures = 0;

void check(bool ok, const char *what)
{
    std::printf("%s  %s\n", ok ? "ok  " : "FAIL", what);
    if (!ok) {
        ++failures;
    }
}

using Map = QHash<QString, QString>;
} // namespace

int main()
{
    check(Wallpaper::fromSwaybg({QStringLiteral("-i"), QStringLiteral("/w/a.jpg"), QStringLiteral("-m"), QStringLiteral("fill")})
              == Map{{QString(), QStringLiteral("/w/a.jpg")}},
          "swaybg: one picture everywhere");
    check(Wallpaper::fromSwaybg({QStringLiteral("-o"), QStringLiteral("eDP-1"), QStringLiteral("-i"), QStringLiteral("/w/a.jpg"), QStringLiteral("--output=DP-2"),
                                 QStringLiteral("--image=/w/b.png"), QStringLiteral("-o"), QStringLiteral("*"), QStringLiteral("-c"), QStringLiteral("000000")})
              == Map{{QStringLiteral("eDP-1"), QStringLiteral("/w/a.jpg")}, {QStringLiteral("DP-2"), QStringLiteral("/w/b.png")}},
          "swaybg: one per output, a colour is no picture");

    check(Wallpaper::fromAwww(QStringLiteral(": winit: 1280x742, scale: 1, currently displaying: image: /w/my pic.jpg\n"))
              == Map{{QStringLiteral("winit"), QStringLiteral("/w/my pic.jpg")}},
          "awww: with the namespace in front, spaces in the path");
    check(Wallpaper::fromAwww(QStringLiteral("eDP-1: 1920x1080, scale: 1, currently displaying: image: /w/a.jpg\n"
                                             "DP-2: 2560x1440, scale: 1, currently displaying: color: 000000\n"))
              == Map{{QStringLiteral("eDP-1"), QStringLiteral("/w/a.jpg")}},
          "swww: a colour is no picture");

    check(Wallpaper::fromList(QStringLiteral("WAYLAND-1: /w/a.jpg\nDP-2: /w/b.png\n"))
              == Map{{QStringLiteral("WAYLAND-1"), QStringLiteral("/w/a.jpg")}, {QStringLiteral("DP-2"), QStringLiteral("/w/b.png")}},
          "hyprpaper 0.8 and wpaperd");
    check(Wallpaper::fromList(QStringLiteral("eDP-1 = /w/a.jpg\n")) == Map{{QStringLiteral("eDP-1"), QStringLiteral("/w/a.jpg")}}, "hyprpaper 0.7");
    check(Wallpaper::fromList(QStringLiteral("no wallpapers active\n")).isEmpty(), "nothing shown, nothing found");

    QTemporaryDir dir;
    QFile a(dir.filePath(QStringLiteral("a.jpg")));
    check(a.open(QIODevice::WriteOnly), "a picture to point at");
    a.close();
    const QUrl url = QUrl::fromLocalFile(a.fileName());

    check(Wallpaper::forLock(QStringLiteral("none")).isEmpty(), "none is none");
    check(Wallpaper::forLock(a.fileName()) == QHash<QString, QUrl>{{QString(), url}}, "a picture set goes everywhere");
    check(Wallpaper::forLock(dir.path()) == QHash<QString, QUrl>{{QString(), url}}, "a folder set gives one from it");
    check(Wallpaper::forLock(dir.filePath(QStringLiteral("gone.jpg"))).isEmpty(), "a picture that is gone gives none");

    const QUrl b = QUrl::fromLocalFile(QStringLiteral("/w/b.png"));
    const QHash<QString, QUrl> both{{QStringLiteral("eDP-1"), url}, {QString(), b}};
    check(Wallpaper::pick(both, QStringLiteral("eDP-1")) == url, "an output gets its own");
    check(Wallpaper::pick(both, QStringLiteral("DP-2")) == b, "else the one for all");
    check(Wallpaper::pick({{QStringLiteral("eDP-1"), url}}, QStringLiteral("winit")) == url, "else any of the desktop's");
    check(Wallpaper::pick({}, QStringLiteral("eDP-1")).isEmpty(), "and none when there is none");

    std::printf("\n%s\n", failures ? "SOME CHECKS FAILED" : "all checks passed");
    return failures ? 1 : 0;
}
