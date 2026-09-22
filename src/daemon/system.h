// SPDX-License-Identifier: GPL-3.0-or-later
//
// Small questions about the machine and its users.

#pragma once

#include <QString>

#include <optional>
#include <sys/types.h>

namespace System
{
std::optional<uid_t> uidOf(const QString &user);
QString nameOf(uid_t uid);

// Whether a laptop lid is shut. Asks logind, and falls back to ACPI.
bool lidClosed();

// When a process started, in clock ticks since boot, as polkit wants it to
// tell a process from a later one that got the same pid.
quint64 processStartTime(pid_t pid);
} // namespace System
