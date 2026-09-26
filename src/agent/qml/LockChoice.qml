// SPDX-License-Identifier: GPL-3.0-or-later
//
// Where the lock screen is a program of the user's (Hyprland, Niri): lock
// with face-unlock's own, with the bubble, or keep that program, which then
// shows a line of text (hyprlock). Each choice shows the user's screen as it
// would look: face-unlock's lock screen live, and their own rebuilt from its
// config (see LockPreview). The pick goes back to the menu, which carries
// it out.

import QtQuick
import QtQuick.Effects
import QtQuick.Window
import FaceUnlock

Window {
    id: window

    // "own", "yours", or empty when there is none yet.
    required property string current
    // The user's lock screen (see LockPreview::locker) and what it shows.
    required property string locker
    required property var widgets
    // The screen the pictures show, and the parts of face-unlock's own.
    required property size screenSize
    required property url ownWallpaper
    required property var lock
    required property var bubble
    // What was picked, empty when the window was closed.
    property string answer: ""
    property string selected: current

    function pick() {
        if (selected.length > 0) {
            answer = selected
            close()
        }
    }

    // One fixed size, so tiling compositors float it (see Enroll). The
    // height follows the texts, whose length depends on the language. It
    // is taken from them rather than from the Row and Column, which only lay
    // out once the window shows, and the agent shows it after that.
    readonly property int fitHeight: Math.ceil(36 + heading.height + 10 + intro.height + 28 + Math.max(own.needed, yours.needed) + 94)
    width: 800
    height: fitHeight
    minimumWidth: 800
    maximumWidth: 800
    minimumHeight: fitHeight
    maximumHeight: fitHeight
    visible: false
    color: Theme.panel
    title: i18n("Choose your lock screen")

    Component {
        id: ownScene
        LockScreen {
            lock: window.lock
            bubble: window.bubble
            primary: true
            wallpaper: window.ownWallpaper
            interactive: false
        }
    }
    Component {
        id: yoursScene
        LockPreview {
            widgets: window.widgets
        }
    }

    // One of the two: the screen, a radio button with the name, what it means.
    component Choice: Rectangle {
        id: card

        required property string style
        required property Component scene
        required property string name
        required property string about
        readonly property bool chosen: window.selected === style
        // The height its text needs; both take the larger one.
        readonly property real needed: body.y + body.height + 20

        width: 364
        radius: 18
        color: area.containsMouse && !chosen ? Qt.lighter(Theme.surface, 1.25) : Theme.surface
        border.width: 2
        border.color: chosen ? Theme.accent : "transparent"
        Behavior on border.color { ColorAnimation { duration: 150 } }
        Behavior on color { ColorAnimation { duration: 120 } }

        // The whole screen, made small: filling the picture, cut at the
        // sides or at the top and bottom.
        Item {
            id: picture
            x: 14
            y: 14
            width: parent.width - 28
            height: Math.round(width * 10 / 16)
            clip: true
            layer.enabled: true
            layer.effect: MultiEffect {
                maskEnabled: true
                maskSource: mask
            }

            Loader {
                sourceComponent: card.scene
                width: window.screenSize.width
                height: window.screenSize.height
                transformOrigin: Item.TopLeft
                scale: Math.max(picture.width / width, picture.height / height)
                x: (picture.width - width * scale) / 2
                y: (picture.height - height * scale) / 2
            }
        }
        Rectangle {
            id: mask
            width: picture.width
            height: picture.height
            radius: 10
            visible: false
            layer.enabled: true
        }

        // A radio button: a ring, filled when picked.
        Rectangle {
            id: radio
            x: 18
            y: title.y + 3
            width: 18
            height: 18
            radius: 9
            color: "transparent"
            border.width: 2
            border.color: card.chosen ? Theme.accent : Theme.textSecondary
            Rectangle {
                anchors.centerIn: parent
                width: 8
                height: 8
                radius: 4
                color: Theme.accent
                visible: card.chosen
            }
        }
        Text {
            id: title
            anchors.top: picture.bottom
            anchors.topMargin: 16
            x: radio.x + radio.width + 10
            width: parent.width - x - 18
            wrapMode: Text.WordWrap
            color: Theme.textPrimary
            font.pixelSize: 16
            font.weight: Font.DemiBold
            text: card.name
        }
        Text {
            id: body
            anchors.top: title.bottom
            anchors.topMargin: 8
            x: 18
            width: parent.width - 36
            wrapMode: Text.WordWrap
            color: Theme.textDetail
            font.pixelSize: 13
            lineHeight: 1.15
            text: card.about
        }

        MouseArea {
            id: area
            anchors.fill: parent
            hoverEnabled: true
            cursorShape: Qt.PointingHandCursor
            onClicked: window.selected = card.style
            onDoubleClicked: {
                window.selected = card.style
                window.pick()
            }
        }
    }

    Column {
        id: head
        anchors.top: parent.top
        anchors.topMargin: 36
        anchors.horizontalCenter: parent.horizontalCenter
        width: parent.width - 96
        spacing: 10

        Text {
            id: heading
            width: parent.width
            horizontalAlignment: Text.AlignHCenter
            color: Theme.textPrimary
            font.pixelSize: 26
            font.weight: Font.Bold
            text: i18n("Your lock screen")
        }
        Text {
            id: intro
            width: parent.width
            horizontalAlignment: Text.AlignHCenter
            wrapMode: Text.WordWrap
            color: Theme.textSecondary
            font.pixelSize: 13
            font.weight: Font.Medium
            lineHeight: 1.15
            text: i18n("Your desktop leaves the lock screen to a program of your choice. Pick how face unlock shows up there. You can change it later under Settings.")
        }
    }

    Row {
        id: cards
        anchors.top: head.bottom
        anchors.topMargin: 28
        anchors.horizontalCenter: parent.horizontalCenter
        spacing: 24
        focus: true

        Keys.onLeftPressed: window.selected = "own"
        Keys.onRightPressed: window.selected = "yours"
        Keys.onReturnPressed: window.pick()
        Keys.onEnterPressed: window.pick()
        Keys.onEscapePressed: window.close()

        Choice {
            id: own
            height: Math.max(own.needed, yours.needed)
            style: "own"
            scene: ownScene
            name: i18n("face-unlock's lock screen")
            about: i18n("The bubble at the top, the time and your wallpaper. Your shortcut and idle lock then run face-unlock lock.")
        }
        Choice {
            id: yours
            height: own.height
            style: "yours"
            scene: yoursScene
            name: i18n("Keep your lock screen")
            about: window.locker === "hyprlock"
                   ? i18n("It stays as it is. hyprlock shows a line of text at the top instead of the bubble. Enter on the empty password field scans.")
                   : i18n("It stays as it is, without the bubble. Enter on the empty password field scans.")
        }
    }

    Row {
        anchors.bottom: parent.bottom
        anchors.bottomMargin: 32
        anchors.horizontalCenter: parent.horizontalCenter
        spacing: 12

        PillButton {
            primary: false
            text: i18n("Cancel")
            onClicked: window.close()
        }
        PillButton {
            enabled: window.selected.length > 0
            text: i18n("Continue")
            onClicked: window.pick()
        }
    }
}
