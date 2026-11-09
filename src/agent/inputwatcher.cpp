// SPDX-License-Identifier: GPL-3.0-or-later

#include "inputwatcher.h"

#include <QDBusConnection>
#include <QDBusConnectionInterface>
#include <QDBusMessage>
#include <QDBusPendingCallWatcher>
#include <QDBusPendingReply>
#include <QDBusVariant>
#include <QGuiApplication>
#include <QWaylandClientExtensionTemplate>
#include <QtGui/qguiapplication_platform.h>

#include <algorithm>
#include <functional>
#include <optional>

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

namespace
{
const QString MutterService = QStringLiteral("org.gnome.Mutter.IdleMonitor");
const QString MutterPath = QStringLiteral("/org/gnome/Mutter/IdleMonitor/Core");
const QString UPower = QStringLiteral("org.freedesktop.UPower");
constexpr int UPowerTimeoutMs = 1000;
} // namespace

// GNOME's way: a watch that fires once nothing was touched for the calm,
// then one that fires at the next input. Each fires once and is gone.
//
// Mutter also counts the charger going in or out as activity. Nobody came
// back then, so that does not count here. It does this because of UPower's
// own signal, so by the time the watch fires UPower has the new value.
class MutterIdle : public QObject
{
    Q_OBJECT
public:
    MutterIdle(std::function<void()> input, QObject *parent)
        : QObject(parent)
        , m_input(std::move(input))
    {
        QDBusConnection::sessionBus().connect(MutterService, MutterPath, MutterService, QStringLiteral("WatchFired"), this, SLOT(onFired(uint)));
    }
    ~MutterIdle() override
    {
        stop();
    }

    void watch(int calmMs)
    {
        stop();
        const int generation = m_generation;
        if (calmMs <= 0) {
            watchActive(generation);
            return;
        }
        // Already calm for that long (the scan itself took a while): the
        // idle watch would wait for the next calm, so go straight on.
        call(QStringLiteral("GetIdletime"), {}, [this, generation, calmMs](const QDBusMessage &reply) {
            if (generation != m_generation) {
                return;
            }
            if (reply.arguments().value(0).toULongLong() >= quint64(calmMs)) {
                watchActive(generation);
            } else {
                addWatch(QStringLiteral("AddIdleWatch"), {QVariant::fromValue(quint64(calmMs))}, generation, &m_idle);
            }
        });
    }

    void stop()
    {
        ++m_generation;
        remove(m_idle);
        remove(m_active);
        m_idle = m_active = 0;
    }

private Q_SLOTS:
    void onFired(uint id)
    {
        if (id != 0 && id == m_idle) {
            remove(m_idle);
            m_idle = 0;
            watchActive(m_generation);
        } else if (id != 0 && id == m_active) {
            m_active = 0;
            const int generation = m_generation;
            readOnBattery([this, generation](std::optional<bool> onBattery) {
                if (generation != m_generation) {
                    return;
                }
                if (onBattery && m_onBattery && *onBattery != *m_onBattery) {
                    watchActive(generation);
                } else {
                    m_input();
                }
            });
        }
    }

private:
    void watchActive(int generation)
    {
        readOnBattery([this, generation](std::optional<bool> onBattery) {
            if (generation == m_generation) {
                m_onBattery = onBattery;
            }
        });
        addWatch(QStringLiteral("AddUserActiveWatch"), {}, generation, &m_active);
    }

    // Nothing when there is no UPower. Then all activity counts as input.
    void readOnBattery(std::function<void(std::optional<bool>)> done)
    {
        QDBusMessage msg = QDBusMessage::createMethodCall(UPower,
                                                          QStringLiteral("/org/freedesktop/UPower"),
                                                          QStringLiteral("org.freedesktop.DBus.Properties"),
                                                          QStringLiteral("Get"));
        msg << UPower << QStringLiteral("OnBattery");
        msg.setAutoStartService(false);
        auto *watcher = new QDBusPendingCallWatcher(QDBusConnection::systemBus().asyncCall(msg, UPowerTimeoutMs), this);
        connect(watcher, &QDBusPendingCallWatcher::finished, this, [watcher, done] {
            watcher->deleteLater();
            const QDBusPendingReply<QDBusVariant> reply = *watcher;
            done(reply.isValid() ? std::optional(reply.value().variant().toBool()) : std::nullopt);
        });
    }

    template<typename Done>
    void call(const QString &method, const QVariantList &args, Done done)
    {
        QDBusMessage msg = QDBusMessage::createMethodCall(MutterService, MutterPath, MutterService, method);
        msg.setArguments(args);
        auto *watcher = new QDBusPendingCallWatcher(QDBusConnection::sessionBus().asyncCall(msg), this);
        connect(watcher, &QDBusPendingCallWatcher::finished, this, [watcher, done] {
            watcher->deleteLater();
            done(watcher->reply());
        });
    }

    void addWatch(const QString &method, const QVariantList &args, int generation, uint *slot)
    {
        call(method, args, [this, generation, slot](const QDBusMessage &reply) {
            const uint id = reply.arguments().value(0).toUInt();
            if (generation != m_generation) {
                // Stopped in the meantime.
                remove(id);
                return;
            }
            *slot = id;
        });
    }

    void remove(uint id)
    {
        if (id != 0) {
            call(QStringLiteral("RemoveWatch"), {QVariant::fromValue(id)}, [](const QDBusMessage &) { });
        }
    }

    std::function<void()> m_input;
    int m_generation = 0;
    uint m_idle = 0;
    uint m_active = 0;
    std::optional<bool> m_onBattery;
};

InputWatcher::InputWatcher(QObject *parent)
    : QObject(parent)
    , m_notifier(std::make_unique<IdleNotifier>())
{
}

InputWatcher::~InputWatcher() = default;

void InputWatcher::watch(int calmMs)
{
    stop();
    m_watching = true;
    m_calmMs = calmMs;
    m_calm.start();
    auto *wayland = qGuiApp->nativeInterface<QNativeInterface::QWaylandApplication>();
    if (!m_notifier->isActive() || !wayland || !wayland->seat()) {
        if (!m_mutter && QDBusConnection::sessionBus().interface()->isServiceRegistered(MutterService)) {
            m_mutter = new MutterIdle(
                [this] {
                    QMetaObject::invokeMethod(this, &InputWatcher::input, Qt::QueuedConnection);
                },
                this);
        }
        if (m_mutter) {
            m_mutter->watch(calmMs);
        } else {
            qWarning("no ext_idle_notifier_v1 and no Mutter IdleMonitor, so no telling when somebody comes back");
        }
        return;
    }
    // Hyprland never says resumed for a timeout of 0. A tenth of a second of
    // calm first changes nothing anywhere.
    calmMs = std::max(calmMs, 100);
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

void InputWatcher::noteInput()
{
    if (m_watching && m_calm.elapsed() >= m_calmMs) {
        m_watching = false;
        QMetaObject::invokeMethod(this, &InputWatcher::input, Qt::QueuedConnection);
    }
    m_calm.restart();
}

void InputWatcher::stop()
{
    m_watching = false;
    m_notification.reset();
    if (m_mutter) {
        m_mutter->stop();
    }
}

#include "inputwatcher.moc"
