// SPDX-License-Identifier: GPL-3.0-or-later

#include "system.h"

#include <QDBusConnection>
#include <QDBusInterface>
#include <QDir>
#include <QFile>

#include <pwd.h>
#include <unistd.h>

#include <vector>

namespace System
{
std::optional<uid_t> uidOf(const QString &user)
{
    if (user.isEmpty()) {
        return std::nullopt;
    }
    std::vector<char> buf(16384);
    passwd pw{};
    passwd *result = nullptr;
    if (getpwnam_r(user.toLocal8Bit().constData(), &pw, buf.data(), buf.size(), &result) != 0 || !result) {
        return std::nullopt;
    }
    return result->pw_uid;
}

QString nameOf(uid_t uid)
{
    std::vector<char> buf(16384);
    passwd pw{};
    passwd *result = nullptr;
    if (getpwuid_r(uid, &pw, buf.data(), buf.size(), &result) != 0 || !result) {
        return QString::number(uid);
    }
    return QString::fromLocal8Bit(result->pw_name);
}

bool lidClosed()
{
    QDBusInterface logind(QStringLiteral("org.freedesktop.login1"),
                          QStringLiteral("/org/freedesktop/login1"),
                          QStringLiteral("org.freedesktop.login1.Manager"),
                          QDBusConnection::systemBus());
    if (logind.isValid()) {
        const QVariant v = logind.property("LidClosed");
        if (v.isValid()) {
            return v.toBool();
        }
    }

    const QDir lids(QStringLiteral("/proc/acpi/button/lid"));
    for (const QString &lid : lids.entryList(QDir::Dirs | QDir::NoDotAndDotDot)) {
        QFile state(lids.filePath(lid + QStringLiteral("/state")));
        if (state.open(QIODevice::ReadOnly) && state.readAll().contains("closed")) {
            return true;
        }
    }
    return false;
}

quint64 processStartTime(pid_t pid)
{
    QFile stat(QStringLiteral("/proc/%1/stat").arg(pid));
    if (!stat.open(QIODevice::ReadOnly)) {
        return 0;
    }
    const QByteArray line = stat.readAll();
    // The second field is the command name in brackets and can contain
    // spaces and brackets of its own. Everything after the last ')' is
    // plain numbers; the start time is the 20th of them.
    const qsizetype close = line.lastIndexOf(')');
    if (close < 0) {
        return 0;
    }
    const QList<QByteArray> fields = line.mid(close + 2).split(' ');
    return fields.size() > 19 ? fields.at(19).toULongLong() : 0;
}
} // namespace System
