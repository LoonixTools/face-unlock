// SPDX-License-Identifier: GPL-3.0-or-later
//
// A padlock that opens the way a real one does: the shackle jumps up, its
// left leg coming out of the body, and swings round its right leg to the
// other side.

import QtQuick
import QtQuick.Shapes

Item {
    id: root

    property color color: Theme.textPrimary
    property bool open: false
    property real pace: 1

    readonly property real u: width / 100

    // 0 shut, 1 open. Everything below follows from it. Only opening is
    // animated: it is shut again for the next scan, and the screen was
    // locked all along, so showing it lock would make no sense.
    property real openness: 0
    onOpenChanged: {
        if (open) {
            opening.restart()
        } else {
            opening.stop()
            openness = 0
        }
    }
    NumberAnimation {
        id: opening
        target: root
        property: "openness"
        to: 1
        duration: 560 * root.pace
    }

    function smooth(from, to, p) {
        const x = Math.max(0, Math.min(1, (p - from) / (to - from)))
        return x * x * (3 - 2 * x)
    }
    // Up first, then round, a little past the other side and back.
    readonly property real lift: 12 * smooth(0, 0.3, openness)
    readonly property real swing: {
        const p = Math.max(0, Math.min(1, (openness - 0.22) / 0.78))
        const s = 1.4
        return 180 * (1 + (s + 1) * Math.pow(p - 1, 3) + s * Math.pow(p - 1, 2))
    }
    // A small beat as it comes open.
    scale: 1 + 0.14 * Math.sin(Math.PI * smooth(0.55, 1, openness))

    // The shackle as points: up the left leg, over the top, down the right
    // leg, which stays in the body. Swung about that leg, everything else
    // narrows towards it and comes out on the other side.
    function shackle() {
        const right = 68.5, radius = 18.5
        const top = 28 - lift
        const narrow = Math.cos(swing * Math.PI / 180)
        const points = []
        const add = (x, y) => points.push(Qt.point((right + (x - right) * narrow) * u, y * u))
        add(31.5, 56 - 18 * smooth(0, 0.3, openness))
        add(31.5, top)
        for (let i = 1; i < 24; ++i) {
            const a = Math.PI + Math.PI * i / 24
            add(50 + radius * Math.cos(a), top + radius * Math.sin(a))
        }
        add(right, top)
        add(right, 56)
        return points
    }

    // Body
    Rectangle {
        x: 14 * root.u
        y: 46 * root.u
        width: 72 * root.u
        height: 50 * root.u
        radius: 12 * root.u
        color: root.color
    }

    Shape {
        anchors.fill: parent
        preferredRendererType: Shape.CurveRenderer

        ShapePath {
            strokeColor: root.color
            strokeWidth: 11 * root.u
            fillColor: "transparent"
            capStyle: ShapePath.RoundCap
            joinStyle: ShapePath.RoundJoin
            PathPolyline { path: root.shackle() }
        }
    }
}
