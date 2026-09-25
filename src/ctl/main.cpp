// SPDX-License-Identifier: GPL-3.0-or-later
//
// face-unlock-ctl: the shell code's way to the daemon.
//
// bash has no Unix sockets, so this sends one request and prints every
// message that comes back as one line of tab separated key=value pairs:
//
//   event=frame	score=0.71	yaw=3.2	...
//   event=result	ok=true	name=Felix	score=0.74
//
// Lists (faces, cameras) come one line per entry, with event=item. Nothing
// in here is meant for people to read; the menu turns it into sentences.
//
// Exit status: 0 when the final result was ok, 1 when it was not, 2 when the
// daemon could not be reached.

#include "buildconfig.h"

#include <QCoreApplication>
#include <QJsonArray>
#include <QJsonDocument>
#include <QJsonObject>
#include <QLocalSocket>
#include <QStringList>

#include <cstdio>

namespace
{
QString flat(const QJsonValue &v)
{
    QString s;
    switch (v.type()) {
    case QJsonValue::Bool:
        s = v.toBool() ? QStringLiteral("true") : QStringLiteral("false");
        break;
    case QJsonValue::Double: {
        const double d = v.toDouble();
        s = (d == double(qint64(d)) && std::abs(d) < 1e15) ? QString::number(qint64(d)) : QString::number(d, 'f', 3);
        break;
    }
    case QJsonValue::String:
        s = v.toString();
        break;
    default:
        s = QString::fromUtf8(QJsonDocument(v.toObject()).toJson(QJsonDocument::Compact));
        break;
    }
    // Nothing a line or a field could be split on.
    s.replace(QLatin1Char('\t'), QLatin1Char(' '));
    s.replace(QLatin1Char('\n'), QLatin1Char(' '));
    return s;
}

void printObject(const QJsonObject &o, const QString &event)
{
    QStringList fields{QStringLiteral("event=") + event};
    for (auto it = o.constBegin(); it != o.constEnd(); ++it) {
        if (it.key() == u"event" || it.value().isArray() || it.key() == u"jpeg") {
            continue;
        }
        fields << it.key() + QLatin1Char('=') + flat(it.value());
    }
    std::printf("%s\n", fields.join(QLatin1Char('\t')).toUtf8().constData());
    std::fflush(stdout);
}

int usage()
{
    std::fprintf(stderr,
                 "usage: face-unlock-ctl [--socket PATH] COMMAND\n"
                 "  hello | status | cameras | list | watch | unlocked\n"
                 "  verify [USER] [PURPOSE] | test\n"
                 "  remove ID | rename ID NAME | enable ID | disable ID | clear\n");
    return 2;
}
} // namespace

int main(int argc, char **argv)
{
    QCoreApplication app(argc, argv);
    QStringList args = app.arguments().mid(1);

    QString socketPath = QStringLiteral(FU_SOCKET);
    if (args.size() >= 2 && args.first() == u"--socket") {
        socketPath = args.at(1);
        args = args.mid(2);
    }
    if (const QByteArray env = qgetenv("FU_SOCKET"); !env.isEmpty()) {
        socketPath = QString::fromLocal8Bit(env);
    }
    if (args.isEmpty()) {
        return usage();
    }

    const QString cmd = args.takeFirst();
    QJsonObject request{{QStringLiteral("cmd"), cmd}};
    if (cmd == u"verify") {
        if (!args.isEmpty()) {
            request.insert(QStringLiteral("user"), args.takeFirst());
        }
        request.insert(QStringLiteral("purpose"), args.isEmpty() ? QStringLiteral("other") : args.takeFirst());
    } else if (cmd == u"test") {
        request = {{QStringLiteral("cmd"), QStringLiteral("verify")}, {QStringLiteral("purpose"), QStringLiteral("test")}, {QStringLiteral("verbose"), true}};
    } else if (cmd == u"remove" || cmd == u"enable" || cmd == u"disable") {
        if (args.isEmpty()) {
            return usage();
        }
        request.insert(QStringLiteral("id"), args.takeFirst());
    } else if (cmd == u"rename") {
        if (args.size() < 2) {
            return usage();
        }
        request.insert(QStringLiteral("id"), args.takeFirst());
        request.insert(QStringLiteral("name"), args.join(QLatin1Char(' ')));
    } else if (!QStringList{QStringLiteral("hello"), QStringLiteral("status"), QStringLiteral("cameras"), QStringLiteral("list"), QStringLiteral("watch"),
                            QStringLiteral("unlocked"), QStringLiteral("clear")}
                    .contains(cmd)) {
        return usage();
    }

    QLocalSocket socket;
    socket.connectToServer(socketPath);
    if (!socket.waitForConnected(5000)) {
        std::printf("event=result\tok=false\treason=unreachable\tmessage=%s\n", qPrintable(socket.errorString()));
        return 2;
    }
    socket.write(QJsonDocument(request).toJson(QJsonDocument::Compact) + '\n');
    socket.flush();

    QByteArray buffer;
    // Changing faces can wait on somebody typing their password into the
    // polkit dialog, and "watch" waits forever.
    while (socket.state() == QLocalSocket::ConnectedState || socket.bytesAvailable() > 0) {
        if (socket.bytesAvailable() == 0 && !socket.waitForReadyRead(-1)) {
            break;
        }
        buffer += socket.readAll();
        qsizetype nl;
        while ((nl = buffer.indexOf('\n')) >= 0) {
            const QJsonObject o = QJsonDocument::fromJson(buffer.left(nl)).object();
            buffer.remove(0, nl + 1);
            const QString event = o.value(u"event").toString();

            if (event == u"result") {
                for (const QJsonValue &f : o.value(u"faces").toArray()) {
                    printObject(f.toObject(), QStringLiteral("item"));
                }
                for (const QJsonValue &c : o.value(u"cameras").toArray()) {
                    printObject(c.toObject(), QStringLiteral("item"));
                }
                printObject(o, event);
                return o.value(u"ok").toBool() ? 0 : 1;
            }
            printObject(o, event);
        }
    }
    std::printf("event=result\tok=false\treason=disconnected\n");
    return 1;
}
