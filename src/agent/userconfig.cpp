// SPDX-License-Identifier: GPL-3.0-or-later

#include "userconfig.h"

#include "keyvalue.h"

#include <QDir>
#include <QFileInfo>
#include <QStandardPaths>

UserConfig::UserConfig(QObject *parent)
    : QObject(parent)
{
    // The menu replaces the file instead of editing it, which a watch on the
    // file alone would lose track of. The directory sees the new one arrive.
    const QString dir = QFileInfo(path()).absolutePath();
    QDir().mkpath(dir);
    m_watcher.addPath(dir);
    connect(&m_watcher, &QFileSystemWatcher::directoryChanged, this, &UserConfig::load);
    connect(&m_watcher, &QFileSystemWatcher::fileChanged, this, &UserConfig::load);
    load();
}

QString UserConfig::path()
{
    return QStandardPaths::writableLocation(QStandardPaths::GenericConfigLocation) + QStringLiteral("/plasma-face-unlock/config");
}

void UserConfig::load()
{
    const KeyValueFile kv = KeyValueFile::load(path());
    m_lockScreen = kv.boolean(QStringLiteral("LockScreen"), true);
    m_scanOnWake = kv.boolean(QStringLiteral("ScanOnWake"), true);
    m_scanOnLock = kv.boolean(QStringLiteral("ScanOnLock"), false);
    m_bubble = kv.boolean(QStringLiteral("Bubble"), true);
    m_bubbleStyle = kv.value(QStringLiteral("BubbleStyle"), QStringLiteral("full")) == u"minimal" ? QStringLiteral("minimal") : QStringLiteral("full");
    m_bubbleForPrompts = kv.boolean(QStringLiteral("BubbleForPrompts"), true);
    const QString speed = kv.value(QStringLiteral("AnimationSpeed"), QStringLiteral("normal"));
    m_pace = speed == u"fast" ? 0.6 : speed == u"slow" ? 1.5 : 1.0;

    if (QFileInfo::exists(path()) && !m_watcher.files().contains(path())) {
        m_watcher.addPath(path());
    }
    Q_EMIT changed();
}
