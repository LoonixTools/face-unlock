// SPDX-License-Identifier: GPL-3.0-or-later

#include "sessionlock.h"

#include "bubblecontroller.h"
#include "lockscreencontroller.h"
#include "userconfig.h"
#include "wallpaper.h"
#include "wayland.h"

#include <QDir>
#include <QFile>
#include <QFileInfo>
#include <QGuiApplication>
#include <QQuickView>
#include <QScreen>
#include <QStandardPaths>
#include <QUrl>
#include <QtWaylandClient/private/qwaylanddisplay_p.h>
#include <QtWaylandClient/private/qwaylandintegration_p.h>
#include <QtWaylandClient/private/qwaylandscreen_p.h>
#include <QtWaylandClient/private/qwaylandshellintegration_p.h>
#include <QtWaylandClient/private/qwaylandshellsurface_p.h>
#include <QtWaylandClient/private/qwaylandsurface_p.h>
#include <QtWaylandClient/private/qwaylandwindow_p.h>

#include "qwayland-ext-session-lock-v1.h"

// The manager, and through it the role every lock window gets.
class LockIntegration : public QtWaylandClient::QWaylandShellIntegrationTemplate<LockIntegration>, public QtWayland::ext_session_lock_manager_v1
{
public:
    LockIntegration()
        : QWaylandShellIntegrationTemplate<LockIntegration>(1)
    {
    }
    ~LockIntegration() override
    {
        if (object()) {
            destroy();
        }
    }
    QtWaylandClient::QWaylandShellSurface *createShellSurface(QtWaylandClient::QWaylandWindow *window) override;

    // The lock the surfaces belong to.
    ::ext_session_lock_v1 *current = nullptr;
};

// One screen's lock surface. Like LayerShellQt's layer surface: the size
// comes from the compositor, and nothing is drawn before it has sent one.
class LockSurface : public QtWaylandClient::QWaylandShellSurface, public QtWayland::ext_session_lock_surface_v1
{
public:
    LockSurface(::ext_session_lock_v1 *lock, QtWaylandClient::QWaylandWindow *window)
        : QWaylandShellSurface(window)
    {
        init(ext_session_lock_v1_get_lock_surface(lock, window->waylandSurface()->object(), window->waylandScreen()->output()));
    }
    ~LockSurface() override
    {
        destroy();
    }
    bool isExposed() const override
    {
        return m_configured;
    }
    // Qt commits a new role at once, as xdg-shell wants. A lock surface must
    // not be committed before it has acknowledged its first size.
    bool commitSurfaceRole() const override
    {
        return false;
    }
    void applyConfigure() override
    {
        window()->resizeFromApplyConfigure(m_size);
    }

protected:
    void ext_session_lock_surface_v1_configure(uint32_t serial, uint32_t width, uint32_t height) override
    {
        ack_configure(serial);
        m_size = QSize(int(width), int(height));
        if (!m_configured) {
            m_configured = true;
            applyConfigure();
            window()->updateExposure();
        } else {
            window()->applyConfigureWhenPossible();
        }
    }

private:
    QSize m_size;
    bool m_configured = false;
};

QtWaylandClient::QWaylandShellSurface *LockIntegration::createShellSurface(QtWaylandClient::QWaylandWindow *window)
{
    return new LockSurface(current, window);
}

class Lock : public QtWayland::ext_session_lock_v1
{
public:
    Lock(::ext_session_lock_v1 *object, SessionLock *owner)
        : QtWayland::ext_session_lock_v1(object)
        , m_owner(owner)
    {
    }

protected:
    // Queued: not from inside the Wayland event, which the answer to it might
    // destroy.
    void ext_session_lock_v1_locked() override
    {
        QMetaObject::invokeMethod(m_owner, &SessionLock::onLocked, Qt::QueuedConnection);
    }
    void ext_session_lock_v1_finished() override
    {
        QMetaObject::invokeMethod(m_owner, &SessionLock::onFinished, Qt::QueuedConnection);
    }

private:
    SessionLock *m_owner;
};

SessionLock::SessionLock(QQmlEngine *engine, BubbleController *bubble, LockScreenController *screen, UserConfig *config, QObject *parent)
    : QObject(parent)
    , m_engine(engine)
    , m_bubble(bubble)
    , m_screen(screen)
    , m_config(config)
{
    connect(qGuiApp, &QGuiApplication::screenAdded, this, [this](QScreen *s) {
        if (m_lock) {
            addScreen(s);
        }
    });
    connect(qGuiApp, &QGuiApplication::screenRemoved, this, &SessionLock::removeScreen);
    connect(m_screen, &LockScreenController::authenticated, this, &SessionLock::unlock);
}

