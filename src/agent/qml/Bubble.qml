// SPDX-License-Identifier: GPL-3.0-or-later
//
// The bubble: a black island at the top of the screen that slides down,
// opens up, shows the face while it looks, and closes again.
//
// The choreography is Glance's: entering, the island slides in first and grows
// a moment later; leaving, it shrinks first and slides away as it finishes.
// Growing overshoots a little, shrinking does not, and both end on time rather
// than creeping in. The content grows and shrinks with the island. While it
// scans the content breathes, so it reads as looking rather than stuck.
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
    // The animation speed setting: how long everything takes, 1 normal.
    readonly property real pace: bubble ? bubble.pace : 1

    property bool positioned: false
    property bool expanded: false
    // Which way it is going. Set before the change it is about, so the
    // animations below have the matching curve by the time they start.
    property bool opening: false

    function choreograph() {
        if (wantOpen) {
            opening = true
            slideOut.stop()
            closeDone.stop()
            positioned = true
            if (!expanded) {
                expandLater.restart()
            }
        } else {
            opening = false
            expandLater.stop()
            expanded = false
            slideOut.restart()
        }
    }
    onWantOpenChanged: choreograph()
    Component.onCompleted: choreograph()

    Timer {
        id: expandLater
        interval: Theme.expandDelay * root.pace
        onTriggered: root.expanded = true
    }
    Timer {
        id: slideOut
        interval: Theme.slideOutDelay * root.pace
        onTriggered: {
            root.positioned = false
            closeDone.restart()
        }
    }
    Timer {
        id: closeDone
        interval: Theme.slideOutDuration * root.pace + 60
        onTriggered: if (root.bubble) root.bubble.closed()
    }

    // The phase on screen. Closing keeps the last one, so the tick does not
    // turn back into a face on its way out.
    property string shownPhase: "idle"
    onPhaseChanged: if (phase !== "hidden") shownPhase = phase

    // What the face shows.
    readonly property string glyphMode: shownPhase === "scanning" ? (bubble.faceSeen ? "tracking" : "scanning") : shownPhase

    // -- the breathing while it scans
    property real pulse: 0
    SequentialAnimation on pulse {
        id: pulseAnimation
        running: root.phase === "scanning" && root.expanded
        loops: Animation.Infinite
        PauseAnimation { duration: 600 * root.pace }
        NumberAnimation { from: 0; to: 1; duration: 400 * root.pace; easing.type: Easing.InOutQuad }
        PauseAnimation { duration: 50 * root.pace }
        NumberAnimation { from: 1; to: 0; duration: 400 * root.pace; easing.type: Easing.InOutQuad }
        PauseAnimation { duration: 50 * root.pace }
        onRunningChanged: if (!running) settle.restart()
    }
    NumberAnimation {
        id: settle
        target: root
        property: "pulse"
        to: 0
        duration: 200 * root.pace
        easing.type: Easing.OutQuad
    }

    // -- the minimal pill shakes as a whole; the full island shakes its face
    property real shake: 0
    SequentialAnimation {
        id: pillShake
        NumberAnimation { target: root; property: "shake"; to: -10; duration: 55 * root.pace; easing.type: Easing.OutQuad }
        NumberAnimation { target: root; property: "shake"; to: 9; duration: 70 * root.pace }
        NumberAnimation { target: root; property: "shake"; to: -6; duration: 65 * root.pace }
        NumberAnimation { target: root; property: "shake"; to: 4; duration: 60 * root.pace }
        NumberAnimation { target: root; property: "shake"; to: 0; duration: 55 * root.pace; easing.type: Easing.OutQuad }
    }

    Item {
        id: island

        readonly property real targetWidth: root.expanded ? (root.minimal ? Theme.minimalWidth : Theme.openWidth) : Theme.closedWidth
        readonly property real targetHeight: root.expanded ? (root.minimal ? Theme.minimalHeight : Theme.openHeight) : Theme.closedHeight
        // How far open it is, for the content that grows with it.
        readonly property real fit: Math.min(width / Theme.openWidth, height / Theme.openHeight)

        width: targetWidth
        height: targetHeight
        // Growing overshoots sideways more than down, so the bottom edge does
        // not sag.
        Behavior on width {
            NumberAnimation {
                duration: (root.opening ? Theme.growDuration : Theme.shrinkDuration) * root.pace
                easing.type: root.opening ? Easing.OutBack : Easing.OutCubic
                easing.overshoot: 1.6
            }
        }
        Behavior on height {
            NumberAnimation {
                duration: (root.opening ? Theme.growDuration : Theme.shrinkDuration) * root.pace
                easing.type: root.opening ? Easing.OutBack : Easing.OutCubic
                easing.overshoot: 0.9
            }
        }

        x: (root.width - width) / 2 + root.shake
        // Off screen by a fixed amount. A target that followed the shrinking
        // height would restart the slide every frame and hold it back.
        y: root.positioned ? Theme.topGap : -Theme.closedHeight - 20
        Behavior on y {
            NumberAnimation {
                duration: (root.opening ? Theme.slideInDuration : Theme.slideOutDuration) * root.pace
                easing.type: root.opening ? Easing.OutCubic : Easing.InCubic
            }
        }

        Rectangle {
            id: shape
            anchors.fill: parent
            color: Theme.panel
            // From size alone, so it cannot jump when the island turns.
            radius: root.minimal ? height / 2 : Math.min(Theme.openRadius, height / 2)

            layer.enabled: true
            layer.effect: MultiEffect {
                shadowEnabled: true
                shadowColor: "#000000"
                shadowOpacity: root.expanded ? 0.35 : 0
                shadowBlur: 0.7
                shadowVerticalOffset: 3
                Behavior on shadowOpacity { NumberAnimation { duration: 200 * root.pace } }
            }
        }

        // -- full: the face, and a line of text under it. Laid out at the
        // open size and scaled with the island, so it never pokes out.
        Item {
            id: full
            width: Theme.openWidth
            height: Theme.openHeight
            anchors.centerIn: parent
            visible: !root.minimal
            scale: island.fit
            opacity: root.expanded ? 1 : 0
            Behavior on opacity { NumberAnimation { duration: (root.opening ? 260 : 120) * root.pace } }

            readonly property bool hasMessage: root.bubble && root.bubble.message.length > 0

            Item {
                anchors.fill: parent
                opacity: 1 - 0.35 * root.pulse
                scale: 1 - 0.03 * root.pulse

                FaceGlyph {
                    id: glyph
                    width: 100
                    height: 100
                    anchors.horizontalCenter: parent.horizontalCenter
                    y: full.hasMessage ? 28 : 40
                    Behavior on y { NumberAnimation { duration: 220 * root.pace; easing.type: Easing.OutCubic } }
                    mode: root.glyphMode
                    pace: root.pace
                }

                Text {
                    anchors.horizontalCenter: parent.horizontalCenter
                    anchors.bottom: parent.bottom
                    anchors.bottomMargin: 20
                    width: parent.width - 32
                    horizontalAlignment: Text.AlignHCenter
                    elide: Text.ElideRight
                    text: root.bubble ? root.bubble.message : ""
                    color: root.shownPhase === "failure" || root.shownPhase === "lockout" ? Theme.textDetail : Theme.textSecondary
                    font.pixelSize: 13
                    font.weight: Font.Medium
                    opacity: full.hasMessage ? 1 : 0
                    Behavior on opacity { NumberAnimation { duration: 200 * root.pace } }
                }
            }
        }

        // -- minimal: [ lock ] ... [ face ]
        Item {
            id: small
            anchors.fill: parent
            visible: root.minimal
            opacity: root.expanded ? 1 : 0
            Behavior on opacity { NumberAnimation { duration: (root.opening ? 200 : 120) * root.pace } }

            LockGlyph {
                width: 18
                height: 18
                anchors.verticalCenter: parent.verticalCenter
                x: 16
                open: root.shownPhase === "success"
                pace: root.pace
                color: root.shownPhase === "failure" || root.shownPhase === "lockout" ? Theme.failure : Theme.textPrimary
            }

            FaceGlyph {
                width: 24
                height: 24
                anchors.verticalCenter: parent.verticalCenter
                anchors.right: parent.right
                anchors.rightMargin: 14
                lineWidth: 2.4
                mode: root.glyphMode === "lockout" ? "failure" : root.glyphMode
                pace: root.pace
                onSettled: ok => {
                    if (!ok) {
                        pillShake.restart()
                    }
                }
                opacity: 1 - 0.35 * root.pulse
                scale: 1 - 0.03 * root.pulse
            }
        }
    }
}
