// SPDX-License-Identifier: GPL-3.0-or-later

#include "polkit.h"
#include "system.h"

#include <QDBusArgument>
#include <QDBusConnection>
#include <QDBusMessage>
#include <QDBusMetaType>
#include <QDBusPendingCallWatcher>
#include <QDBusPendingReply>
#include <QLoggingCategory>

namespace
{
// (sa{sv}): the subject, "unix-process" with its pid, start time and uid.
struct Subject {
    QString kind;
    QVariantMap details;
};

// (bba{ss}): authorized, challenge, details.
struct Result {
    bool authorized = false;
    bool challenge = false;
    QMap<QString, QString> details;
};
} // namespace

Q_DECLARE_METATYPE(Subject)
Q_DECLARE_METATYPE(Result)

namespace
{
QDBusArgument &operator<<(QDBusArgument &arg, const Subject &s)
{
    arg.beginStructure();
    arg << s.kind << s.details;
    arg.endStructure();
    return arg;
}

const QDBusArgument &operator>>(const QDBusArgument &arg, Subject &s)
{
    arg.beginStructure();
    arg >> s.kind >> s.details;
    arg.endStructure();
    return arg;
}

QDBusArgument &operator<<(QDBusArgument &arg, const Result &r)
{
    arg.beginStructure();
    arg << r.authorized << r.challenge << r.details;
    arg.endStructure();
    return arg;
}

const QDBusArgument &operator>>(const QDBusArgument &arg, Result &r)
{
    arg.beginStructure();
    arg >> r.authorized >> r.challenge >> r.details;
    arg.endStructure();
    return arg;
}

void registerTypes()
{
    static bool done = false;
    if (!done) {
        qDBusRegisterMetaType<Subject>();
        qDBusRegisterMetaType<Result>();
        qDBusRegisterMetaType<QMap<QString, QString>>();
        done = true;
    }
}

constexpr quint32 AllowUserInteraction = 1;
// Long enough to find the dialog and type a password.
constexpr int TimeoutMs = 5 * 60 * 1000;
} // namespace

namespace Polkit
{
void checkAuthorization(pid_t pid, uid_t uid, const char *action, QObject *context, std::function<void(bool)> done)
{
    registerTypes();

    Subject subject;
    subject.kind = QStringLiteral("unix-process");
    subject.details.insert(QStringLiteral("pid"), QVariant::fromValue(quint32(pid)));
    subject.details.insert(QStringLiteral("start-time"), QVariant::fromValue(quint64(System::processStartTime(pid))));
    subject.details.insert(QStringLiteral("uid"), QVariant::fromValue(qint32(uid)));

    QDBusMessage msg = QDBusMessage::createMethodCall(QStringLiteral("org.freedesktop.PolicyKit1"),
                                                      QStringLiteral("/org/freedesktop/PolicyKit1/Authority"),
                                                      QStringLiteral("org.freedesktop.PolicyKit1.Authority"),
                                                      QStringLiteral("CheckAuthorization"));
    msg << QVariant::fromValue(subject) << QString::fromLatin1(action) << QVariant::fromValue(QMap<QString, QString>())
        << AllowUserInteraction << QString();

    const QDBusPendingCall call = QDBusConnection::systemBus().asyncCall(msg, TimeoutMs);
    auto *watcher = new QDBusPendingCallWatcher(call, context);
    QObject::connect(watcher, &QDBusPendingCallWatcher::finished, context, [watcher, done] {
        watcher->deleteLater();
        const QDBusPendingReply<Result> reply = *watcher;
        if (reply.isError()) {
            qWarning("polkit: %s", qPrintable(reply.error().message()));
            done(false);
            return;
        }
        done(reply.value().authorized);
    });
}
} // namespace Polkit
