// SPDX-License-Identifier: GPL-3.0-or-later

#include "lockpreview.h"

#include <KLocalizedString>

#include <QDateTime>
#include <QDir>
#include <QElapsedTimer>
#include <QFile>
#include <QFileInfo>
#include <QHash>
#include <QProcess>
#include <QRegularExpression>
#include <QPointF>
#include <QStandardPaths>
#include <QTime>
#include <QVariantMap>

#include <algorithm>
#include <pwd.h>
#include <unistd.h>

namespace
{
QString readFile(const QString &path)
{
    QFile f(path);
    return f.open(QIODevice::ReadOnly | QIODevice::Text) ? QString::fromUtf8(f.readAll()) : QString();
}

QString expandHome(const QString &path)
{
    return path == u"~" ? QDir::homePath() : path.startsWith(u"~/") ? QDir::homePath() + path.mid(1) : path;
}

QString configHome()
{
    return QStandardPaths::writableLocation(QStandardPaths::GenericConfigLocation);
}

// The first file of name under the user's and then the system's config
// directories, as hyprlock and swaylock look for theirs.
QString findConfig(const QString &name)
{
    QStringList dirs{configHome()};
    dirs += QStandardPaths::standardLocations(QStandardPaths::GenericConfigLocation);
    for (const QString &dir : std::as_const(dirs)) {
        if (QFileInfo::exists(dir + u'/' + name)) {
            return dir + u'/' + name;
        }
    }
    return {};
}

bool truthy(const QString &value)
{
    return value == u"1" || value.compare(u"true", Qt::CaseInsensitive) == 0 || value.compare(u"yes", Qt::CaseInsensitive) == 0
        || value.compare(u"on", Qt::CaseInsensitive) == 0;
}

// ---------------------------------------------------------------------------
// hyprlang, as far as a lock screen's config needs it

struct Section {
    QString name;
    QHash<QString, QString> values;
};

// Sections, $variables, source = (with ~ and wildcards, relative to the
// file), and # comments, where ## stands for a #.
class Hyprlang
{
public:
    void parse(const QString &path, int depth = 0);

    QList<Section> sections;

private:
    QString expand(const QString &value) const;

