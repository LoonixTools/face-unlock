// SPDX-License-Identifier: GPL-3.0-or-later
//
// A video camera with a line through it: another program has the camera, a
// video call most likely. The line strikes through once the camera shows.

import QtQuick
import QtQuick.Shapes

Item {
    id: root

    property color color: Theme.textPrimary
    // What the line cuts out of the camera: the island behind it.
    property color background: Theme.panel
    property bool shown: false
    property real pace: 1

    readonly property real u: width / 100

    // How far the line has come, 0 to 1.
    property real strike: 0
    onShownChanged: {
        if (shown) {
            striking.restart()
        } else {
            striking.stop()
            strike = 0
        }
    }
    SequentialAnimation {
        id: striking
        PropertyAction { target: root; property: "strike"; value: 0 }
        PauseAnimation { duration: 120 * root.pace }
        NumberAnimation { target: root; property: "strike"; to: 1; duration: 320 * root.pace; easing.type: Easing.OutCubic }
    }

    readonly property point lineStart: Qt.point(12 * u, 10 * u)
    readonly property point lineEnd: Qt.point((12 + 76 * strike) * u, (10 + 80 * strike) * u)

    // Body
    Rectangle {
        x: 4 * root.u
        y: 24 * root.u
        width: 62 * root.u
        height: 52 * root.u
        radius: 12 * root.u
        color: root.color
    }

    // Lens, its corners rounded by a stroke of the same colour
    Shape {
        anchors.fill: parent
        preferredRendererType: Shape.CurveRenderer

        ShapePath {
            fillColor: root.color
            strokeColor: root.color
            strokeWidth: 6 * root.u
            joinStyle: ShapePath.RoundJoin
            startX: 72 * root.u; startY: 42 * root.u
            PathLine { x: 94 * root.u; y: 29 * root.u }
            PathLine { x: 94 * root.u; y: 71 * root.u }
            PathLine { x: 72 * root.u; y: 58 * root.u }
            PathLine { x: 72 * root.u; y: 42 * root.u }
        }
    }

    // The line, with a gap round it so it reads on the filled camera
    Shape {
        anchors.fill: parent
        visible: root.strike > 0.01
        preferredRendererType: Shape.CurveRenderer

        ShapePath {
            strokeColor: root.background
            strokeWidth: 24 * root.u
            fillColor: "transparent"
            capStyle: ShapePath.RoundCap
            startX: root.lineStart.x; startY: root.lineStart.y
            PathLine { x: root.lineEnd.x; y: root.lineEnd.y }
        }
        ShapePath {
            strokeColor: root.color
            strokeWidth: 9 * root.u
            fillColor: "transparent"
            capStyle: ShapePath.RoundCap
            startX: root.lineStart.x; startY: root.lineStart.y
            PathLine { x: root.lineEnd.x; y: root.lineEnd.y }
        }
    }
}
