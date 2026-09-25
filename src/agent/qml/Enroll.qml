// SPDX-License-Identifier: GPL-3.0-or-later
//
// Setting up a face: a short introduction, the password (polkit's own
// dialog), then the camera in a circle with the ring of ticks around it, and
// a tick at the end. Laid out like the phone's setup and Glance's onboarding.

import QtQuick
import QtQuick.Window
import FaceUnlock

Window {
    id: window

    required property var controller

    width: 520
    height: 680
    minimumWidth: 460
    minimumHeight: 620
    visible: true
    color: Theme.panel
    title: i18n("Set up Face Unlock")

    readonly property string step: controller.state
    readonly property bool scanning: step === "center" || step === "circle"
    readonly property bool done: step === "done"

    onClosing: controller.cancel()

    Item {
        id: stage
        anchors.horizontalCenter: parent.horizontalCenter
        y: 40
        width: 360
        height: 360

        // Before the camera, and when something went wrong: the face, alive.
        FaceGlyph {
            anchors.centerIn: parent
            width: 150
            height: 150
            mode: window.step === "failed" ? "failure" : "scanning"
            opacity: window.scanning || window.done ? 0 : 1
            scale: window.scanning || window.done ? 0.8 : 1
            Behavior on opacity { NumberAnimation { duration: 250 } }
            Behavior on scale { NumberAnimation { duration: 300; easing.type: Easing.OutCubic } }
        }

        // The camera, the ring, and the tick at the end.
        Item {
            anchors.fill: parent
            opacity: window.scanning || window.done ? 1 : 0
            scale: window.scanning || window.done ? 1 : 0.9
            Behavior on opacity { NumberAnimation { duration: 300 } }
            Behavior on scale { NumberAnimation { duration: 350; easing.type: Easing.OutCubic } }

            PreviewItem {
                id: preview
                anchors.centerIn: parent
                width: 264
                height: 264
                controller: window.controller
                opacity: window.done ? 0.25 : 1
                Behavior on opacity { NumberAnimation { duration: 400 } }
            }

            TickRing {
                anchors.fill: parent
                sectors: window.controller.sectors
                pose: window.controller.pose
                tracking: window.step === "circle" && window.controller.faceVisible
                complete: window.done
            }

            Checkmark {
                anchors.centerIn: parent
                width: 150
                height: 150
                color: Theme.accent
                progress: window.done ? 1 : 0
                Behavior on progress { SequentialAnimation {
                    PauseAnimation { duration: 350 }
                    NumberAnimation { duration: 450; easing.type: Easing.OutCubic }
                } }
            }
        }
    }

    Column {
        id: texts
        anchors.top: stage.bottom
        anchors.topMargin: 26
        anchors.horizontalCenter: parent.horizontalCenter
        width: parent.width - 80
        spacing: 12

        Text {
            width: parent.width
            horizontalAlignment: Text.AlignHCenter
            wrapMode: Text.WordWrap
            color: Theme.textPrimary
            font.pixelSize: 26
            font.weight: Font.Bold
            visible: text.length > 0
            text: window.step === "intro" ? i18n("Face Unlock")
                : window.step === "authorizing" ? i18n("Confirm it is you")
                : window.done ? i18n("You are all set")
                : window.step === "failed" ? i18n("Setup did not finish")
                : ""
        }

        Text {
            width: parent.width
            horizontalAlignment: Text.AlignHCenter
            wrapMode: Text.WordWrap
            color: window.scanning ? Theme.textPrimary : Theme.textSecondary
            font.pixelSize: window.scanning ? 15 : 13
            font.weight: Font.Medium
            lineHeight: 1.15
            text: window.step === "intro"
                  ? i18n("Look at the camera, then move your head slowly in a circle. Your face is turned into numbers on this computer and never stored as a picture.")
                  : window.done
                  ? i18n("Lock the screen and look at it to try it out. You can add a second look, with glasses for example, from the menu.")
                  : window.controller.instruction
        }

        // A name, so more than one face can be told apart in the menu.
        Rectangle {
            visible: window.step === "intro"
            anchors.horizontalCenter: parent.horizontalCenter
            width: 260
            height: 38
            radius: 10
            color: Theme.surfaceRaised

            TextInput {
                id: nameInput
                anchors.fill: parent
                anchors.leftMargin: 14
                anchors.rightMargin: 14
                verticalAlignment: TextInput.AlignVCenter
                color: Theme.textPrimary
                selectionColor: Theme.accent
                font.pixelSize: 13
                clip: true
                maximumLength: 64
                focus: true
                text: window.controller.name
                onTextEdited: window.controller.name = text
                onAccepted: window.controller.start()
            }
            Text {
                anchors.fill: nameInput
                verticalAlignment: Text.AlignVCenter
                color: Theme.textSecondary
                font.pixelSize: 13
                text: i18n("Name (optional)")
                visible: nameInput.text.length === 0
            }
        }
    }

    Row {
        anchors.bottom: parent.bottom
        anchors.bottomMargin: 32
        anchors.horizontalCenter: parent.horizontalCenter
        spacing: 12

        PillButton {
            visible: window.step !== "done"
            primary: false
            text: i18n("Cancel")
            onClicked: window.controller.cancel()
        }
        PillButton {
            visible: window.controller.canFinish
            primary: false
            text: i18n("Finish now")
            onClicked: window.controller.finish()
        }
        PillButton {
            visible: window.step === "intro" || window.step === "failed"
            text: window.step === "failed" ? i18n("Try again") : i18n("Get started")
            onClicked: window.controller.start()
        }
        PillButton {
            visible: window.done
            text: i18n("Done")
            onClicked: window.controller.cancel()
        }
    }
}