SessionLock::~SessionLock()
{
    // Leaving with the lock held keeps the session locked, which is the
    // point. The windows go, the lock stays.
    qDeleteAll(m_views);
    delete m_lock;
    delete m_integration;
}

bool SessionLock::available()
{
    return Wayland::hasGlobal("ext_session_lock_manager_v1");
}

QString SessionLock::markerPath()
{
    return QStandardPaths::writableLocation(QStandardPaths::RuntimeLocation) + QStringLiteral("/face-unlock/locked");
}

void SessionLock::lock()
{
    if (m_lock) {
        return;
    }
    auto *qpa = QtWaylandClient::QWaylandIntegration::instance();
    if (!m_integration && qpa) {
        m_integration = new LockIntegration;
        if (!m_integration->initialize(qpa->display())) {
            delete m_integration;
            m_integration = nullptr;
        }
    }
    if (!m_integration) {
        qWarning("the compositor has no ext_session_lock_manager_v1, so this cannot lock the screen");
        return;
    }

    m_confirmed = false;
    m_unlockWanted = false;
    m_lock = new Lock(m_integration->lock(), this);
    m_integration->current = m_lock->object();
    m_screen->reset();
    m_pictures = Wallpaper::forLock(m_config->lockWallpaper());
    m_screen->setBlur(m_config->lockBlur());
    for (QScreen *s : QGuiApplication::screens()) {
        addScreen(s);
    }

    QDir().mkpath(QFileInfo(markerPath()).absolutePath());
    QFile marker(markerPath());
    if (!marker.open(QIODevice::WriteOnly)) {
        qWarning("cannot write %s", qPrintable(markerPath()));
    }
    Q_EMIT lockedChanged(true);
}

void SessionLock::addScreen(QScreen *screen)
{
    if (m_views.contains(screen) || !dynamic_cast<QtWaylandClient::QWaylandScreen *>(screen->handle())) {
        return;
    }
    auto *view = new QQuickView(m_engine, nullptr);
    view->setColor(Qt::black);
    view->setScreen(screen);
    view->setResizeMode(QQuickView::SizeRootObjectToView);
    view->resize(screen->size());
    view->setInitialProperties({{QStringLiteral("lock"), QVariant::fromValue(m_screen)},
                                {QStringLiteral("bubble"), QVariant::fromValue(m_bubble)},
                                {QStringLiteral("wallpaper"), Wallpaper::pick(m_pictures, screen->name())},
                                {QStringLiteral("primary"), screen == QGuiApplication::primaryScreen()}});
    view->loadFromModule(QStringLiteral("FaceUnlock"), QStringLiteral("LockScreen"));
    for (const QQmlError &e : view->errors()) {
        qWarning("%s", qPrintable(e.toString()));
    }
    view->create();
    if (auto *wayland = dynamic_cast<QtWaylandClient::QWaylandWindow *>(view->handle())) {
        wayland->setShellIntegration(m_integration);
    }
    view->show();
    view->requestActivate();
    m_views.insert(screen, view);
}

void SessionLock::removeScreen(QScreen *screen)
{
    if (auto view = m_views.take(screen)) {
        delete view;
    }
}

void SessionLock::onLocked()
{
    if (!m_lock) {
        return;
    }
    m_confirmed = true;
    Q_EMIT confirmed();
    if (m_unlockWanted) {
        unlock();
    }
}

void SessionLock::onFinished()
{
    if (!m_lock) {
        return;
    }
    // Refused (another lock screen is up) or ended by the compositor.
    if (m_confirmed) {
        m_lock->unlock_and_destroy();
    } else {
        qWarning("the compositor did not let this lock the screen; is another lock screen running?");
        m_lock->destroy();
    }
    release();
}

void SessionLock::unlock()
{
    if (!m_lock) {
        return;
    }
    // Unlocking before the compositor has confirmed the lock is a protocol
    // error. It follows as soon as it has.
    if (!m_confirmed) {
        m_unlockWanted = true;
        return;
    }
    m_lock->unlock_and_destroy();
    release();
}

void SessionLock::release()
{
    qDeleteAll(m_views);
    m_views.clear();
    delete m_lock;
    m_lock = nullptr;
    m_confirmed = false;
    m_integration->current = nullptr;
    QFile::remove(markerPath());
    // Out to the compositor now: an agent started only for this lock quits
    // straight after.
    Wayland::sync();
    m_screen->reset();
    Q_EMIT lockedChanged(false);
}
