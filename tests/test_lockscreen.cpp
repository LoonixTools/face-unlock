// SPDX-License-Identifier: GPL-3.0-or-later
//
// The password on face-unlock's own lock screen, through real PAM but with
// PAM files of its own (FU_PAM_CONFDIR), so no test ever counts as a wrong
// password for the real account.

#include "lockscreencontroller.h"

#include <QCoreApplication>
#include <QDir>
#include <QFile>
#include <QTemporaryDir>
#include <QTimer>

#include <cstdio>

namespace
{
int failures = 0;

void check(bool ok, const char *what)
{
    std::printf("%s  %s\n", ok ? "ok  " : "FAIL", what);
    if (!ok) {
        ++failures;
    }
}

// Types the password into a fresh controller and waits for the verdict:
// 1 let in, 0 refused, -1 no answer.
int attempt(const QString &stack, const QString &password, QString *message = nullptr)
{
    QTemporaryDir dir;
    QFile file(dir.filePath(QStringLiteral("face-unlock-lock")));
    if (!file.open(QIODevice::WriteOnly)) {
        return -1;
    }
    file.write(stack.toUtf8());
    file.close();
    qputenv("FU_PAM_CONFDIR", dir.path().toLocal8Bit());

    LockScreenController lock;
    int verdict = -1;
    QObject::connect(&lock, &LockScreenController::authenticated, [&] {
        verdict = 1;
        QCoreApplication::quit();
    });
    QObject::connect(&lock, &LockScreenController::rejected, [&] {
        verdict = 0;
        QCoreApplication::quit();
    });
    QTimer::singleShot(5000, &QCoreApplication::quit);
    lock.setPassword(password);
    lock.submit();
    QCoreApplication::exec();
    if (message) {
        *message = lock.message();
    }
    check(lock.password().isEmpty(), "the password is gone once it was checked");
    return verdict;
}
} // namespace

int main(int argc, char **argv)
{
    QCoreApplication app(argc, argv);

    check(attempt(QStringLiteral("auth required pam_permit.so\n"), QStringLiteral("secret")) == 1, "a stack that says yes unlocks");

    QString message;
    check(attempt(QStringLiteral("auth required pam_deny.so\n"), QStringLiteral("secret"), &message) == 0, "a stack that says no does not");
    check(!message.isEmpty(), "and the screen says so");

    // Nothing typed: nothing to check, and nothing happens.
    LockScreenController empty;
    bool answered = false;
    QObject::connect(&empty, &LockScreenController::authenticated, [&] {
        answered = true;
    });
    empty.submit();
    check(!empty.busy() && !answered, "an empty password is not sent");

    std::printf("\n%s\n", failures ? "SOME CHECKS FAILED" : "all checks passed");
    return failures ? 1 : 0;
}