    QHash<QString, QString> m_vars;
    QList<qsizetype> m_open;
};

QString stripComment(const QString &line)
{
    QString out;
    for (qsizetype i = 0; i < line.size(); ++i) {
        if (line.at(i) == u'#') {
            if (i + 1 < line.size() && line.at(i + 1) == u'#') {
                out += u'#';
                ++i;
                continue;
            }
            break;
        }
        out += line.at(i);
    }
    return out.trimmed();
}

void Hyprlang::parse(const QString &path, int depth)
{
    if (depth > 8) {
        return;
    }
    const QString dir = QFileInfo(path).absolutePath();
    const QStringList lines = readFile(path).split(u'\n');
    for (const QString &raw : lines) {
        const QString line = stripComment(raw);
        if (line.isEmpty()) {
            continue;
        }
        if (line == u"}") {
            if (!m_open.isEmpty()) {
                m_open.removeLast();
            }
            continue;
        }
        if (line.endsWith(u'{')) {
            sections.append({line.chopped(1).trimmed(), {}});
            m_open.append(sections.size() - 1);
            continue;
        }
        const qsizetype eq = line.indexOf(u'=');
        if (eq < 0) {
            continue;
        }
        const QString key = line.left(eq).trimmed();
        const QString value = expand(line.mid(eq + 1).trimmed());
        if (key.startsWith(u'$')) {
            m_vars.insert(key.mid(1), value);
        } else if (key == u"source" && m_open.isEmpty()) {
            const QFileInfo pattern(QDir(dir).absoluteFilePath(expandHome(value)));
            const QStringList matches = QDir(pattern.absolutePath()).entryList({pattern.fileName()}, QDir::Files, QDir::Name);
            for (const QString &match : matches) {
                parse(pattern.absolutePath() + u'/' + match, depth + 1);
            }
        } else if (!m_open.isEmpty()) {
            sections[m_open.last()].values.insert(key, value);
        }
    }
}

QString Hyprlang::expand(const QString &value) const
{
    if (!value.contains(u'$') || m_vars.isEmpty()) {
        return value;
    }
    // Longest first, so $font does not eat the start of $fontsize.
    QStringList names = m_vars.keys();
    std::sort(names.begin(), names.end(), [](const QString &a, const QString &b) {
        return a.size() > b.size();
    });
    QString out = value;
    for (const QString &name : std::as_const(names)) {
        out.replace(u'$' + name, m_vars.value(name));
    }
    return out;
}

// ---------------------------------------------------------------------------
// Values

// "rgba(a, b) rgba(c) 45deg": the colours and the angle.
QVariantMap gradient(const QString &value)
{
    QVariantList colors;
    double angle = 0;
    QString token;
    int depth = 0;
    const auto flush = [&] {
        const QString t = token.trimmed();
        token.clear();
        if (t.endsWith(u"deg")) {
            angle = t.chopped(3).toDouble();
        } else if (const QColor c = LockPreview::color(t); c.isValid()) {
            colors.append(c);
        }
    };
    for (const QChar ch : value) {
        if (ch == u'(') {
            ++depth;
        } else if (ch == u')') {
            --depth;
        }
        if (ch.isSpace() && depth == 0) {
            flush();
        } else {
            token += ch;
        }
    }
    flush();
    return {{QStringLiteral("colors"), colors}, {QStringLiteral("angle"), angle}};
}

// "x, y", each in pixels or percent of the screen.
QPointF layout(const QString &value, const QSizeF &screen)
{
    const QStringList parts = value.split(u',');
    const auto one = [](const QString &part, qreal whole) {
        const QString s = part.trimmed();
        return s.endsWith(u'%') ? s.chopped(1).toDouble() / 100.0 * whole : s.toDouble();
    };
    return {one(parts.value(0), screen.width()), one(parts.value(1), screen.height())};
}

// A Pango font description ("Noto Sans Light 12") as family, weight and
// slant.
void font(const QString &description, QVariantMap &into)
{
    static const QHash<QString, int> weights{
        {QStringLiteral("thin"), 100},       {QStringLiteral("ultra-light"), 200}, {QStringLiteral("extra-light"), 200},
        {QStringLiteral("ultralight"), 200}, {QStringLiteral("extralight"), 200},  {QStringLiteral("light"), 300},
        {QStringLiteral("semi-light"), 350}, {QStringLiteral("book"), 380},        {QStringLiteral("regular"), 400},
        {QStringLiteral("normal"), 400},     {QStringLiteral("medium"), 500},      {QStringLiteral("semi-bold"), 600},
        {QStringLiteral("semibold"), 600},   {QStringLiteral("demi-bold"), 600},   {QStringLiteral("demibold"), 600},
        {QStringLiteral("bold"), 700},       {QStringLiteral("ultra-bold"), 800},  {QStringLiteral("extra-bold"), 800},
        {QStringLiteral("extrabold"), 800},  {QStringLiteral("heavy"), 900},       {QStringLiteral("black"), 900},
    };
    QStringList words = description.section(u',', 0, 0).split(u' ', Qt::SkipEmptyParts);
    int weight = 400;
    bool italic = false;
    while (words.size() > 1) {
        const QString word = words.last().toLower();
        bool number = false;
        word.toDouble(&number);
        if (number) {
            words.removeLast();
        } else if (word == u"italic" || word == u"oblique") {
            italic = true;
            words.removeLast();
        } else if (weights.contains(word)) {
            weight = weights.value(word);
            words.removeLast();
        } else {
            break;
        }
    }
    into.insert(QStringLiteral("family"), words.isEmpty() ? QStringLiteral("Sans") : words.join(u' '));
    into.insert(QStringLiteral("weight"), weight);
    into.insert(QStringLiteral("italic"), italic);
}

// Pango markup as Qt's StyledText: span becomes font, b and i, the rest of
// Pango's tags go and their text stays.
QString styled(const QString &markup)
{
    static const QRegularExpression tag(QStringLiteral("<(/?)([a-zA-Z]+)([^>]*)>"));
    static const QRegularExpression attribute(QStringLiteral("([a-z_]+)\\s*=\\s*[\"']([^\"']*)[\"']"));
    QString out;
    QStringList closers;
    qsizetype last = 0;
    auto it = tag.globalMatch(markup);
    while (it.hasNext()) {
        const auto m = it.next();
        out += markup.mid(last, m.capturedStart() - last);
        last = m.capturedEnd();
        const bool closing = !m.captured(1).isEmpty();
        const QString name = m.captured(2).toLower();
        if (name == u"span") {
            if (closing) {
                out += closers.isEmpty() ? QString() : closers.takeLast();
                continue;
            }
            QString open;
            QString close;
            QString fontAttrs;
            auto attrs = attribute.globalMatch(m.captured(3));
            while (attrs.hasNext()) {
                const auto a = attrs.next();
                const QString key = a.captured(1);
                QString value = a.captured(2);
                if (key == u"foreground" || key == u"fgcolor" || key == u"color") {
                    // Pango's #RRGGBBAA is Qt's #AARRGGBB.
                    if (value.startsWith(u'#') && value.size() == 9) {
                        value = u'#' + value.mid(7, 2) + value.mid(1, 6);
                    }
                    fontAttrs += QStringLiteral(" color=\"%1\"").arg(value);
                } else if (key == u"font_family" || key == u"face") {
                    fontAttrs += QStringLiteral(" face=\"%1\"").arg(value);
                } else if ((key == u"weight" || key == u"font_weight") && (value.contains(u"bold") || value.contains(u"heavy") || value.toInt() >= 600)) {
                    open += QStringLiteral("<b>");
                    close.prepend(QStringLiteral("</b>"));
                } else if ((key == u"style" || key == u"font_style") && value == u"italic") {
                    open += QStringLiteral("<i>");
                    close.prepend(QStringLiteral("</i>"));
                }
            }
            if (!fontAttrs.isEmpty()) {
                open.prepend(QStringLiteral("<font%1>").arg(fontAttrs));
                close += QStringLiteral("</font>");
            }
            out += open;
            closers.append(close);
        } else if (name == u"b" || name == u"i" || name == u"u") {
            out += m.captured(0);
        }
    }
    out += markup.mid(last);
    out.replace(u'\n', QStringLiteral("<br>"));
    return out;
}

// What a command prints, or nothing when it takes too long.
QString run(const QString &command, QElapsedTimer &budget)
{
    // A config full of slow commands (weather, music) must not keep the
    // window from opening.
    if (budget.elapsed() > 2000) {
        return {};
    }
    QProcess process;
    process.start(QStringLiteral("/bin/sh"), {QStringLiteral("-c"), command});
    if (!process.waitForFinished(700)) {
        process.kill();
        process.waitForFinished(100);
        return {};
    }
    return QString::fromUtf8(process.readAllStandardOutput());
}

// A label's text as hyprlock would show it right after locking.
QString labelText(QString text, bool trim, QElapsedTimer &budget)
{
    const passwd *pw = getpwuid(getuid());
    text.replace(QStringLiteral("$DESC"), pw ? QString::fromLocal8Bit(pw->pw_gecos) : QString());
    text.replace(QStringLiteral("$USER"), pw ? QString::fromLocal8Bit(pw->pw_name) : QString());
    text.replace(QStringLiteral("<br/>"), QStringLiteral("\n"));
    const QTime now = QTime::currentTime();
    text.replace(QStringLiteral("$TIME12"), now.toString(QStringLiteral("hh:mm AP")));
    text.replace(QStringLiteral("$TIME"), now.toString(QStringLiteral("HH:mm")));
    static const QRegularExpression layoutVar(QStringLiteral("\\$LAYOUT(\\[([^\\]]*)\\])?"));
    auto layouts = layoutVar.globalMatch(text);
    while (layouts.hasNext()) {
        const auto m = layouts.next();
        const QString first = m.captured(2).section(u',', 0, 0).trimmed();
        text.replace(m.captured(0), first.isEmpty() ? QStringLiteral("en") : first);
    }
    // What only a lock screen that runs knows: nothing has failed yet.
    static const QRegularExpression runtime(QStringLiteral("\\$(ATTEMPTS(\\[[^\\]]*\\])?|PAMFAIL|FPRINTFAIL|FPRINTPROMPT|FAIL)"));
    text.remove(runtime);
    text.replace(QStringLiteral("$PAMPROMPT"), QStringLiteral("Password:"));

    if (text.startsWith(u"cmd[") && text.contains(u']')) {
        const QString command = text.mid(text.indexOf(u']') + 1);
        // face-unlock's own line, as it reads during a scan.
        text = command.contains(u"face-unlock/lock-text") ? i18n("Looking for your face…").toHtmlEscaped() : run(command, budget);
    }
    return trim ? text.trimmed() : text;
}

void place(const Section &s, const QString &position, const QSizeF &screen, QVariantMap &into)
{
    const QPointF pos = layout(s.values.value(QStringLiteral("position"), position), screen);
    into.insert(QStringLiteral("x"), pos.x());
    into.insert(QStringLiteral("y"), pos.y());
    into.insert(QStringLiteral("halign"), s.values.value(QStringLiteral("halign"), QStringLiteral("center")));
    into.insert(QStringLiteral("valign"), s.values.value(QStringLiteral("valign"), QStringLiteral("center")));
    into.insert(QStringLiteral("rotate"), s.values.value(QStringLiteral("rotate"), QStringLiteral("0")).toDouble());
}

QColor colorOr(const QString &value, const QColor &fallback)
{
    const QColor c = LockPreview::color(value);
    return c.isValid() ? c : fallback;
}

// swaylock's colours: RRGGBB or RRGGBBAA, without a #.
QColor hexColor(const QString &value, const QColor &fallback)
{
    QString v = value.trimmed();
    if (v.startsWith(u'#')) {
        v = v.mid(1);
    }
    bool ok = false;
    const uint n = v.toUInt(&ok, 16);
    if (!ok || (v.size() != 6 && v.size() != 8)) {
        return fallback;
    }
    return v.size() == 6 ? QColor((n >> 16) & 0xff, (n >> 8) & 0xff, n & 0xff) : QColor((n >> 24) & 0xff, (n >> 16) & 0xff, (n >> 8) & 0xff, n & 0xff);
}

QVariantMap wallpaperOnly(const QUrl &wallpaper)
{
    return {{QStringLiteral("type"), QStringLiteral("background")},
            {QStringLiteral("color"), QColor(0x11, 0x11, 0x11)},
            {QStringLiteral("source"), wallpaper},
            {QStringLiteral("fill"), QStringLiteral("fill")},
            {QStringLiteral("blur"), 0.0},
            {QStringLiteral("dim"), 0.0}};
}
} // namespace

