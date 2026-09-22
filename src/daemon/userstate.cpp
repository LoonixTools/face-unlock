// SPDX-License-Identifier: GPL-3.0-or-later

#include "userstate.h"

#include <QDir>
#include <QFile>
#include <QJsonDocument>
#include <QJsonObject>
#include <QSaveFile>

namespace
{
QString fileFor(const QString &stateDir, uid_t uid)
{
    return QDir(stateDir).filePath(QStringLiteral("users/%1.state").arg(uid));
}
} // namespace

UserState UserState::load(const QString &stateDir, uid_t uid)
{
    UserState s;
    QFile file(fileFor(stateDir, uid));
    if (!file.open(QIODevice::ReadOnly)) {
        return s;
    }
    const QJsonObject o = QJsonDocument::fromJson(file.readAll()).object();
    s.failures = o.value(u"failures").toInt();
    s.lockedUntil = qint64(o.value(u"lockedUntil").toDouble());
    s.lastUnlock = qint64(o.value(u"lastUnlock").toDouble());
    s.lastPurpose = o.value(u"lastPurpose").toString();
    return s;
}

bool UserState::save(const QString &stateDir, uid_t uid) const
{
    QDir().mkpath(QDir(stateDir).filePath(QStringLiteral("users")));
    QSaveFile file(fileFor(stateDir, uid));
    if (!file.open(QIODevice::WriteOnly)) {
        return false;
    }
    file.setPermissions(QFileDevice::ReadOwner | QFileDevice::WriteOwner);
    const QJsonObject o{
        {QStringLiteral("failures"), failures},
        {QStringLiteral("lockedUntil"), double(lockedUntil)},
        {QStringLiteral("lastUnlock"), double(lastUnlock)},
        {QStringLiteral("lastPurpose"), lastPurpose},
    };
    file.write(QJsonDocument(o).toJson(QJsonDocument::Compact));
    return file.commit();
}

qint64 UserState::lockoutLeft(qint64 now) const
{
    return lockedUntil > now ? lockedUntil - now : 0;
}
