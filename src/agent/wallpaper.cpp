// SPDX-License-Identifier: GPL-3.0-or-later

#include "wallpaper.h"

#include <QDir>
#include <QFile>
#include <QFileInfo>
#include <QProcess>
#include <QRandomGenerator>
#include <QRegularExpression>

#include <unistd.h>

namespace
{
// A picture, or one at random from a folder.
QUrl picture(const QString &setting)
{
    QString path = setting;
    if (path.startsWith(u"~/")) {
        path = QDir::homePath() + path.mid(1);
    }
    const QFileInfo info(path);
    if (setting.isEmpty() || !info.exists()) {
        return {};
    }
    if (info.isFile()) {
        return QUrl::fromLocalFile(info.absoluteFilePath());
    }
    const QStringList pictures = QDir(path).entryList({QStringLiteral("*.jpg"), QStringLiteral("*.jpeg"), QStringLiteral("*.png"), QStringLiteral("*.webp")},
                                                      QDir::Files | QDir::Readable);
    if (pictures.isEmpty()) {
        return {};
    }
    return QUrl::fromLocalFile(QDir(path).absoluteFilePath(pictures.at(QRandomGenerator::global()->bounded(int(pictures.size())))));
}

// What a command prints, or nothing when it is not there or hangs.
QString run(const QString &program, const QStringList &args)
{
    QProcess process;
    process.start(program, args);
    if (!process.waitForFinished(1000) || process.exitStatus() != QProcess::NormalExit || process.exitCode() != 0) {
        process.kill();
        process.waitForFinished(100);
        return {};
    }
    return QString::fromLocal8Bit(process.readAllStandardOutput());
}

// The pictures the running wallpaper programs of this user show.
QHash<QString, QString> desktop()
{
    QHash<QString, QString> found;
    const uint me = getuid();
    const QStringList pids = QDir(QStringLiteral("/proc")).entryList({QStringLiteral("[0-9]*")}, QDir::Dirs);
    for (const QString &pid : pids) {
        const QString dir = QStringLiteral("/proc/") + pid;
        if (QFileInfo(dir).ownerId() != me) {
            continue;
        }
        QFile comm(dir + QStringLiteral("/comm"));
        if (!comm.open(QIODevice::ReadOnly)) {
            continue;
        }
        const QByteArray name = comm.readAll().trimmed();
        if (name == "swaybg") {
            QFile cmdline(dir + QStringLiteral("/cmdline"));
            if (cmdline.open(QIODevice::ReadOnly)) {
                QStringList args;
                for (const QByteArray &arg : cmdline.readAll().split('\0')) {
                    args << QString::fromLocal8Bit(arg);
                }
                // Relative to where it was started.
                const QDir cwd(QFileInfo(dir + QStringLiteral("/cwd")).symLinkTarget());
                QHash<QString, QString> shown = Wallpaper::fromSwaybg(args.mid(1));
                for (QString &path : shown) {
                    path = cwd.absoluteFilePath(path);
                }
                found.insert(shown);
            }
        } else if (name == "awww-daemon" || name == "swww-daemon") {
            found.insert(Wallpaper::fromAwww(run(QString::fromLatin1(name.left(4)), {QStringLiteral("query")})));
        } else if (name == "hyprpaper") {
            found.insert(Wallpaper::fromList(run(QStringLiteral("hyprctl"), {QStringLiteral("hyprpaper"), QStringLiteral("listactive")})));
        } else if (name == "wpaperd") {
            found.insert(Wallpaper::fromList(run(QStringLiteral("wpaperctl"), {QStringLiteral("all-wallpapers")})));
        }
    }
    return found;
}
} // namespace

QHash<QString, QString> Wallpaper::fromSwaybg(const QStringList &args)
{
    QHash<QString, QString> found;
    QString output;
    for (int i = 0; i < args.size(); ++i) {
        const QString &arg = args.at(i);
        const bool hasNext = i + 1 < args.size();
        if ((arg == u"-o" || arg == u"--output") && hasNext) {
            output = args.at(++i);
        } else if (arg.startsWith(u"--output=")) {
            output = arg.mid(9);
        } else if ((arg == u"-i" || arg == u"--image") && hasNext) {
            found.insert(output == u"*" ? QString() : output, args.at(++i));
        } else if (arg.startsWith(u"--image=")) {
            found.insert(output == u"*" ? QString() : output, arg.mid(8));
        }
    }
    return found;
}

QHash<QString, QString> Wallpaper::fromAwww(const QString &text)
{
    // "eDP-1: 1920x1080, scale: 1, currently displaying: image: /a/b.jpg",
    // with a namespace in front on awww. A plain colour is no picture.
    static const QRegularExpression line(QStringLiteral("([^\\s:]+): \\d+x\\d+,.*currently displaying: image: (.+)$"), QRegularExpression::MultilineOption);
    QHash<QString, QString> found;
    auto it = line.globalMatch(text);
    while (it.hasNext()) {
        const auto match = it.next();
        found.insert(match.captured(1), match.captured(2).trimmed());
    }
    return found;
}

QHash<QString, QString> Wallpaper::fromList(const QString &text)
{
    // "eDP-1: /a/b.jpg", on older hyprpaper "eDP-1 = /a/b.jpg".
    static const QRegularExpression line(QStringLiteral("^([^\\s:=]+)(?:: | = )(/.+)$"), QRegularExpression::MultilineOption);
    QHash<QString, QString> found;
    auto it = line.globalMatch(text);
    while (it.hasNext()) {
        const auto match = it.next();
        found.insert(match.captured(1), match.captured(2).trimmed());
    }
    return found;
}

QHash<QString, QUrl> Wallpaper::forLock(const QString &setting)
{
    QHash<QString, QUrl> pictures;
    if (setting.compare(u"none", Qt::CaseInsensitive) == 0) {
        return pictures;
    }
    if (!setting.isEmpty()) {
        const QUrl url = picture(setting);
        if (!url.isEmpty()) {
            pictures.insert(QString(), url);
        }
        return pictures;
    }
    const QHash<QString, QString> shown = desktop();
    for (auto it = shown.cbegin(); it != shown.cend(); ++it) {
        const QUrl url = picture(it.value());
        if (!url.isEmpty()) {
            pictures.insert(it.key(), url);
        }
    }
    return pictures;
}

QUrl Wallpaper::pick(const QHash<QString, QUrl> &pictures, const QString &output)
{
    if (pictures.isEmpty()) {
        return {};
    }
    if (pictures.contains(output)) {
        return pictures.value(output);
    }
    if (pictures.contains(QString())) {
        return pictures.value(QString());
    }
    // Named differently than here: some picture of the desktop is still
    // better than none.
    QStringList outputs = pictures.keys();
    outputs.sort();
    return pictures.value(outputs.first());
}
