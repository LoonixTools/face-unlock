// SPDX-License-Identifier: GPL-3.0-or-later
//
// The lock screens that are programs of their own and take signals from
// outside: hyprlock, swaylock and gtklock open on SIGUSR1, hyprlock reads
// its labels again on SIGUSR2.

#pragma once

#include <QByteArray>
#include <QList>

#include <sys/types.h>

namespace Lockers
{
// The ones of this user on this display, or only those called name.
QList<pid_t> find(const QByteArray &name = {});
// Sends sig to each that is ready for it, and says how many were not. A
// lock screen only handles its signals once the screen is locked: before
// that, the signal kills it, and the compositor keeps the screen locked with
// nobody to open it.
int signal(int sig, const QByteArray &name = {});
// Whether one of them handles sig: gtklock only opens on SIGUSR1 since 2025,
// and none does before it has locked.
bool ready(int sig);
}
