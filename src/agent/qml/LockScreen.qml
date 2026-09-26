// SPDX-License-Identifier: GPL-3.0-or-later
//
// face-unlock's own lock screen, one per screen (see SessionLock): the time,
// the password, and on the main screen the bubble at the top, the same one
// that shows over the desktop. Behind it the desktop's picture, or the one
// the user set (see Wallpaper).

import QtQuick
import QtQuick.Effects

Item {
    id: root

    required property var lock
    required property var bubble
    required property bool primary
    required property url wallpaper

    property date now: new Date()
    Timer {
        interval: 1000
        running: true
        repeat: true
        onTriggered: root.now = new Date()
    }

    Rectangle {
        anchors.fill: parent
        gradient: Gradient {
            GradientStop { position: 0; color: "#11161D" }
            GradientStop { position: 0.6; color: "#000000" }
        }
    }

    // Loaded aside, so a big picture does not hold up the lock, and faded in.
    Item {
        anchors.fill: parent
        opacity: picture.status === Image.Ready ? 1 : 0
        visible: opacity > 0
        Behavior on opacity {
            NumberAnimation { duration: 200 }
        }

        Image {
            id: picture
            // Past the edges when blurred: the blur would pull in the nothing
            // beyond them and darken the rim.
            anchors.fill: parent
            anchors.margins: root.lock.blur ? -64 : 0
            source: root.wallpaper
            fillMode: Image.PreserveAspectCrop
            sourceSize: Qt.size(root.width, root.height)
            asynchronous: true
            cache: false
            layer.enabled: root.lock.blur
            layer.effect: MultiEffect {
                blurEnabled: true
                blur: 1
                blurMax: 64
            }
        }
        // Dimmed a little, so the time and the password read on any picture.
        Rectangle {
            anchors.fill: parent
            color: "#000000"
            opacity: 0.35
        }
    }

    // Any touch counts as somebody being back, and a click anywhere puts the
    // keys into the password.
    MouseArea {
        anchors.fill: parent
        hoverEnabled: true
        acceptedButtons: Qt.AllButtons
        onPositionChanged: root.lock.activity()
        onPressed: {
            root.lock.activity()
            field.forceActiveFocus()
        }
    }

    // Below the open bubble, which hangs from the top edge.
    Column {
        anchors.horizontalCenter: parent.horizontalCenter
        y: Math.max(Theme.topGap + Theme.openHeight + 24, Math.round(parent.height * 0.2))
        spacing: 2

        Text {
            anchors.horizontalCenter: parent.horizontalCenter
            color: Theme.textPrimary
            font.pixelSize: Math.max(56, Math.min(120, Math.round(root.height * 0.11)))
            font.weight: Font.Light
            text: Qt.formatTime(root.now, Qt.locale().timeFormat(Locale.ShortFormat))
        }
        Text {
            anchors.horizontalCenter: parent.horizontalCenter
            color: Theme.textDetail
            font.pixelSize: 18
            font.weight: Font.Medium
            text: Qt.formatDate(root.now, Qt.locale().dateFormat(Locale.LongFormat))
        }
    }

    Column {
        anchors.horizontalCenter: parent.horizontalCenter
        y: Math.round(parent.height * 0.62)
        spacing: 12

        Text {
            anchors.horizontalCenter: parent.horizontalCenter
            color: Theme.textPrimary
            font.pixelSize: 17
            font.weight: Font.DemiBold
            text: root.lock.userName
        }

        Rectangle {
            id: pill
            anchors.horizontalCenter: parent.horizontalCenter
            width: 280
            height: 44
            radius: height / 2
            color: Theme.surfaceRaised
            opacity: root.lock.busy ? 0.6 : 1
            border.width: field.activeFocus ? 1 : 0
            border.color: Qt.rgba(1, 1, 1, 0.18)

            property real shake: 0
            transform: Translate { x: pill.shake }
            SequentialAnimation {
                id: shakeAnimation
                NumberAnimation { target: pill; property: "shake"; to: -10; duration: 55; easing.type: Easing.OutQuad }
                NumberAnimation { target: pill; property: "shake"; to: 9; duration: 70 }
                NumberAnimation { target: pill; property: "shake"; to: -6; duration: 65 }
                NumberAnimation { target: pill; property: "shake"; to: 4; duration: 60 }
                NumberAnimation { target: pill; property: "shake"; to: 0; duration: 55; easing.type: Easing.OutQuad }
            }

            TextInput {
                id: field
                anchors.fill: parent
                anchors.leftMargin: 20
                anchors.rightMargin: 20
                verticalAlignment: TextInput.AlignVCenter
                horizontalAlignment: TextInput.AlignHCenter
                echoMode: TextInput.Password
                passwordCharacter: "●"
                color: Theme.textPrimary
                selectionColor: Theme.accent
                font.pixelSize: 15
                clip: true
                focus: true
                // The dots show how far it is; a blinking bar in the middle
                // of the empty field only cuts the word in it in two.
                cursorDelegate: Item {}
                readOnly: root.lock.busy
                text: root.lock.password
                onTextEdited: root.lock.password = text
                onAccepted: root.lock.submit()
                Keys.onPressed: event => {
                    root.lock.activity()
                    event.accepted = false
                }
                Component.onCompleted: forceActiveFocus()
            }
            Text {
                anchors.centerIn: parent
                color: Theme.textSecondary
                font.pixelSize: 15
                text: i18n("Password")
                visible: field.text.length === 0
            }
        }

        Text {
            anchors.horizontalCenter: parent.horizontalCenter
            width: 360
            horizontalAlignment: Text.AlignHCenter
            wrapMode: Text.WordWrap
            color: root.lock.message.length > 0 ? Theme.failure : Theme.textDetail
            font.pixelSize: 13
            font.weight: Font.Medium
            text: root.lock.message.length > 0 ? root.lock.message
                : root.lock.faceUnlock ? i18n("Look at the camera or type your password")
                : i18n("Type your password")
        }
    }

    Connections {
        target: root.lock
        function onRejected() {
            shakeAnimation.restart()
        }
    }

    // The bubble, where it shows over the desktop too.
    Bubble {
        visible: root.primary
        anchors.horizontalCenter: parent.horizontalCenter
        y: 0
        bubble: root.bubble
    }
}