QColor LockPreview::color(const QString &value)
{
    const QString v = value.trimmed();
    static const QRegularExpression call(QStringLiteral("^(rgba?)\\((.*)\\)$"));
    const auto m = call.match(v);
    if (m.hasMatch()) {
        const QString inner = m.captured(2).trimmed();
        const QStringList parts = inner.split(u',');
        if (parts.size() >= 3) {
            QColor c(parts.at(0).trimmed().toInt(), parts.at(1).trimmed().toInt(), parts.at(2).trimmed().toInt());
            if (parts.size() >= 4) {
                c.setAlphaF(std::clamp(parts.at(3).trimmed().toDouble(), 0.0, 1.0));
            }
            return c;
        }
        return hexColor(inner, {});
    }
    bool ok = false;
    const qulonglong n = v.startsWith(u"0x", Qt::CaseInsensitive) ? v.mid(2).toULongLong(&ok, 16) : v.toULongLong(&ok, 10);
    if (!ok) {
        return {};
    }
    return QColor(int((n >> 16) & 0xff), int((n >> 8) & 0xff), int(n & 0xff), int((n >> 24) & 0xff));
}

QString LockPreview::locker()
{
    static const QStringList names{QStringLiteral("hyprlock"), QStringLiteral("swaylock"), QStringLiteral("gtklock"), QStringLiteral("waylock")};
    // Where a lock screen is started from: the idle daemons first, then the
    // keys. The first one named there, outside comments.
    static const QStringList places{QStringLiteral("hypr/hypridle.conf"), QStringLiteral("swayidle/config"), QStringLiteral("niri/config.kdl"),
                                    QStringLiteral("hypr/hyprland.lua"), QStringLiteral("hypr/hyprland.conf")};
    for (const QString &place : places) {
        for (const QString &line : readFile(configHome() + u'/' + place).split(u'\n')) {
            const QString code = line.trimmed();
            if (code.startsWith(u'#') || code.startsWith(u"//") || code.startsWith(u"--")) {
                continue;
            }
            qsizetype first = -1;
            QString found;
            for (const QString &name : names) {
                const qsizetype at = code.indexOf(name);
                if (at >= 0 && (first < 0 || at < first)) {
                    first = at;
                    found = name;
                }
            }
            if (!found.isEmpty()) {
                return found;
            }
        }
    }
    // Started some other way (a script of its own): one that is set up.
    if (!QStandardPaths::findExecutable(QStringLiteral("hyprlock")).isEmpty() && !findConfig(QStringLiteral("hypr/hyprlock.conf")).isEmpty()) {
        return QStringLiteral("hyprlock");
    }
    for (const QString &name : names) {
        if (!QStandardPaths::findExecutable(name).isEmpty()) {
            return name;
        }
    }
    return {};
}

