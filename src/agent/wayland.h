// SPDX-License-Identifier: GPL-3.0-or-later
//
// What the compositor offers. Not every desktop has everything: GNOME has no
// layer-shell (so no bubble) and no ext-idle-notify, and only compositors
// whose lock screen is a program of its own have ext-session-lock.

#pragma once

namespace Wayland
{
// Whether the compositor announces this interface, for example
// "zwlr_layer_shell_v1". Asked once, on an event queue of its own, so Qt's
// own handling of the connection is not disturbed.
bool hasGlobal(const char *interface);
} // namespace Wayland
