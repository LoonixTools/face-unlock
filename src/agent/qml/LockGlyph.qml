// SPDX-License-Identifier: GPL-3.0-or-later
//
// A padlock whose shackle lifts and swings open.

import QtQuick
import QtQuick.Shapes

Item {
    id: root

    property color color: Theme.textPrimary
    property bool open: false

    readonly property real u: width / 100

    // Body
    Rectangle {
        x: 14 * root.u
        y: 46 * root.u
        width: 72 * root.u
        height: 50 * root.u
        radius: 12 * root.u
        color: root.color
    }

    // Shackle: a U standing on the body. Opening lifts it and swings it about
    // its right leg.
    Item {
        id: shackle
        x: 26 * root.u
        y: 4 * root.u
        width: 48 * root.u
        height: 52 * root.u
        transformOrigin: Item.BottomRight

        property real lift: root.open ? 7 * root.u : 0
        Behavior on lift { NumberAnimation { duration: 260; easing.type: Easing.OutBack } }
        rotation: root.open ? -28 : 0
        Behavior on rotation { NumberAnimation { duration: 320; easing.type: Easing.OutBack } }
        transform: Translate { y: -shackle.lift }

        Shape {
            anchors.fill: parent
            preferredRendererType: Shape.CurveRenderer
            ShapePath {
                strokeColor: root.color
                strokeWidth: 11 * root.u
                fillColor: "transparent"
                capStyle: ShapePath.FlatCap
                startX: 5.5 * root.u
                startY: shackle.height
                PathLine { x: 5.5 * root.u; y: 24 * root.u }
                PathArc {
                    x: shackle.width - 5.5 * root.u
                    y: 24 * root.u
                    radiusX: (shackle.width - 11 * root.u) / 2
                    radiusY: radiusX
                }
                PathLine { x: shackle.width - 5.5 * root.u; y: shackle.height }
            }
        }
    }
}