QVariantList LockPreview::hyprlock(const QString &configPath, const QSizeF &screen, const QString &output, const QUrl &wallpaper)
{
    Hyprlang config;
    config.parse(configPath);

    bool trim = true;
    for (const Section &s : std::as_const(config.sections)) {
        if (s.name == u"general" && s.values.contains(QStringLiteral("text_trim"))) {
            trim = truthy(s.values.value(QStringLiteral("text_trim")));
        }
    }

    QElapsedTimer budget;
    budget.start();
    QList<QPair<int, QVariantMap>> placed;
    bool ourLine = false;
    bool background = false;
    for (const Section &s : std::as_const(config.sections)) {
        const auto value = [&s](const char *key, const char *fallback) {
            return s.values.value(QString::fromLatin1(key), QString::fromLatin1(fallback));
        };
        const QString monitor = value("monitor", "");
        if (!monitor.isEmpty() && monitor != output && !monitor.startsWith(u"desc:")) {
            continue;
        }
        QVariantMap w;
        int z = value("zindex", "0").toInt();
        if (s.name == u"background") {
            background = true;
            z = value("zindex", "-1").toInt();
            const QString path = value("path", "");
            const int passes = value("blur_passes", "0").toInt();
            w = wallpaperOnly(path == u"screenshot" ? wallpaper : path.isEmpty() ? QUrl() : QUrl::fromLocalFile(expandHome(path)));
            w.insert(QStringLiteral("color"), colorOr(value("color", ""), QColor(0x11, 0x11, 0x11)));
            w.insert(QStringLiteral("blur"), passes > 0 ? std::min(1.0, passes * value("blur_size", "8").toDouble() / 24.0) : 0.0);
            // hyprlock's blur also darkens, by its brightness.
            w.insert(QStringLiteral("dim"), passes > 0 ? std::clamp(1.0 - value("brightness", "0.8172").toDouble(), 0.0, 1.0) : 0.0);
        } else if (s.name == u"label") {
            const QString text = value("text", "Sample Text");
            ourLine = ourLine || text.contains(u"face-unlock/lock-text");
            w.insert(QStringLiteral("type"), QStringLiteral("label"));
            w.insert(QStringLiteral("text"), styled(labelText(text, trim, budget)));
            font(value("font_family", "Sans"), w);
            w.insert(QStringLiteral("size"), value("font_size", "16").toDouble());
            w.insert(QStringLiteral("color"), colorOr(value("color", ""), Qt::white));
            w.insert(QStringLiteral("align"), value("text_align", "center"));
            w.insert(QStringLiteral("shadow"), value("shadow_passes", "0").toInt() > 0);
            w.insert(QStringLiteral("shadowSize"), value("shadow_size", "3").toDouble());
            w.insert(QStringLiteral("shadowColor"), colorOr(value("shadow_color", ""), Qt::black));
            place(s, QStringLiteral("0,0"), screen, w);
        } else if (s.name == u"input-field") {
            const QPointF size = layout(value("size", "400,90"), screen);
            w.insert(QStringLiteral("type"), QStringLiteral("input"));
            w.insert(QStringLiteral("width"), size.x());
            w.insert(QStringLiteral("height"), size.y());
            w.insert(QStringLiteral("thickness"), value("outline_thickness", "4").toDouble());
            w.insert(QStringLiteral("outer"), gradient(value("outer_color", "0xFF111111")));
            w.insert(QStringLiteral("inner"), colorOr(value("inner_color", ""), QColor(0xdd, 0xdd, 0xdd)));
            w.insert(QStringLiteral("fontColor"), colorOr(value("font_color", ""), Qt::black));
            font(value("font_family", "Sans"), w);
            w.insert(QStringLiteral("placeholder"), styled(labelText(value("placeholder_text", "<i>Input Password</i>"), trim, budget)));
            w.insert(QStringLiteral("rounding"), value("rounding", "-1").toDouble());
            place(s, QStringLiteral("0,0"), screen, w);
        } else if (s.name == u"shape") {
            const QPointF size = layout(value("size", "100,100"), screen);
            w.insert(QStringLiteral("type"), QStringLiteral("shape"));
            w.insert(QStringLiteral("width"), size.x());
            w.insert(QStringLiteral("height"), size.y());
            w.insert(QStringLiteral("color"), colorOr(value("color", ""), QColor(0x11, 0x11, 0x11)));
            w.insert(QStringLiteral("rounding"), value("rounding", "0").toDouble());
            w.insert(QStringLiteral("borderSize"), value("border_size", "0").toDouble());
            w.insert(QStringLiteral("border"), gradient(value("border_color", "0xFF00CFE6")));
            place(s, QStringLiteral("0,0"), screen, w);
        } else if (s.name == u"image") {
            const QString path = value("path", "");
            if (path.isEmpty()) {
                continue;
            }
            w.insert(QStringLiteral("type"), QStringLiteral("image"));
            w.insert(QStringLiteral("source"), QUrl::fromLocalFile(expandHome(path)));
            w.insert(QStringLiteral("size"), value("size", "150").toDouble());
            w.insert(QStringLiteral("rounding"), value("rounding", "-1").toDouble());
            w.insert(QStringLiteral("borderSize"), value("border_size", "4").toDouble());
            w.insert(QStringLiteral("border"), gradient(value("border_color", "0xFFDDDDDD")));
            place(s, QStringLiteral("0,0"), screen, w);
        } else {
            continue;
        }
        placed.append({z, w});
    }

    if (!background) {
        QVariantMap w = wallpaperOnly({});
        placed.append({-1, w});
    }
    // face-unlock's line, as the menu puts it in (fu_hyprlock_text_on),
    // for those who have not picked it yet.
    if (!ourLine) {
        QVariantMap w{{QStringLiteral("type"), QStringLiteral("label")},
                      {QStringLiteral("text"), i18n("Looking for your face…").toHtmlEscaped()},
                      {QStringLiteral("size"), 20.0},
                      {QStringLiteral("color"), QColor(255, 255, 255, 242)},
                      {QStringLiteral("align"), QStringLiteral("center")},
                      {QStringLiteral("shadow"), true},
                      {QStringLiteral("shadowSize"), 3.0},
                      {QStringLiteral("shadowColor"), QColor(Qt::black)},
                      {QStringLiteral("x"), 0.0},
                      {QStringLiteral("y"), -48.0},
                      {QStringLiteral("halign"), QStringLiteral("center")},
                      {QStringLiteral("valign"), QStringLiteral("top")},
                      {QStringLiteral("rotate"), 0.0}};
        font(QStringLiteral("Sans"), w);
        placed.append({10, w});
    }

    std::stable_sort(placed.begin(), placed.end(), [](const auto &a, const auto &b) {
        return a.first < b.first;
    });
    QVariantList widgets;
    for (const auto &p : std::as_const(placed)) {
        widgets.append(p.second);
    }
    return widgets;
}

