// SPDX-License-Identifier: GPL-3.0-or-later

#include "wayland.h"

#include <QByteArray>
#include <QGuiApplication>
#include <QSet>

#include <wayland-client.h>

namespace
{
void onGlobal(void *data, wl_registry *, uint32_t, const char *interface, uint32_t)
{
    static_cast<QSet<QByteArray> *>(data)->insert(QByteArray(interface));
}

void onGlobalRemove(void *, wl_registry *, uint32_t)
{
}

const wl_registry_listener listener = {onGlobal, onGlobalRemove};

const QSet<QByteArray> &globals()
{
    static QSet<QByteArray> names;
    static bool asked = false;
    if (asked) {
        return names;
    }
    asked = true;

    auto *app = qGuiApp ? qGuiApp->nativeInterface<QNativeInterface::QWaylandApplication>() : nullptr;
    wl_display *display = app ? app->display() : nullptr;
    if (!display) {
        return names;
    }
    wl_event_queue *queue = wl_display_create_queue(display);
    auto *wrapped = static_cast<wl_display *>(wl_proxy_create_wrapper(display));
    wl_proxy_set_queue(reinterpret_cast<wl_proxy *>(wrapped), queue);
    wl_registry *registry = wl_display_get_registry(wrapped);
    wl_registry_add_listener(registry, &listener, &names);
    wl_display_roundtrip_queue(display, queue);
    wl_registry_destroy(registry);
    wl_proxy_wrapper_destroy(wrapped);
    wl_event_queue_destroy(queue);
    return names;
}
} // namespace

bool Wayland::hasGlobal(const char *interface)
{
    return globals().contains(QByteArray(interface));
}
