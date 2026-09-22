// SPDX-License-Identifier: GPL-3.0-or-later
//
// The bubble: a black island at the top of the screen that slides down,
// opens up, shows the face while it looks, and closes again.
//
// The choreography is Glance's: entering, the island slides in first and grows
// a moment later; leaving, it shrinks first and slides away after. Growing is
// a spring that overshoots a little, shrinking does not. While it scans the
// content breathes, so it reads as looking rather than stuck.
//
// "full" is the open island with the face in it. "minimal" is a small pill
// with a lock on one side and the face on the other.

import QtQuick
import QtQuick.Effects

Item {
    id: root

    required property var bubble

    width: 420
    height: 300

    readonly property string phase: bubble ? bubble.phase : "hidden"
    readonly property bool minimal: bubble && bubble.style === "minimal"
    readonly property bool wantOpen: phase !== "hidden"

    property bool positioned: false
    property bool expanded: false

    function choreograph() {
        if (wantOpen) {
            slideOut.stop()
            closeDone.stop()
            positioned = true
            if (!expanded) {
                expandLater.restart()
            }
        } else {
            expandLater.stop()
            expanded = false
            slideOut.restart()
        }
    }
    onWantOpenChanged: choreograph()
    Component.onCompleted: choreograph()

    Timer {
        id: expandLater
        interval: Theme.expandDelay
        onTriggered: root.expanded = true
    }
    Timer {
        id: slideOut
        interval: Theme.slideOutDelay
        onTriggered: {
            root.positioned = false
            closeDone.restart()
        }
    }
    Timer {
        id: closeDone
        interval: Theme.slideDuration + 60
        onTriggered: if (root.bubble) root.bubble.closed()
    }

    // What the face shows, from the phase.
    readonly property string glyphMode: phase === "success" ? "success"
                                      : phase === "failure" ? "failure"
                                      : phase === "lockout" ? "lockout"
                                      : phase === "scanning" ? (bubble.faceSeen ? "tracking" : "scanning")
                                      : "idle"

    // -- the breathing while it scans
    property real pulse: 0
    SequentialAnimation on pulse {
        id: pulseAnimation
        running: root.phase === "scanning" && root.expanded
        loops: Animation.Infinite
        PauseAnimation { duration: 600 }
        NumberAnimation { from: 0; to: 1; duration: 400; easing.type: Easing.InOutQuad }
        PauseAnimation { duration: 50 }
        NumberAnimation { from: 1; to: 0; duration: 400; easing.type: Easing.InOutQuad }
        PauseAnimation { duration: 50 }
        onRunningChanged: if (!running) settle.restart()
    }
    NumberAnimation {
        id: settle
        target: root
        property: "pulse"
        to: 0
        duration: 200
        easing.type: Easing.OutQuad
    }

    // -- the minimal pill shakes as a whole; the full island shakes its face
    property real shake: 0
    onPhaseChanged: if (phase === "failure" && minimal) pillShake.restart()
    SequentialAnimation {
        id: pillShake
        NumberAnimation { target: root; property: "shake"; to: -10; duration: 55; easing.type: Easing.OutQuad }
        NumberAnimation { target: root; property: "shake"; to: 9; duration: 70 }
        NumberAnimation { target: root; property: "shake"; to: -6; duration: 65 }
        NumberAnimation { target: root; property: "shake"; to: 4; duration: 60 }
        NumberAnimation { target: root; property: "shake"; to: 0; duration: 55; easing.type: Easing.OutQuad }
    }

    Item {
        id: island

        readonly property real targetWidth: root.expanded ? (root.minimal ? Theme.minimalWidth : Theme.openWidth) : Theme.closedWidth
        readonly property real targetHeight: root.expanded ? (root.minimal ? Theme.minimalHeight : Theme.openHeight) : Theme.closedHeight

        width: targetWidth
        height: targetHeight
        // Growing overshoots a little; shrinking settles without bouncing.
        Behavior on width { SpringAnimation { spring: root.expanded ? 3.4 : 6; damping: root.expanded ? 0.28 : 0.9; epsilon: 0.25 } }
        Behavior on height { SpringAnimation { spring: root.expanded ? 3.4 : 6; damping: root.expanded ? 0.28 : 0.9; epsilon: 0.25 } }

        x: (root.width - width) / 2 + root.shake
        y: root.positioned ? Theme.topGap : -height - 30
        Behavior on y { NumberAnimation { duration: Theme.slideDuration; easing.type: Easing.OutCubic } }

        Rectangle {
            id: shape
            anchors.fill: parent
            color: Theme.panel
            radius: root.expanded && !root.minimal ? Math.min(Theme.openRadius, height / 2) : height / 2

            layer.enabled: true
            layer.effect: MultiEffect {
                shadowEnabled: true
                shadowColor: "#000000"
                shadowOpacity: root.expanded ? 0.35 : 0
                shadowBlur: 0.7
                shadowVerticalOffset: 3
                Behavior on shadowOpacity { NumberAnimation { duration: 200 } }
            }
        }

        // -- full: the face, and a line of text under it
        Item {
            id: full
            anchors.fill: parent
            visible: !root.minimal
            opacity: root.expanded ? 1 - 0.35 * root.pulse : 0
            scale: root.expanded ? 1 - 0.03 * root.pulse : 0.3
            Behavior on opacity { enabled: !pulseAnimation.running; NumberAnimation { duration: 220 } }
            Behavior on scale { enabled: !pulseAnimation.running; NumberAnimation { duration: 260; easing.type: Easing.OutCubic } }

            readonly property bool hasMessage: root.bubble && root.bubble.message.length > 0

            FaceGlyph {
                id: glyph
                width: 100
                height: 100
                anchors.horizontalCenter: parent.horizontalCenter
                y: full.hasMessage ? 28 : 40
                Behavior on y { NumberAnimation { duration: 220; easing.type: Easing.OutCubic } }
                mode: root.glyphMode
            }

            Text {
                anchors.horizontalCenter: parent.horizontalCenter
                anchors.bottom: parent.bottom
                anchors.bottomMargin: 20
                width: parent.width - 32
                horizontalAlignment: Text.AlignHCenter
                elide: Text.ElideRight
                text: root.bubble ? root.bubble.message : ""
                color: root.phase === "failure" || root.phase === "lockout" ? Theme.textDetail : Theme.textSecondary
                font.pixelSize: 13
                font.weight: Font.Medium
                opacity: full.hasMessage ? 1 : 0
                Behavior on opacity { NumberAnimation { duration: 200 } }
            }
        }

        // -- minimal: [ lock ] ... [ face ]
        Item {
            id: small
            anchors.fill: parent
            visible: root.minimal
            opacity: root.expanded ? 1 : 0
            Behavior on opacity { NumberAnimation { duration: 200 } }

            LockGlyph {
                width: 18
                height: 18
                anchors.verticalCenter: parent.verticalCenter
                x: 16
                open: root.phase === "success"
                color: root.phase === "failure" || root.phase === "lockout" ? Theme.failure : Theme.textPrimary
            }

            FaceGlyph {
                width: 24
                height: 24
                anchors.verticalCenter: parent.verticalCenter
                anchors.right: parent.right
                anchors.rightMargin: 14
                lineWidth: 2.4
                mode: root.glyphMode === "lockout" ? "failure" : root.glyphMode
                opacity: 1 - 0.35 * root.pulse
                scale: 1 - 0.03 * root.pulse
            }
        }
    }
}
