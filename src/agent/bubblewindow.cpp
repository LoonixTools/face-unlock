// SPDX-License-Identifier: GPL-3.0-or-later

#include "bubblewindow.h"

#include "bubblecontroller.h"

#include <LayerShellQt/Window>

#include <QGuiApplication>
#include <QQuickView>
#include <QWaylandClientExtensionTemplate>
#include <qpa/qplatformwindow_p.h>

#include "qwayland-kde-lockscreen-overlay-v1.h"

namespace
{
// Big enough for the open island plus its shadow. Only the content moves;
// the window itself never changes size, which keeps the compositor from
// having to reconfigure the surface in the middle of an animation.
constexpr int WindowWidth = 420;
constexpr int WindowHeight = 300;
} // namespace

class LockscreenOverlay : public QWaylandClientExtensionTemplate<LockscreenOverlay>, public QtWayland::kde_lockscreen_overlay_v1
{
public:
    LockscreenOverlay()
        : QWaylandClientExtensionTemplate<LockscreenOverlay>(1)
    {
        initialize();
    }
    ~LockscreenOverlay() override
    {
        if (isActive()) {
            destroy();
        }
    }
};

BubbleWindow::BubbleWindow(QQmlEngine *engine, BubbleController *controller, QObject *parent)
    : QObject(parent)
    , m_engine(engine)
    , m_controller(controller)
    , m_overlay(std::make_unique<LockscreenOverlay>())
{
    connect(m_controller, &BubbleController::shownChanged, this, &BubbleWindow::update);
    create();
}

BubbleWindow::~BubbleWindow()
{
    delete m_view;
}

void BubbleWindow::create()
{
    m_view = new QQuickView(m_engine, nullptr);
    m_view->setColor(Qt::transparent);
    m_view->setFlags(Qt::FramelessWindowHint | Qt::WindowDoesNotAcceptFocus | Qt::WindowTransparentForInput);
    m_view->resize(WindowWidth, WindowHeight);
    m_view->setScreen(QGuiApplication::primaryScreen());

    if (auto *layer = LayerShellQt::Window::get(m_view)) {
        layer->setLayer(LayerShellQt::Window::LayerOverlay);
        layer->setAnchors(LayerShellQt::Window::AnchorTop);
        // -1: sit at the very top edge, over a top panel if there is one,
        // rather than being pushed down below it.
        layer->setExclusiveZone(-1);
        layer->setKeyboardInteractivity(LayerShellQt::Window::KeyboardInteractivityNone);
        // KWin makes a window type of the scope and takes one it does not
        // know for a normal window. Its scale effect then opens and closes
        // that with a blur forced behind the whole, mostly clear window: a
        // faint box with sharp corners around the bubble. An on-screen
        // display only fades, which a clear window does not show.
        layer->setScope(QStringLiteral("on-screen-display"));
#ifdef FU_LAYERSHELL_HAS_SCREEN
        layer->setScreen(QGuiApplication::primaryScreen());
#endif
    }

    m_view->setInitialProperties({{QStringLiteral("bubble"), QVariant::fromValue(m_controller)}});
    m_view->loadFromModule(QStringLiteral("FaceUnlock"), QStringLiteral("Bubble"));
    if (m_view->status() == QQuickView::Error) {
        for (const QQmlError &e : m_view->errors()) {
            qWarning("%s", qPrintable(e.toString()));
        }
    }

    m_view->create();
    if (auto *wayland = m_view->nativeInterface<QNativeInterface::Private::QWaylandWindow>()) {
        connect(wayland, &QNativeInterface::Private::QWaylandWindow::surfaceRoleCreated, this, &BubbleWindow::allowOverLockscreen);
    }
}

void BubbleWindow::allowOverLockscreen()
{
    static bool warned = false;
    auto *wayland = m_view ? m_view->nativeInterface<QNativeInterface::Private::QWaylandWindow>() : nullptr;
    if (!wayland || !wayland->surface()) {
        return;
    }
    if (!m_overlay->isActive()) {
        // Only KWin has the protocol. Anywhere else there is nothing to fix.
        if (!warned && qEnvironmentVariable("XDG_CURRENT_DESKTOP").split(u':').contains(u"KDE")) {
            warned = true;
            qWarning("KWin does not let this program show above the lock screen; is its desktop file installed?");
        }
        return;
    }
    m_overlay->allow(wayland->surface());
}

void BubbleWindow::update()
{
    if (!m_view) {
        return;
    }
    if (m_controller->shown()) {
        m_view->show();
    } else {
        m_view->hide();
    }
}
