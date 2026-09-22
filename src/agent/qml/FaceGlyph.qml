// SPDX-License-Identifier: GPL-3.0-or-later
//
// The face: four corner brackets around two eyes, a nose and a smile, the
// shape everybody knows from a phone. Drawn with shapes rather than played
// from a video, so it is sharp at any size and every part can move on its own.
//
//   idle      still
//   scanning  the face looks around inside the brackets, the brackets breathe
//   tracking  a face is in view: it looks straight ahead, brackets close in
//   success   the face gives way to a tick, the brackets turn green
//   failure   it shakes its head and the smile goes flat
//   lockout   a lock instead of a face

import QtQuick
import QtQuick.Shapes

Item {
    id: root

    property string mode: "idle"
    property color color: Theme.textPrimary
    property real lineWidth: width * 0.055

    readonly property real u: width / 100
    readonly property bool looking: mode === "scanning"
    readonly property color bracketColor: mode === "success" ? Theme.success
                                        : mode === "failure" ? Theme.failure
                                        : root.color

    // -- where the face looks, while it looks around
    property real lookAngle: 0
    NumberAnimation on lookAngle {
        running: root.looking
        from: 0
        to: 2 * Math.PI
        duration: 2400
        loops: Animation.Infinite
    }
    property real lookAmount: root.looking ? 1 : 0
    Behavior on lookAmount { NumberAnimation { duration: 300; easing.type: Easing.InOutQuad } }
    readonly property real lookX: 3.5 * u * Math.cos(lookAngle) * lookAmount
    readonly property real lookY: 2.2 * u * Math.sin(lookAngle) * lookAmount

    // -- the smile, flat on failure
    property real smile: mode === "failure" ? 0 : 1
    Behavior on smile { NumberAnimation { duration: 220; easing.type: Easing.OutQuad } }

    // -- the brackets breathe while scanning and close in on a face
    property real breathe: 0
    SequentialAnimation on breathe {
        running: root.looking
        loops: Animation.Infinite
        NumberAnimation { from: 0; to: 1; duration: 700; easing.type: Easing.InOutSine }
        NumberAnimation { from: 1; to: 0; duration: 700; easing.type: Easing.InOutSine }
        onRunningChanged: if (!running) root.breathe = 0
    }
    readonly property real bracketScale: mode === "tracking" ? 0.9
                                       : mode === "success" ? 1.04
                                       : 1 - 0.04 * breathe

    // -- a shake of the head
    property real shake: 0
    onModeChanged: if (mode === "failure") shakeAnimation.restart()
    SequentialAnimation {
        id: shakeAnimation
        NumberAnimation { target: root; property: "shake"; to: -9; duration: 55; easing.type: Easing.OutQuad }
        NumberAnimation { target: root; property: "shake"; to: 8; duration: 70; easing.type: Easing.InOutQuad }
        NumberAnimation { target: root; property: "shake"; to: -6; duration: 65; easing.type: Easing.InOutQuad }
        NumberAnimation { target: root; property: "shake"; to: 4; duration: 60; easing.type: Easing.InOutQuad }
        NumberAnimation { target: root; property: "shake"; to: -2; duration: 55; easing.type: Easing.InOutQuad }
        NumberAnimation { target: root; property: "shake"; to: 0; duration: 50; easing.type: Easing.OutQuad }
    }

    Item {
        id: glyph
        anchors.fill: parent
        transform: Translate { x: root.shake * root.u }

        // Brackets
        Shape {
            id: brackets
            anchors.fill: parent
            preferredRendererType: Shape.CurveRenderer
            scale: root.bracketScale
            Behavior on scale { NumberAnimation { duration: 260; easing.type: Easing.OutBack } }

            component Bracket: ShapePath {
                strokeColor: root.bracketColor
                strokeWidth: root.lineWidth
                fillColor: "transparent"
                capStyle: ShapePath.RoundCap
                joinStyle: ShapePath.RoundJoin
                Behavior on strokeColor { ColorAnimation { duration: 220 } }
            }

            Bracket {
                startX: 6 * root.u; startY: 30 * root.u
                PathLine { x: 6 * root.u; y: 19 * root.u }
                PathQuad { x: 19 * root.u; y: 6 * root.u; controlX: 6 * root.u; controlY: 6 * root.u }
                PathLine { x: 30 * root.u; y: 6 * root.u }
            }
            Bracket {
                startX: 70 * root.u; startY: 6 * root.u
                PathLine { x: 81 * root.u; y: 6 * root.u }
                PathQuad { x: 94 * root.u; y: 19 * root.u; controlX: 94 * root.u; controlY: 6 * root.u }
                PathLine { x: 94 * root.u; y: 30 * root.u }
            }
            Bracket {
                startX: 94 * root.u; startY: 70 * root.u
                PathLine { x: 94 * root.u; y: 81 * root.u }
                PathQuad { x: 81 * root.u; y: 94 * root.u; controlX: 94 * root.u; controlY: 94 * root.u }
                PathLine { x: 70 * root.u; y: 94 * root.u }
            }
            Bracket {
                startX: 30 * root.u; startY: 94 * root.u
                PathLine { x: 19 * root.u; y: 94 * root.u }
                PathQuad { x: 6 * root.u; y: 81 * root.u; controlX: 6 * root.u; controlY: 94 * root.u }
                PathLine { x: 6 * root.u; y: 70 * root.u }
            }
        }

        // The face itself
        Shape {
            id: face
            anchors.fill: parent
            preferredRendererType: Shape.CurveRenderer
            opacity: root.mode === "success" || root.mode === "lockout" ? 0 : 1
            scale: root.mode === "success" ? 0.6 : 1
            Behavior on opacity { NumberAnimation { duration: 180 } }
            Behavior on scale { NumberAnimation { duration: 220; easing.type: Easing.InQuad } }
            transform: Translate { x: root.lookX; y: root.lookY }

            component Feature: ShapePath {
                strokeColor: root.mode === "failure" ? Theme.failure : root.color
                strokeWidth: root.lineWidth
                fillColor: "transparent"
                capStyle: ShapePath.RoundCap
                joinStyle: ShapePath.RoundJoin
                Behavior on strokeColor { ColorAnimation { duration: 220 } }
            }

            // Eyes
            Feature {
                startX: 34 * root.u; startY: 36 * root.u
                PathLine { x: 34 * root.u; y: 45 * root.u }
            }
            Feature {
                startX: 66 * root.u; startY: 36 * root.u
                PathLine { x: 66 * root.u; y: 45 * root.u }
            }
            // Nose, with its little hook
            Feature {
                startX: 50 * root.u; startY: 37 * root.u
                PathLine { x: 50 * root.u; y: 56 * root.u }
                PathQuad { x: 45 * root.u; y: 61 * root.u; controlX: 50 * root.u; controlY: 61 * root.u }
            }
            // Mouth
            Feature {
                startX: 35 * root.u; startY: 70 * root.u
                PathQuad { x: 65 * root.u; y: 70 * root.u; controlX: 50 * root.u; controlY: (70 + 11 * root.smile) * root.u }
            }
        }

        Checkmark {
            anchors.fill: parent
            color: Theme.success
            lineWidth: root.lineWidth * 1.15
            progress: root.mode === "success" ? 1 : 0
            Behavior on progress { NumberAnimation { duration: 380; easing.type: Easing.OutCubic } }
        }

        LockGlyph {
            anchors.centerIn: parent
            width: parent.width * 0.42
            height: width
            color: root.color
            opacity: root.mode === "lockout" ? 1 : 0
            scale: root.mode === "lockout" ? 1 : 0.7
            Behavior on opacity { NumberAnimation { duration: 200 } }
            Behavior on scale { NumberAnimation { duration: 260; easing.type: Easing.OutBack } }
        }
    }
}
