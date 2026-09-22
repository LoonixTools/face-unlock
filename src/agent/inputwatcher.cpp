// SPDX-License-Identifier: GPL-3.0-or-later

#include "inputwatcher.h"

#include <QGuiApplication>
#include <QWaylandClientExtensionTemplate>
#include <QtGui/qguiapplication_platform.h>

#include <functional>

#include "qwayland-ext-idle-notify-v1.h"

class IdleNotifier : public QWaylandClientExtensionTemplate<IdleNotifier>, public QtWayland::ext_idle_notifier_v1
{
public:
    IdleNotifier()
        : QWaylandClientExtensionTemplate<IdleNotifier>(2)
    {
        initialize();
    }
    ~IdleNotifier() override
    {
        if (isActive()) {
            destroy();
        }
    }
};

class IdleNotification : public QtWayland::ext_idle_notification_v1
{
public:
    IdleNotification(::ext_idle_notification_v1 *object, std::function<void()> resumed)
        : QtWayland::ext_idle_notification_v1(object)
        , m_resumed(std::move(resumed))
    {
    }
    ~IdleNotification() override
    {
        destroy();
    }

protected:
    // Only ever sent after idled: calm first, then this.
    void ext_idle_notification_v1_resumed() override
    {
        if (!m_fired) {
            m_fired = true;
            m_resumed();
        }
    }

private:
    std::function<void()> m_resumed;
    bool m_fired = false;
};

InputWatcher::InputWatcher(QObject *parent)
    : QObject(parent)
    , m_notifier(std::make_unique<IdleNotifier>())
{
}

InputWatcher::~InputWatcher() = default;

void InputWatcher::watch(int calmMs)
{
    m_notification.reset();
    auto *wayland = qGuiApp->nativeInterface<QNativeInterface::QWaylandApplication>();
    if (!m_notifier->isActive() || !wayland || !wayland->seat()) {
        qWarning("no ext_idle_notifier_v1, so no telling when somebody comes back");
        return;
    }
    // Version 1 has only the notification that inhibitors hold back.
    ::ext_idle_notification_v1 *object = m_notifier->QWaylandClientExtension::version() >= 2
        ? m_notifier->get_input_idle_notification(uint32_t(calmMs), wayland->seat())
        : m_notifier->get_idle_notification(uint32_t(calmMs), wayland->seat());
    // Queued: not from inside the Wayland event, which the receiver might
    // answer by watching anew and so destroying the notification.
    m_notification = std::make_unique<IdleNotification>(object, [this] {
        QMetaObject::invokeMethod(this, &InputWatcher::input, Qt::QueuedConnection);
    });
}

void InputWatcher::stop()
{
    m_notification.reset();
}
