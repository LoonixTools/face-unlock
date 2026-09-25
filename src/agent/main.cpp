// SPDX-License-Identifier: GPL-3.0-or-later
//
// face-unlock-agent: the part in the user's session.
//
//   (no arguments)   stay in the background: unlock the lock screen by face,
//                    and show the bubble for every scan
//   --enroll         open the window that sets up a face
//   --demo           play the bubble's animations once, for trying out a
//                    style (--bubble-style minimal) and for screenshots

#include "agentsocket.h"
#include "bubblecontroller.h"
#include "bubblewindow.h"
#include "enrollcontroller.h"
#include "lockcontroller.h"
#include "userconfig.h"

#include "buildconfig.h"

#include <KLocalizedQmlContext>
#include <KLocalizedString>

#include <QCommandLineParser>
#include <QGuiApplication>
#include <QQmlApplicationEngine>
#include <QQmlEngine>
#include <QTimer>

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
    QGuiApplication app(argc, argv);
    app.setApplicationName(QStringLiteral("face-unlock-agent"));
    app.setApplicationVersion(QStringLiteral(FU_VERSION));
    app.setDesktopFileName(QStringLiteral("io.github.loonixtools.face-unlock-agent"));
    KLocalizedString::setApplicationDomain(FU_NAME);

    QCommandLineParser parser;
    parser.setApplicationDescription(i18n("Face unlock for KDE Plasma"));
    parser.addHelpOption();
    parser.addVersionOption();
    const QCommandLineOption enrollOpt(QStringLiteral("enroll"), i18n("Set up a face."));
    const QCommandLineOption nameOpt(QStringLiteral("name"), i18n("What to call the new face."), QStringLiteral("name"));
    const QCommandLineOption demoOpt(QStringLiteral("demo"), i18n("Play the bubble's animations once."));
    // Not "--style": QGuiApplication takes that one for itself.
    const QCommandLineOption styleOpt(QStringLiteral("bubble-style"), i18n("Bubble style for the demo: full or minimal."), QStringLiteral("style"));
    parser.addOptions({enrollOpt, nameOpt, demoOpt, styleOpt});
    parser.process(app);

    if (parser.isSet(enrollOpt)) {
        QString name = parser.value(nameOpt);
        if (name.isEmpty()) {
            name = qEnvironmentVariable("USER");
        }
        return runEnroll(app, name);
    }

    app.setQuitOnLastWindowClosed(false);
    QQmlEngine engine;
    KLocalization::setupLocalizedContext(&engine);

    UserConfig config;
    if (parser.isSet(styleOpt)) {
        config.overrideStyle(parser.value(styleOpt));
    }
    BubbleController bubble(&config);
    BubbleWindow window(&engine, &bubble);

    if (parser.isSet(demoOpt)) {
        scheduleDemo(&bubble);
        return app.exec();
    }

    AgentSocket socket;
    socket.listen();
    QObject::connect(&socket, &AgentSocket::scanEvent, &bubble, &BubbleController::daemonEvent);

    LockController lock(&bubble, &config);
    return app.exec();
}
