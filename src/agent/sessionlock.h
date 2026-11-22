// SPDX-License-Identifier: GPL-3.0-or-later
//
// face-unlock's own lock screen, for whoever wants it on a compositor whose
// lock screen is a program of its own (ext-session-lock: Hyprland, Niri,
// Sway and others): `face-unlock lock`.
//
// Any other lock screen there covers the bubble, and only opens itself (it
// gets the face through PAM, see src/lib/pam.sh). This one is the lock
// screen, so the bubble shows on it and a face opens it straight away. The
// picture behind it is the user's choice (LockWallpaper). The bubble shows on
// the screen picked for it, or else the one with the mouse pointer.
//
// Qt knows no ext-session-lock, so each screen's window gets the lock surface
// role through Qt's shell integration, the way LayerShellQt gives windows the
// layer-shell role. If this program goes away while locked, the compositor
// keeps the session locked: that is the protocol's promise.

#pragma once

#include <QHash>
#include <QObject>
#include <QPointer>
#include <QUrl>
#include <QtGlobal>

class BubbleController;
class LockScreenController;
class UserConfig;
class QQmlEngine;
class QQuickView;
class QScreen;
class LockIntegration;
class Lock;

class SessionLock : public QObject
{
    Q_OBJECT
public:
    SessionLock(QQmlEngine *engine, BubbleController *bubble, LockScreenController *screen, UserConfig *config, QObject *parent = nullptr);
    ~SessionLock() override;

    // Whether this build has it. Qt before 6.10 commits a window's surface
    // before a lock surface may be committed, which the protocol forbids.
    static constexpr bool built = QT_VERSION >= QT_VERSION_CHECK(6, 10, 0);

    // Whether it can lock here: built, and the compositor offers
    // ext-session-lock.
    static bool available();

    // From the lock request until the lock is gone again.
    bool isLocked() const
    {
        return m_lock != nullptr;
    }
    // The compositor has confirmed the lock: nothing unlocked is on screen.
    bool isConfirmed() const
    {
        return m_confirmed;
    }

    // A file that says the screen is locked, so that an agent started again
    // after a crash locks again (see main.cpp).
    static QString markerPath();

public Q_SLOTS:
    void lock();
    void unlock();

Q_SIGNALS:
    void lockedChanged(bool locked);
    void confirmed();

private:
    friend class Lock;
    void onLocked();
    void onFinished();
    void addScreen(QScreen *screen);
    void removeScreen(QScreen *screen);
    void placeBubble();
    void release();

    QQmlEngine *m_engine;
    BubbleController *m_bubble;
    LockScreenController *m_screen;
    UserConfig *m_config;
    LockIntegration *m_integration = nullptr;
    Lock *m_lock = nullptr;
    bool m_confirmed = false;
    bool m_unlockWanted = false;
    QHash<QScreen *, QPointer<QQuickView>> m_views;
    // The pictures behind it, by output (see Wallpaper).
    QHash<QString, QUrl> m_pictures;
    // The output the mouse pointer was last seen on.
    QString m_pointer;
};
