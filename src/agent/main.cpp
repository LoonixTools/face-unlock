// SPDX-License-Identifier: GPL-3.0-or-later
//
// face-unlock-agent: the part in the user's session.
//
//   (no arguments)   stay in the background: unlock the lock screen by face,
//                    and show the bubble for every scan
//   --enroll         open the window that sets up a face
//   --lock           lock the screen: with face-unlock's own lock screen
//                    where the lock screen is a program of its own (Hyprland,
//                    Niri), else through logind with the desktop's
//   --demo           play the bubble's animations once, for trying out a
//                    style (--bubble-style minimal) and for screenshots
//   --choose-lock-screen
//                    open the window that asks which lock screen to use
//                    where it is a program of its own, and print the answer

#include "agentsocket.h"
#include "bubblecontroller.h"
#include "bubbleservice.h"
#include "bubblewindow.h"
#include "enrollcontroller.h"
#include "lockcontroller.h"
#include "lockpreview.h"
#include "lockscreencontroller.h"
#include "locktext.h"
#include "sessionlock.h"
#include "userconfig.h"
#include "wallpaper.h"
#include "wayland.h"

#include "buildconfig.h"

#include <KLocalizedQmlContext>
#include <KLocalizedString>

#include <QCommandLineParser>
#include <QDBusConnection>
#include <QDBusMessage>
#include <QFile>
#include <QGuiApplication>
#include <QQmlApplicationEngine>
#include <QQmlEngine>
#include <QScreen>
#include <QTimer>
#include <QWindow>

#include <cstdio>
#include <cstring>
#include <sys/socket.h>
#include <sys/un.h>
#include <unistd.h>

