// SPDX-License-Identifier: GPL-3.0-or-later
//
// What the daemon remembers about each user between scans: failures in a row,
// the lockout they lead to, and the last successful unlock.
//
// Kept on disk rather than in memory. The daemon exits when it has been idle
// for a minute, and a lockout that ended with the process would be a lockout
// that waiting a minute gets around.

#pragma once

#include <QString>

#include <sys/types.h>

struct UserState {
    int failures = 0;
    qint64 lockedUntil = 0;
    qint64 lastUnlock = 0;
    QString lastPurpose;

    static UserState load(const QString &stateDir, uid_t uid);
    bool save(const QString &stateDir, uid_t uid) const;

    // Seconds left, 0 when not locked out.
    qint64 lockoutLeft(qint64 now) const;
};
