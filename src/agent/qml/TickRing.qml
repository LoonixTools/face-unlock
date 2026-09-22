// SPDX-License-Identifier: GPL-3.0-or-later
//
// The ring of ticks around the camera picture during setup, as on the phone.
// 80 ticks in eight sectors; a sector lights up once the head has pointed
// that way long enough. A few ticks also follow where the head points right
// now, so it is clear which way is still missing. When everything is done the
// ticks give way to one closed ring.

import QtQuick

Item {
    id: root

    // Eight booleans, clockwise from the top.
    property var sectors: [false, false, false, false, false, false, false, false]
    // Where the head points, x to the right and y up, 1.0 a comfortable turn.
    property point pose: Qt.point(0, 0)
    property bool tracking: false
    property bool complete: false
    // Grows the centre ticks for a moment when the straight-ahead samples
    // have been taken.
    property bool centreDone: false

    readonly property int tickCount: 80
    readonly property real innerRadius: width / 2 - 22
    readonly property real headAngle: {
        const a = Math.atan2(pose.x, pose.y) * 180 / Math.PI
        return a < 0 ? a + 360 : a
    }
    readonly property real headReach: Math.min(1, Math.sqrt(pose.x * pose.x + pose.y * pose.y))

    function lit(index) {
        if (complete) {
            return true
        }
        const sector = Math.round(index * 360 / tickCount / 45) % 8
        return sectors[sector] === true
    }

    // How much a tick lights up for the head pointing its way.
    function intensity(index) {
        if (!tracking || complete || lit(index)) {
            return 0
        }
        let delta = Math.abs(index * 360 / tickCount - headAngle)
        if (delta > 180) {
            delta = 360 - delta
        }
        const halfSpan = 6 * 360 / tickCount
        return headReach * Math.max(0, 1 - delta / halfSpan)
    }

    Repeater {
        model: root.tickCount

        Item {
            id: tick
            required property int index
            readonly property bool isLit: root.lit(index)
            readonly property real glow: root.intensity(index)

            width: root.width
            height: root.height
            rotation: index * 360 / root.tickCount

            Rectangle {
                width: 3
                // Grows outwards: the inner tip stays on the ring.
                height: tick.isLit ? 18 : 10 + 8 * tick.glow
                radius: 1.5
                x: (parent.width - width) / 2
                y: parent.height / 2 - root.innerRadius - height
                antialiasing: true
                color: tick.isLit ? Theme.accent : Qt.rgba(1 - 0.8 * tick.glow * (1 - Theme.accent.r) , 1 - 0.8 * tick.glow * (1 - Theme.accent.g), 1 - 0.8 * tick.glow * (1 - Theme.accent.b), 0.28 + 0.72 * tick.glow)
                opacity: root.complete ? 0 : 1

                Behavior on height { NumberAnimation { duration: 180; easing.type: Easing.OutCubic } }
                Behavior on color { ColorAnimation { duration: 250 } }
                Behavior on opacity { SequentialAnimation {
                    PauseAnimation { duration: tick.index * 4 }
                    NumberAnimation { duration: 400; easing.type: Easing.InOutQuad }
                } }
            }
        }
    }

    // The closed ring that replaces the ticks at the end.
    Rectangle {
        anchors.centerIn: parent
        width: 2 * root.innerRadius + 26
        height: width
        radius: width / 2
        color: "transparent"
        border.color: Theme.accent
        border.width: 5
        opacity: root.complete ? 1 : 0
        scale: root.complete ? 1 : 0.92
        Behavior on opacity { NumberAnimation { duration: 450; easing.type: Easing.InOutQuad } }
        Behavior on scale { NumberAnimation { duration: 450; easing.type: Easing.InOutQuad } }
    }
}
