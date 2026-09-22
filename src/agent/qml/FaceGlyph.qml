// SPDX-License-Identifier: GPL-3.0-or-later
//
// The face: four corner brackets around two eyes, a nose and a smile, the
// shape everybody knows from a phone. Drawn with shapes rather than played
// from a video, so it is sharp at any size and every part can move on its own.
//
//   idle      still
//   scanning  the face turns its head around inside the brackets, the
//             brackets breathe
//   tracking  a face is in view: it looks straight ahead, brackets close in
//   success   the rings (below), then a tick in the one that is left
//   failure   it turns red and shakes its head, as on iOS
//   lockout   a lock instead of a face
//
// The rings are Face ID's in the Dynamic Island: the face gives way to two
// crossed rings that tumble in 3D with a soft glow, slow down, and land flat
// facing forward; one fades, the other sharpens, and the tick draws inside it.

import QtQuick
import QtQuick.Effects
import QtQuick.Shapes

Item {
    id: root

    property string mode: "idle"
    property color color: Theme.textPrimary
    property real lineWidth: width * 0.055

    // The verdict is showing: after the rings on success, at once on failure.
    signal settled(bool success)

    readonly property real u: width / 100
    readonly property bool looking: mode === "scanning"
    readonly property color bracketColor: mode === "success" ? Theme.success
                                        : mode === "failure" ? Theme.failure
                                        : root.color

    // -- the head turning round while it looks, as Face ID's does. The
    // features sit on a head at different depths (the nose in front, the
    // mouth further back) and turn with it, so the nose swings further than
    // the eyes and the face narrows on the side it turns away from.
    property real lookAngle: 0
    NumberAnimation on lookAngle {
        running: root.looking
        from: 0
        to: 2 * Math.PI
        duration: 2000
        loops: Animation.Infinite
    }
    property real lookAmount: root.looking ? 1 : 0
    Behavior on lookAmount { NumberAnimation { duration: 350; easing.type: Easing.InOutQuad } }
    readonly property real yaw: 0.34 * Math.cos(lookAngle) * lookAmount
    readonly property real pitch: 0.2 * Math.sin(lookAngle) * lookAmount

    // A point of the face, x and y from the middle and z towards the viewer
    // (all in hundredths of the glyph), turned with the head and drawn flat.
    function faceX(x, y, z) {
        return (50 + x * Math.cos(yaw) + z * Math.sin(yaw)) * u
    }
    function faceY(x, y, z) {
        const depth = z * Math.cos(yaw) - x * Math.sin(yaw)
        return (50 + y * Math.cos(pitch) - depth * Math.sin(pitch)) * u
    }

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
    readonly property real bracketScale: mode === "tracking" ? 0.9 : 1 - 0.04 * breathe

    // -- the rings, on one timeline
    // Through the success, 0 to 1.
    property real t: 0
    property real tick: 0

    function smooth(from, to, p) {
        const x = Math.max(0, Math.min(1, (p - from) / (to - from)))
        return x * x * (3 - 2 * x)
    }
    // How far the rings have turned at p: speeding up from rest, fastest a
    // quarter of the way in, then settling softly, facing forward.
    function turned(p) {
        return p * p * (10 - p * (20 - p * (15 - 4 * p)))
    }

    readonly property real faceGone: smooth(0, 0.22, t)
    readonly property real ringsIn: smooth(0.04, 0.26, t)
    readonly property real turn: turned(Math.min(1, t / 0.7))
    readonly property real ringSize: 40 * (0.7 + 0.3 * ringsIn)
    // One ring turns over once and round a quarter, ending flat; the other
    // crosses it, tilted, and fades as they land.
    readonly property var ringA: ring(0, -360 * turn, 90 * turn, ringSize)
    readonly property var ringB: ring(70 + 360 * turn, 30 + 360 * turn, -90 * turn, ringSize)
    // Gone by the time the tick starts, so it draws in a clean ring.
    readonly property real ringBShown: 1 - smooth(0.36, 0.58, t)
    // How dim the far half is: more the more a ring is tilted, none flat.
    readonly property real ringABack: 1 - 0.6 * tilt(ringA)
    readonly property real ringBBack: 1 - 0.6 * tilt(ringB)
    // Sharpens as it settles and on while the tick draws, not in one go.
    readonly property real glow: 1 - smooth(0.42, 0.75, t)

    onModeChanged: {
        succeed.stop()
        drawTick.stop()
        t = 0
        tick = 0
        // From mode itself: bindings on it may not have caught up yet.
        if (mode === "failure") {
            shakeAnimation.restart()
            settled(false)
        } else if (mode === "success") {
            succeed.start()
        }
    }

    SequentialAnimation {
        id: succeed
        ParallelAnimation {
            NumberAnimation { target: root; property: "t"; from: 0; to: 1; duration: 850 }
            // The tick starts as the ring comes to rest, so the one motion
            // runs on into the other.
            SequentialAnimation {
                PauseAnimation { duration: 470 }
                ScriptAction { script: drawTick.start() }
            }
        }
        ScriptAction { script: root.settled(true) }
    }
    NumberAnimation {
        id: drawTick
        target: root
        property: "tick"
        to: 1
        duration: 260
        easing.type: Easing.OutQuad
    }

    // A ring of the given size turned in 3D (degrees about x, then y, then
    // z), as points x and y from the middle and z towards the viewer.
    function ring(ax, ay, az, radius) {
        const d = Math.PI / 180
        const ca = Math.cos(ax * d), sa = Math.sin(ax * d)
        const cb = Math.cos(ay * d), sb = Math.sin(ay * d)
        const cg = Math.cos(az * d), sg = Math.sin(az * d)
        const points = []
        for (let i = 0; i < 64; ++i) {
            const phi = 2 * Math.PI * i / 64
            let x = radius * Math.cos(phi)
            let y = radius * Math.sin(phi) * ca
            let z = radius * Math.sin(phi) * sa
            const x1 = x * cb + z * sb
            z = z * cb - x * sb
            x = x1
            const x2 = x * cg - y * sg
            y = x * sg + y * cg
            x = x2
            points.push([x, y, z])
        }
        return points
    }
    // How far a ring leans out of the screen, 0 flat to 1 edge on.
    function tilt(points) {
        let most = 0
        for (let i = 0; i < points.length; ++i) {
            most = Math.max(most, Math.abs(points[i][2]))
        }
        return points.length ? most / Math.max(1, Math.hypot(points[0][0], points[0][1], points[0][2])) : 0
    }
    function onScreen(p) {
        return Qt.point((50 + p[0]) * u, (50 + p[1]) * u)
    }
    // The half of a ring in front of the middle, or behind it, in one piece
    // and reaching one point into the other half so the two meet.
    function half(points, front) {
        const n = points.length
        const inFront = i => points[(i + n) % n][2] >= -1e-6
        let start = -1
        for (let i = 0; i < n; ++i) {
            if (inFront(i) === front && inFront(i - 1) !== front) {
                start = i
                break
            }
        }
        const out = []
        if (start < 0) {
            if (inFront(0) === front) {
                for (let i = 0; i <= n; ++i) {
                    out.push(onScreen(points[i % n]))
                }
            }
            return out
        }
        out.push(onScreen(points[(start + n - 1) % n]))
        let i = start
        while (inFront(i) === front && i < start + n) {
            out.push(onScreen(points[i % n]))
            ++i
        }
        out.push(onScreen(points[i % n]))
        return out
    }
    function whole(points) {
        const out = points.map(onScreen)
        out.push(out[0])
        return out
    }
    function faded(c, alpha) {
        return Qt.rgba(c.r, c.g, c.b, alpha)
    }

    // -- a shake of the head
    property real shake: 0
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

        // Brackets, giving way to the rings
        Item {
            anchors.fill: parent
            opacity: 1 - root.faceGone
            scale: 1 - 0.15 * root.faceGone

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
        }

        // The rings: a soft glow under them, their far halves dimmed, the near
        // halves on top.
        Item {
            anchors.fill: parent
            visible: root.t > 0
            opacity: root.ringsIn

            Shape {
                anchors.fill: parent
                opacity: 0.8 * root.glow
                layer.enabled: parent.visible && root.glow > 0
                layer.effect: MultiEffect {
                    blurEnabled: true
                    blur: 1
                    blurMax: 20
                }

                ShapePath {
                    strokeColor: root.bracketColor
                    strokeWidth: root.lineWidth * 1.6
                    fillColor: "transparent"
                    PathPolyline { path: root.whole(root.ringA) }
                }
                ShapePath {
                    strokeColor: root.faded(root.bracketColor, root.ringBShown)
                    strokeWidth: root.lineWidth * 1.6
                    fillColor: "transparent"
                    PathPolyline { path: root.whole(root.ringB) }
                }
            }

            Shape {
                anchors.fill: parent
                preferredRendererType: Shape.CurveRenderer

                component RingPath: ShapePath {
                    fillColor: "transparent"
                    strokeWidth: root.lineWidth
                    capStyle: ShapePath.RoundCap
                    joinStyle: ShapePath.RoundJoin
                }

                RingPath {
                    strokeColor: root.faded(root.bracketColor, root.ringABack)
                    PathPolyline { path: root.half(root.ringA, false) }
                }
                RingPath {
                    strokeColor: root.faded(root.bracketColor, root.ringBBack * root.ringBShown)
                    PathPolyline { path: root.half(root.ringB, false) }
                }
                RingPath {
                    strokeColor: root.faded(root.bracketColor, root.ringBShown)
                    PathPolyline { path: root.half(root.ringB, true) }
                }
                RingPath {
                    strokeColor: root.bracketColor
                    PathPolyline { path: root.half(root.ringA, true) }
                }
            }
        }

        // The face itself
        Shape {
            id: face
            anchors.fill: parent
            preferredRendererType: Shape.CurveRenderer
            // Gives way to the rings, and to the lock.
            property real shown: root.mode === "lockout" ? 0 : 1
            Behavior on shown { NumberAnimation { duration: 180 } }
            opacity: shown * (1 - root.faceGone)
            scale: 1 - 0.2 * root.faceGone

            component Feature: ShapePath {
                strokeColor: root.bracketColor
                strokeWidth: root.lineWidth
                fillColor: "transparent"
                capStyle: ShapePath.RoundCap
                joinStyle: ShapePath.RoundJoin
                Behavior on strokeColor { ColorAnimation { duration: 220 } }
            }

            // Eyes
            Feature {
                startX: root.faceX(-16, -14, 26); startY: root.faceY(-16, -14, 26)
                PathLine { x: root.faceX(-16, -5, 26); y: root.faceY(-16, -5, 26) }
            }
            Feature {
                startX: root.faceX(16, -14, 26); startY: root.faceY(16, -14, 26)
                PathLine { x: root.faceX(16, -5, 26); y: root.faceY(16, -5, 26) }
            }
            // Nose, with its little hook
            Feature {
                startX: root.faceX(0, -13, 29); startY: root.faceY(0, -13, 29)
                PathLine { x: root.faceX(0, 6, 37); y: root.faceY(0, 6, 37) }
                PathQuad {
                    x: root.faceX(-5, 11, 31); y: root.faceY(-5, 11, 31)
                    controlX: root.faceX(0, 11, 35); controlY: root.faceY(0, 11, 35)
                }
            }
            // Mouth
            Feature {
                startX: root.faceX(-15, 20, 21); startY: root.faceY(-15, 20, 21)
                PathQuad {
                    x: root.faceX(15, 20, 21); y: root.faceY(15, 20, 21)
                    controlX: root.faceX(0, 20 + 11 * root.smile, 24); controlY: root.faceY(0, 20 + 11 * root.smile, 24)
                }
            }
        }

        // The tick, drawn inside the ring as it comes to rest
        Checkmark {
            anchors.fill: parent
            scale: 0.75
            transform: Translate { x: 1.5 * root.u }
            color: Theme.success
            lineWidth: root.lineWidth / 0.75
            progress: root.tick
            // Its first dot fades in rather than popping up.
            opacity: Math.min(1, root.tick * 8)
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
