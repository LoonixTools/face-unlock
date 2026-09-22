// SPDX-License-Identifier: GPL-3.0-or-later
//
// The settings files, read the same way the shell code reads them: one
// Key=Value per line, '#' starts a comment, quotes around a value are dropped.
// Both halves of the program write and read these files, so they have to
// agree on every detail of the format.

#pragma once

#include <QHash>
#include <QString>

class KeyValueFile
{
public:
    static KeyValueFile load(const QString &path);

    QString value(const QString &key, const QString &fallback = {}) const;
    bool boolean(const QString &key, bool fallback) const;
    int integer(const QString &key, int fallback, int min, int max) const;
    bool contains(const QString &key) const;

    static bool parseBool(const QString &value, bool fallback);

private:
    QHash<QString, QString> m_values;
};
