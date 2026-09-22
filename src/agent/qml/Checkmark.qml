// SPDX-License-Identifier: GPL-3.0-or-later
//
// A tick that draws itself: the short stroke first, then the long one.

import QtQuick
import QtQuick.Shapes

Item {
    id: root

    property color color: Theme.success
    property real lineWidth: width * 0.07
    // 0: nothing, 1: the whole tick.
    property real progress: 0

    readonly property real u: width / 100
    readonly property point a: Qt.point(28 * u, 52 * u)
    readonly property point b: Qt.point(43 * u, 67 * u)
    readonly property point c: Qt.point(73 * u, 35 * u)
    // The short stroke is about a third of the length.
    readonly property real split: 0.33

    function lerp(p, q, t) {
        return Qt.point(p.x + (q.x - p.x) * t, p.y + (q.y - p.y) * t)
    }

    Shape {
        anchors.fill: parent
        visible: root.progress > 0.001
        preferredRendererType: Shape.CurveRenderer

        ShapePath {
            strokeColor: root.color
            strokeWidth: root.lineWidth
            fillColor: "transparent"
            capStyle: ShapePath.RoundCap
            joinStyle: ShapePath.RoundJoin

            PathPolyline {
                path: root.progress <= root.split
                      ? [root.a, root.lerp(root.a, root.b, root.progress / root.split)]
                      : [root.a, root.b, root.lerp(root.b, root.c, (root.progress - root.split) / (1 - root.split))]
            }
        }
    }
}