namespace
{
int runEnroll(QGuiApplication &app, const QString &name)
{
    app.setQuitOnLastWindowClosed(true);
    EnrollController controller;
    controller.setName(name);

    QQmlApplicationEngine engine;
    KLocalization::setupLocalizedContext(&engine);
    engine.setInitialProperties({{QStringLiteral("controller"), QVariant::fromValue(&controller)}});
    engine.loadFromModule(QStringLiteral("FaceUnlock"), QStringLiteral("Enroll"));
    if (engine.rootObjects().isEmpty()) {
        return 2;
    }

    int code = 1;
    QObject::connect(&controller, &EnrollController::completed, &app, [&code](bool ok) {
        code = ok ? 0 : 1;
        QCoreApplication::quit();
    });
    app.exec();
    return controller.state() == u"done" ? 0 : code;
}

// The window of --choose-lock-screen. Prints "own" or "yours" for the menu,
// which carries it out.
int runChooseLockScreen(QGuiApplication &app)
{
    app.setQuitOnLastWindowClosed(true);
    UserConfig config;

    // Both choices show this screen as it would look: face-unlock's lock
    // screen as it is, with a scan going on, and the user's own rebuilt.
    const QScreen *screen = QGuiApplication::primaryScreen();
    const QString output = screen ? screen->name() : QString();
    const QSize size = screen ? screen->size() : QSize(1920, 1080);
    const QUrl desktop = Wallpaper::pick(Wallpaper::forLock({}), output);
    const QUrl ownWallpaper = config.lockWallpaper().isEmpty() ? desktop : Wallpaper::pick(Wallpaper::forLock(config.lockWallpaper()), output);
    const QString locker = LockPreview::locker();

    LockScreenController lock;
    lock.setBlur(config.lockBlur());
    BubbleController bubble(&config);
    bubble.scanStarted();
    // A scan that goes on: the bubble closes by itself after a while.
    QTimer rescan;
    QObject::connect(&rescan, &QTimer::timeout, &bubble, &BubbleController::scanStarted);
    rescan.start(20000);

    QQmlApplicationEngine engine;
    KLocalization::setupLocalizedContext(&engine);
    engine.setInitialProperties({{QStringLiteral("current"), config.lockScreenStyle()},
                                 {QStringLiteral("locker"), locker},
                                 {QStringLiteral("widgets"), LockPreview::widgets(locker, size, output, desktop)},
                                 {QStringLiteral("screenSize"), size},
                                 {QStringLiteral("ownWallpaper"), ownWallpaper},
                                 {QStringLiteral("lock"), QVariant::fromValue(&lock)},
                                 {QStringLiteral("bubble"), QVariant::fromValue(&bubble)}});
    engine.loadFromModule(QStringLiteral("FaceUnlock"), QStringLiteral("LockChoice"));
    auto *window = engine.rootObjects().isEmpty() ? nullptr : qobject_cast<QWindow *>(engine.rootObjects().constFirst());
    if (!window) {
        return 2;
    }
    // Shown only now, at the size its texts need (see LockChoice.qml).
    window->show();
    app.exec();
    const QString answer = window->property("answer").toString();
    if (answer.isEmpty()) {
        return 1;
    }
    std::printf("%s\n", qPrintable(answer));
    return 0;
}

// Where --lock tells the command that ran it that the screen is locked, when
// it forked to stay behind as the lock screen. -1: it did not.
int readyFd = -1;

void signalReady(bool ok)
{
    if (readyFd >= 0) {
        if (ok) {
            [[maybe_unused]] const ssize_t n = write(readyFd, "1", 1);
        }
        close(readyFd);
        readyFd = -1;
    }
}

// Whether an agent listens on its socket. Asked before Qt is up, to decide
// whether to fork.
bool agentListening()
{
    const char *runtime = getenv("XDG_RUNTIME_DIR");
    if (!runtime) {
        return false;
    }
    sockaddr_un addr{};
    addr.sun_family = AF_UNIX;
    const int len = snprintf(addr.sun_path, sizeof(addr.sun_path), "%s/face-unlock/agent.socket", runtime);
    if (len <= 0 || size_t(len) >= sizeof(addr.sun_path)) {
        return false;
    }
    const int fd = socket(AF_UNIX, SOCK_STREAM | SOCK_CLOEXEC, 0);
    const bool ok = fd >= 0 && connect(fd, reinterpret_cast<sockaddr *>(&addr), sizeof(addr)) == 0;
    if (fd >= 0) {
        close(fd);
    }
    return ok;
}

// With no agent running, --lock stays behind as the lock screen until it is
// unlocked. The command that asked for it returns as soon as the screen is
// locked, the way swaylock -f does: an idle daemon locking before sleep
// waits for that, and not a moment longer.
void forkForLock(int argc, char **argv)
{
    bool lock = false;
    for (int i = 1; i < argc; ++i) {
        lock = lock || std::strcmp(argv[i], "--lock") == 0;
    }
    int fds[2];
    if (!lock || agentListening() || pipe(fds) != 0) {
        return;
    }
    const pid_t pid = fork();
    if (pid < 0) {
        close(fds[0]);
        close(fds[1]);
        return;
    }
    if (pid > 0) {
        close(fds[1]);
        char c = 0;
        const bool locked = read(fds[0], &c, 1) == 1;
        _exit(locked ? 0 : 1);
    }
    close(fds[0]);
    setsid();
    readyFd = fds[1];
}

// Plasma and GNOME lock with their own lock screen when logind asks them to,
// as `loginctl lock-session` does.
int lockThroughLogind()
{
    const QString id = qEnvironmentVariable("XDG_SESSION_ID");
    QDBusMessage call = QDBusMessage::createMethodCall(QStringLiteral("org.freedesktop.login1"),
                                                       QStringLiteral("/org/freedesktop/login1"),
                                                       QStringLiteral("org.freedesktop.login1.Manager"),
                                                       QStringLiteral("LockSession"));
    call << (id.isEmpty() ? QStringLiteral("auto") : id);
    const QDBusMessage reply = QDBusConnection::systemBus().call(call);
    if (reply.type() == QDBusMessage::ErrorMessage) {
        qWarning("logind would not lock: %s", qPrintable(reply.errorMessage()));
        return 1;
    }
    return 0;
}

// The whole life of a bubble, twice: a face that is recognised after a blink,
// then one that is not.
void scheduleDemo(BubbleController *bubble)
{
    const QList<std::pair<int, std::function<void()>>> steps = {
        {300, [bubble] { bubble->scanStarted(); }},
        {900, [bubble] { bubble->faceFound(); }},
        {1900, [bubble] { bubble->hint(QStringLiteral("blink")); }},
        {3000, [bubble] { bubble->succeeded(); }},
        {5600, [bubble] { bubble->scanStarted(); }},
        {6200, [bubble] { bubble->faceFound(); }},
        {7800, [bubble] { bubble->failed(QStringLiteral("mismatch")); }},
        {11000, [] { QCoreApplication::quit(); }},
    };
    for (const auto &[at, what] : steps) {
        QTimer::singleShot(at, bubble, what);
    }
}
} // namespace

