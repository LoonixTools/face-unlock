// SPDX-License-Identifier: GPL-3.0-or-later
//
// The user's own lock screen as the window that asks which one to use shows
// it: hyprlock's and swaylock's configs read the way they read them.

#include "lockpreview.h"

#include <QDir>
#include <QFile>
#include <QTemporaryDir>
#include <QVariantMap>

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

void write(const QString &path, const QByteArray &text)
{
    QDir().mkpath(path.section(u'/', 0, -2));
    QFile f(path);
    if (f.open(QIODevice::WriteOnly)) {
        f.write(text);
    }
}

QList<QVariantMap> ofType(const QVariantList &widgets, const QString &type)
{
    QList<QVariantMap> found;
    for (const QVariant &w : widgets) {
        if (w.toMap().value(QStringLiteral("type")).toString() == type) {
            found.append(w.toMap());
        }
    }
    return found;
}
} // namespace

int main()
{
    using LockPreview::color;
    check(color(QStringLiteral("rgba(255, 128, 0, 0.5)")).rgba() == QColor(255, 128, 0, 128).rgba(), "rgba() with numbers");
    check(color(QStringLiteral("rgba(33ccffee)")) == QColor(0x33, 0xcc, 0xff, 0xee), "rgba() in hex");
    check(color(QStringLiteral("rgb(10, 20, 30)")) == QColor(10, 20, 30), "rgb() with numbers");
    check(color(QStringLiteral("0x80112233")) == QColor(0x11, 0x22, 0x33, 0x80), "0xAARRGGBB");
    check(!color(QStringLiteral("blue-ish")).isValid(), "and nothing for a word");

    QTemporaryDir dir;
    const QString conf = dir.filePath(QStringLiteral("hyprlock.conf"));
    write(conf, R"(
$font = Noto Sans Light
$white = rgba(255, 255, 255, 1.0)

background {
    monitor =
    path = screenshot
    blur_passes = 2
}

label {
    monitor =
    text = cmd[update:1000] echo "Hi ##1"   # a comment
    font_family = $font
    font_size = 30
    color = $white
    position = 0, 10%
    halign = center
    valign = top
}

label {
    monitor = HDMI-A-9
    text = only on another screen
}

source = ./more/*.conf
)");
    write(dir.filePath(QStringLiteral("more/field.conf")), R"(
input-field {
    size = 20%, 50
    outer_color = rgba(33ccffee) rgba(00ff99ee) 45deg
    zindex = 5
}
)");

    const QUrl desktop = QUrl::fromLocalFile(QStringLiteral("/w/desktop.jpg"));
    const QVariantList widgets = LockPreview::hyprlock(conf, QSizeF(1000, 500), QStringLiteral("eDP-1"), desktop);
    const auto backgrounds = ofType(widgets, QStringLiteral("background"));
    check(backgrounds.size() == 1 && backgrounds.first().value(QStringLiteral("source")).toUrl() == desktop, "a screenshot background shows the desktop's picture");
    check(backgrounds.first().value(QStringLiteral("blur")).toDouble() > 0, "and is blurred");

    const auto labels = ofType(widgets, QStringLiteral("label"));
    check(labels.size() == 2, "the label for another screen is left out, face-unlock's line is added");
    const QVariantMap hi = labels.value(0);
    check(hi.value(QStringLiteral("text")).toString() == u"Hi #1", "a command's output, ## as #, the comment gone");
    check(hi.value(QStringLiteral("family")).toString() == u"Noto Sans" && hi.value(QStringLiteral("weight")).toInt() == 300, "a variable, and the font's style word as weight");
    check(hi.value(QStringLiteral("color")).value<QColor>() == QColor(Qt::white), "a colour through a variable");
    check(qFuzzyCompare(hi.value(QStringLiteral("y")).toDouble(), 50.0), "a position in percent of the screen");
    check(labels.value(1).value(QStringLiteral("valign")).toString() == u"top", "face-unlock's line at the top");

    const auto fields = ofType(widgets, QStringLiteral("input"));
    check(fields.size() == 1, "a sourced file is read");
    check(qFuzzyCompare(fields.value(0).value(QStringLiteral("width")).toDouble(), 200.0), "a size in percent");
    const QVariantMap outer = fields.value(0).value(QStringLiteral("outer")).toMap();
    check(outer.value(QStringLiteral("colors")).toList().size() == 2 && qFuzzyCompare(outer.value(QStringLiteral("angle")).toDouble(), 45.0), "a gradient");
    check(widgets.last().toMap().value(QStringLiteral("type")).toString() == u"label", "drawn by zindex, the line on top");

    // Once the menu has put the line in, it is not there twice.
    write(conf, "label {\n    text = cmd[update:0:1] cat \"$XDG_RUNTIME_DIR/face-unlock/lock-text\"\n}\n");
    const auto ours = ofType(LockPreview::hyprlock(conf, QSizeF(1000, 500), QString(), desktop), QStringLiteral("label"));
    check(ours.size() == 1 && !ours.first().value(QStringLiteral("text")).toString().isEmpty(), "face-unlock's line once, saying what it would");

    const QString sway = dir.filePath(QStringLiteral("swaylock"));
    write(sway, "# comment\nimage=eDP-1:~/pic.png\ncolor=102030\nindicator-idle-visible\nindicator-radius=80\n");
    const QVariantList swaylock = LockPreview::swaylock(sway, desktop);
    const QVariantMap back = swaylock.value(0).toMap();
    check(back.value(QStringLiteral("source")).toUrl() == QUrl::fromLocalFile(QDir::homePath() + QStringLiteral("/pic.png")), "swaylock: the image, without its output");
    check(back.value(QStringLiteral("color")).value<QColor>() == QColor(0x10, 0x20, 0x30), "swaylock: the colour");
    check(ofType(swaylock, QStringLiteral("ring")).value(0).value(QStringLiteral("radius")).toDouble() == 80, "swaylock: the ring when it stays visible");
    check(LockPreview::swaylock(QString(), desktop).value(0).toMap().value(QStringLiteral("color")).value<QColor>() == QColor(Qt::white), "swaylock without a config: white");

    std::printf("\n%s\n", failures ? "SOME CHECKS FAILED" : "all checks passed");
    return failures ? 1 : 0;
}
