// SPDX-License-Identifier: GPL-3.0-or-later
//
// Asking polkit whether a process may change face data.
//
// Adding a face is adding a way in, so it asks for the password first, the
// same way a phone asks for its code before it sets up a face. The question
// goes to the polkit agent of the person's own session (on Plasma, the
// familiar password dialog), which is why the daemon never sees the
// password.

#pragma once

#include <QObject>
#include <functional>

#include <sys/types.h>

namespace Polkit
{
inline constexpr char ManageAction[] = "io.github.loonixtools.plasma-face-unlock.manage";

// Calls done(true) when the process may go ahead. Interactive: this can take
// as long as it takes somebody to type a password.
void checkAuthorization(pid_t pid, uid_t uid, const char *action, QObject *context, std::function<void(bool)> done);
} // namespace Polkit