int main(int argc, char **argv)
{
    forkForLock(argc, argv);
    QGuiApplication app(argc, argv);
    app.setApplicationName(QStringLiteral("face-unlock-agent"));
    app.setApplicationVersion(QStringLiteral(FU_VERSION));
    app.setDesktopFileName(QStringLiteral("io.github.loonixtools.face-unlock-agent"));
    KLocalizedString::setApplicationDomain(FU_NAME);

    QCommandLineParser parser;
    parser.setApplicationDescription(i18n("Face unlock for the lock screen, sudo and admin prompts"));
    parser.addHelpOption();
    parser.addVersionOption();
    const QCommandLineOption enrollOpt(QStringLiteral("enroll"), i18n("Set up a face."));
    const QCommandLineOption nameOpt(QStringLiteral("name"), i18n("What to call the new face."), QStringLiteral("name"));
    const QCommandLineOption lockOpt(QStringLiteral("lock"), i18n("Lock the screen."));
    const QCommandLineOption demoOpt(QStringLiteral("demo"), i18n("Play the bubble's animations once."));
    // Not "--style": QGuiApplication takes that one for itself.
    const QCommandLineOption styleOpt(QStringLiteral("bubble-style"), i18n("Bubble style for the demo: full or minimal."), QStringLiteral("style"));
    const QCommandLineOption chooseOpt(QStringLiteral("choose-lock-screen"), i18n("Ask which lock screen to use, and print the answer."));
    parser.addOptions({enrollOpt, nameOpt, lockOpt, demoOpt, styleOpt, chooseOpt});
    parser.process(app);

    if (parser.isSet(chooseOpt)) {
        return runChooseLockScreen(app);
    }

    if (parser.isSet(enrollOpt)) {
        QString name = parser.value(nameOpt);
        if (name.isEmpty()) {
            name = qEnvironmentVariable("USER");
        }
        return runEnroll(app, name);
    }

    const bool lockNow = parser.isSet(lockOpt);
    if (lockNow && !SessionLock::available()) {
        const int rc = lockThroughLogind();
        signalReady(rc == 0);
        return rc;
    }

    app.setQuitOnLastWindowClosed(false);
    QQmlEngine engine;
    KLocalization::setupLocalizedContext(&engine);

    UserConfig config;
    if (parser.isSet(styleOpt)) {
        config.overrideStyle(parser.value(styleOpt));
    }
    BubbleController bubble(&config);
    // The bubble floats above everything as a layer-shell surface. GNOME has
    // none, and a normal window cannot stay on top or pick its place: there
    // face-unlock's GNOME Shell extension draws it, from what this tells it.
    std::unique_ptr<BubbleWindow> window;
    if (Wayland::hasGlobal("zwlr_layer_shell_v1")) {
        window = std::make_unique<BubbleWindow>(&engine, &bubble);
    }

    if (parser.isSet(demoOpt)) {
        if (!window) {
            qWarning("this desktop has no layer-shell; on GNOME the extension draws the bubble");
            return 1;
        }
        scheduleDemo(&bubble);
        return app.exec();
    }

    AgentSocket socket;
    if (AgentSocket::running()) {
        if (lockNow) {
            return AgentSocket::requestLock() ? 0 : 1;
        }
        qInfo("an agent is already running in this session");
        return 0;
    }
    socket.listen();
    QObject::connect(&socket, &AgentSocket::scanEvent, &bubble, &BubbleController::daemonEvent);
    std::unique_ptr<BubbleService> service;
    if (!window) {
        service = std::make_unique<BubbleService>(&bubble);
    }

    LockScreenController screen;
    std::unique_ptr<SessionLock> sessionLock;
    if (SessionLock::available()) {
        sessionLock = std::make_unique<SessionLock>(&engine, &bubble, &screen, &config);
        SessionLock *lock = sessionLock.get();
        QObject::connect(&socket, &AgentSocket::lockRequested, lock, [lock, &socket] {
            lock->lock();
            if (lock->isConfirmed()) {
                socket.confirmLock();
            }
        });
        QObject::connect(lock, &SessionLock::confirmed, &socket, &AgentSocket::confirmLock);
    }
    LockController lock(&bubble, &config, sessionLock.get(), &screen);
    // hyprlock covers the bubble: it can show it as a line of text instead.
    std::unique_ptr<LockText> text;
    if (sessionLock && !lockNow) {
        text = std::make_unique<LockText>(&bubble, &config);
    }

    if (lockNow) {
        // No agent was running, so this one is only here for the lock. It
        // goes once the lock is gone and the bubble has finished.
        QObject::connect(sessionLock.get(), &SessionLock::lockedChanged, &app, [&bubble](bool locked) {
            if (locked) {
                return;
            }
            if (!bubble.shown()) {
                QCoreApplication::quit();
            }
            QObject::connect(&bubble, &BubbleController::shownChanged, qApp, [&bubble] {
                if (!bubble.shown()) {
                    QCoreApplication::quit();
                }
            });
            QTimer::singleShot(5000, qApp, &QCoreApplication::quit);
        });
        QObject::connect(sessionLock.get(), &SessionLock::confirmed, &app, [] {
            signalReady(true);
        });
        sessionLock->lock();
        if (!sessionLock->isLocked()) {
            signalReady(false);
            return 1;
        }
    } else if (sessionLock && QFile::exists(SessionLock::markerPath())) {
        // The last agent went away with the screen locked, and the compositor
        // kept it locked. Take the lock back, so it can be opened again.
        sessionLock->lock();
    }
    return app.exec();
}