QVariantList LockPreview::swaylock(const QString &configPath, const QUrl &wallpaper)
{
    // One option per line, as on the command line without the dashes.
    QHash<QString, QString> o;
    for (const QString &raw : readFile(configPath).split(u'\n')) {
        const QString line = raw.trimmed();
        if (line.isEmpty() || line.startsWith(u'#')) {
            continue;
        }
        o.insert(line.section(u'=', 0, 0).trimmed(), line.contains(u'=') ? line.section(u'=', 1).trimmed() : QString());
    }

    // An image may name its output first: [[<output>]:]<path>.
    QString image = o.value(QStringLiteral("image"));
    if (image.contains(u':') && !image.startsWith(u'/') && !image.startsWith(u'~')) {
        image = image.section(u':', 1);
    }
    QVariantMap bg = wallpaperOnly(o.contains(QStringLiteral("screenshots")) ? wallpaper
                                       : image.isEmpty()                    ? QUrl()
                                                                            : QUrl::fromLocalFile(expandHome(image)));
    bg.insert(QStringLiteral("color"), hexColor(o.value(QStringLiteral("color")), Qt::white));
    bg.insert(QStringLiteral("fill"), o.value(QStringLiteral("scaling"), QStringLiteral("fill")));
    // swaylock-effects: effect-blur=<radius>x<times>.
    const QString blur = o.value(QStringLiteral("effect-blur"));
    if (!blur.isEmpty()) {
        bg.insert(QStringLiteral("blur"), std::min(1.0, blur.section(u'x', 0, 0).toDouble() * blur.section(u'x', 1, 1).toDouble() / 24.0));
    }
    QVariantList widgets{bg};

    // The ring shows only while typing, unless it is asked to stay.
    const bool clock = o.contains(QStringLiteral("clock"));
    if (o.contains(QStringLiteral("indicator")) || o.contains(QStringLiteral("indicator-idle-visible")) || clock) {
        const QDateTime now = QDateTime::currentDateTime();
        widgets.append(QVariantMap{
            {QStringLiteral("type"), QStringLiteral("ring")},
            {QStringLiteral("radius"), o.value(QStringLiteral("indicator-radius"), QStringLiteral("50")).toDouble()},
            {QStringLiteral("thickness"), o.value(QStringLiteral("indicator-thickness"), QStringLiteral("10")).toDouble()},
            {QStringLiteral("inside"), hexColor(o.value(QStringLiteral("inside-color")), QColor(0, 0, 0, 0xc0))},
            {QStringLiteral("ring"), hexColor(o.value(QStringLiteral("ring-color")), QColor(0x33, 0x7d, 0x00))},
            {QStringLiteral("textColor"), hexColor(o.value(QStringLiteral("text-color")), QColor(0xe5, 0xa4, 0x45))},
            {QStringLiteral("text"), clock ? now.toString(QStringLiteral("HH:mm")) + u'\n' + now.toString(QStringLiteral("yyyy-MM-dd")) : QString()},
        });
    }
    return widgets;
}

QVariantList LockPreview::widgets(const QString &locker, const QSizeF &screen, const QString &output, const QUrl &wallpaper)
{
    if (locker == u"hyprlock") {
        const QString path = findConfig(QStringLiteral("hypr/hyprlock.conf"));
        if (!path.isEmpty()) {
            return hyprlock(path, screen, output, wallpaper);
        }
    } else if (locker == u"swaylock") {
        QString path = findConfig(QStringLiteral("swaylock/config"));
        if (path.isEmpty() && QFileInfo::exists(QDir::homePath() + QStringLiteral("/.swaylock/config"))) {
            path = QDir::homePath() + QStringLiteral("/.swaylock/config");
        }
        return swaylock(path, wallpaper);
    }
    // Something this does not know how to draw: the desktop's picture.
    return {wallpaperOnly(wallpaper)};
}
