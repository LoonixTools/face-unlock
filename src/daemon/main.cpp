// SPDX-License-Identifier: GPL-3.0-or-later
//
// plasma-face-unlockd: the part that runs as root.
//
// It owns the camera and the face data, and it is the only thing that does.
// Everything else (the lock screen agent, sudo, the setup window, the menu)
// asks it over one socket. systemd starts it on the first connection and it
// exits again after a minute with nothing to do, so most of the time it is not
// running at all.

#include "server.h"

#include "buildconfig.h"

#include <QCommandLineParser>
#include <QCoreApplication>
#include <QSocketNotifier>

#include <csignal>
#include <sys/signalfd.h>
#include <sys/stat.h>
#include <unistd.h>

namespace
{
// sd_listen_fds(), without linking libsystemd for one function.
int systemdSocket()
{
    const QByteArray pid = qgetenv("LISTEN_PID");
    const QByteArray fds = qgetenv("LISTEN_FDS");
    if (pid.isEmpty() || fds.isEmpty() || pid.toLongLong() != getpid() || fds.toInt() < 1) {
        return -1;
    }
    qunsetenv("LISTEN_PID");
    qunsetenv("LISTEN_FDS");
    qunsetenv("LISTEN_FDNAMES");
    return 3;
}

// SIGTERM from systemd is the normal way this ends. The camera thread is
// cancelled and joined on the way out rather than being cut off mid-frame.
void quitOnSignals(QCoreApplication &app)
{
    sigset_t mask;
    sigemptyset(&mask);
    sigaddset(&mask, SIGTERM);
    sigaddset(&mask, SIGINT);
    sigprocmask(SIG_BLOCK, &mask, nullptr);
    const int fd = signalfd(-1, &mask, SFD_CLOEXEC | SFD_NONBLOCK);
    if (fd < 0) {
        return;
    }
    auto *notifier = new QSocketNotifier(fd, QSocketNotifier::Read, &app);
    QObject::connect(notifier, &QSocketNotifier::activated, &app, [fd] {
        signalfd_siginfo info;
        while (read(fd, &info, sizeof(info)) > 0) {
        }
        QCoreApplication::quit();
    });
}
} // namespace

int main(int argc, char **argv)
{
    // Face data and state are for root's eyes only, whatever creates them.
    umask(077);

    QCoreApplication app(argc, argv);
    app.setApplicationName(QStringLiteral("plasma-face-unlockd"));
    app.setApplicationVersion(QStringLiteral(PFU_VERSION));
    qSetMessagePattern(QStringLiteral("%{if-warning}warning: %{endif}%{if-critical}error: %{endif}%{message}"));

    QCommandLineParser parser;
    parser.setApplicationDescription(QStringLiteral("Face unlock daemon for KDE Plasma"));
    parser.addHelpOption();
    parser.addVersionOption();
    const QCommandLineOption socketOpt(QStringLiteral("socket"), QStringLiteral("Listen here instead of the system socket."), QStringLiteral("path"),
                                       QStringLiteral(PFU_SOCKET));
    const QCommandLineOption stateOpt(QStringLiteral("state-dir"), QStringLiteral("Where face data is kept."), QStringLiteral("dir"),
                                      QStringLiteral(PFU_STATEDIR));
    const QCommandLineOption configOpt(QStringLiteral("config"), QStringLiteral("The settings file."), QStringLiteral("file"), QStringLiteral(PFU_CONFIG));
    const QCommandLineOption modelOpt(QStringLiteral("models"), QStringLiteral("Where the networks are."), QStringLiteral("dir"),
                                      QStringLiteral(PFU_MODELDIR));
    const QCommandLineOption idleOpt(QStringLiteral("idle-exit"), QStringLiteral("Exit after this many idle seconds (0: never)."), QStringLiteral("seconds"));
    parser.addOptions({socketOpt, stateOpt, configOpt, modelOpt, idleOpt});
    parser.process(app);

    ServerOptions options;
    options.socketPath = parser.value(socketOpt);
    options.stateDir = parser.value(stateOpt);
    options.configPath = parser.value(configOpt);
    options.modelDir = parser.value(modelOpt);
    options.systemdFd = systemdSocket();
    // Started by the socket: go away again when there is nothing to do.
    // Started by hand (development): stay.
    options.idleSeconds = parser.isSet(idleOpt) ? parser.value(idleOpt).toInt() : (options.systemdFd >= 0 ? 60 : 0);
    options.askPolkit = geteuid() == 0;
    if (!options.askPolkit) {
        qInfo("not running as root: face changes are not checked with polkit (development only)");
    }

    quitOnSignals(app);

    Server server(options);
    QString error;
    if (!server.start(&error)) {
        qCritical("cannot listen: %s", qPrintable(error));
        return 1;
    }
    return app.exec();
}
