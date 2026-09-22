// SPDX-License-Identifier: GPL-3.0-or-later

#include "keyvalue.h"

#include <QFile>

KeyValueFile KeyValueFile::load(const QString &path)
{
    KeyValueFile kv;
    QFile file(path);
    if (!file.open(QIODevice::ReadOnly | QIODevice::Text)) {
        return kv;
    }

    while (!file.atEnd()) {
        QString line = QString::fromUtf8(file.readLine()).trimmed();
        if (line.isEmpty() || line.startsWith(QLatin1Char('#'))) {
            continue;
        }
        const qsizetype eq = line.indexOf(QLatin1Char('='));
        if (eq <= 0) {
            continue;
        }
        const QString key = line.left(eq).trimmed();
        QString value = line.mid(eq + 1);

        // A comment can follow a value, the same as in the shell reader.
        const qsizetype hash = value.indexOf(QLatin1Char('#'));
        if (hash >= 0) {
            value.truncate(hash);
        }
        value = value.trimmed();
        if (value.size() >= 2 && value.startsWith(QLatin1Char('"')) && value.endsWith(QLatin1Char('"'))) {
            value = value.mid(1, value.size() - 2);
        }

        // The last occurrence wins, which is also what the shell reader does.
        kv.m_values.insert(key, value);
    }
    return kv;
}

QString KeyValueFile::value(const QString &key, const QString &fallback) const
{
    const auto it = m_values.constFind(key);
    if (it == m_values.cend() || it->isEmpty()) {
        return fallback;
    }
    return *it;
}

bool KeyValueFile::contains(const QString &key) const
{
    return m_values.contains(key);
}

bool KeyValueFile::parseBool(const QString &value, bool fallback)
{
    const QString v = value.trimmed().toLower();
    if (v == u"yes" || v == u"y" || v == u"true" || v == u"1" || v == u"on" || v == u"enabled") {
        return true;
    }
    if (v == u"no" || v == u"n" || v == u"false" || v == u"0" || v == u"off" || v == u"disabled") {
        return false;
    }
    return fallback;
}

bool KeyValueFile::boolean(const QString &key, bool fallback) const
{
    return parseBool(value(key), fallback);
}

int KeyValueFile::integer(const QString &key, int fallback, int min, int max) const
{
    bool ok = false;
    const int v = value(key).toInt(&ok);
    if (!ok) {
        return fallback;
    }
    return std::clamp(v, min, max);
}
