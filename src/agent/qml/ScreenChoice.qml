// SPDX-License-Identifier: GPL-3.0-or-later
//
// Which screen the bubble shows on: the one with the mouse pointer, or always
// the same one. The user's screens are drawn as they stand, with the picture
// on them, and a short film plays on them: the pointer goes from screen to
// screen, and a scan shows its bubble where the choice puts it. Clicking a
// screen picks it. The pick goes back to the menu, which writes it down.

import QtQuick
import QtQuick.Effects
import QtQuick.Shapes
import QtQuick.Window
import FaceUnlock

Window {
    id: window

    // The connected screens: name, title, detail, x, y, width, height (in
    // the desktop's coordinates), laptop, wallpaper.
    required property var screens
    // "auto" or an output name.
    required property string current
    required property var bubble
    // What was picked, empty when the window was closed.
    property string answer: ""
    property string selected: current

    function screenIndex(name) {
        for (let i = 0; i < screens.length; ++i) {
            if (screens[i].name === name) {
                return i
            }
        }
        return -1
    }
    // The screen that always gets the bubble, -1 to follow the pointer.
    readonly property int fixedIndex: screenIndex(selected)

    // "auto", every screen, and the picked one when it is not connected,
    // so that it can be kept.
    readonly property var options: {
        const list = [{ name: "auto", title: i18n("Follow the mouse"), detail: i18n("The screen with the pointer") }]
        for (const s of screens) {
            list.push({ name: s.name, title: s.title, detail: s.detail })
        }
        if (current !== "auto" && screenIndex(current) < 0) {
            list.push({ name: current, title: current, detail: i18n("Not connected") })
        }
        return list
    }

    function pick() {
        if (selected.length > 0) {
            answer = selected
            close()
        }
    }
    function step(by) {
        let i = 0
        for (let j = 0; j < options.length; ++j) {
            if (options[j].name === selected) {
                i = j
            }
        }
        selected = options[(i + by + options.length) % options.length].name
    }

    // One fixed size, so tiling compositors float it (see Enroll). The
    // height follows the texts, whose length depends on the language. It is
    // taken from them rather than from the layout, which only happens once
    // the window shows.
    readonly property int rows: Math.ceil(options.length / 3)
    readonly property int cardsHeight: rows * 64 + (rows - 1) * 12
    readonly property int fitHeight: Math.ceil(36 + heading.height + 10 + intro.height + 26 + stage.height + 14 + caption.height + 18 + cardsHeight + 32 + 38 + 30)
    width: 860
    height: fitHeight
    minimumWidth: 860
    maximumWidth: 860
    minimumHeight: fitHeight
    maximumHeight: fitHeight
    visible: false
    color: Theme.panel
    title: i18n("Choose a screen for the bubble")

    // -- one screen, drawn: a bezel around the picture, on a stand or as
    // a laptop.
    component Monitor: Item {
        id: monitor

        required property var modelData
        required property int index
        readonly property bool chosen: window.selected === modelData.name
        // Another screen sits right below it: no stand then, it would be in
        // the way.
        readonly property bool stacked: {
            for (const s of window.screens) {
                if (s !== modelData && s.y >= modelData.y + modelData.height - 2
                        && s.x < modelData.x + modelData.width && s.x + s.width > modelData.x) {
                    return true
                }
            }
            return false
        }
        // Where the bubble goes.
        property alias glass: glass
        readonly property int gap: 8

        x: stage.offsetX + modelData.x * stage.fit + gap
        y: stage.offsetY + modelData.y * stage.fit + gap
        width: modelData.width * stage.fit - 2 * gap
        height: modelData.height * stage.fit - 2 * gap

        // The stand: a neck and a foot.
        Rectangle {
            visible: !monitor.modelData.laptop && !monitor.stacked
            anchors.horizontalCenter: parent.horizontalCenter
            y: parent.height - 2
            width: Math.max(10, parent.width * 0.09)
            height: 18
            gradient: Gradient {
                GradientStop { position: 0; color: "#1A1B1F" }
                GradientStop { position: 1; color: "#2C2E34" }
            }
        }
        Rectangle {
            visible: !monitor.modelData.laptop && !monitor.stacked
            anchors.horizontalCenter: parent.horizontalCenter
            y: parent.height + 15
            width: Math.max(36, parent.width * 0.3)
            height: 5
            radius: 2.5
            color: "#33353B"
        }
        // A laptop's keyboard half, seen from the front.
        Rectangle {
            visible: monitor.modelData.laptop && !monitor.stacked
            anchors.horizontalCenter: parent.horizontalCenter
            y: parent.height
            width: parent.width + 28
            height: 8
            radius: 4
            gradient: Gradient {
                GradientStop { position: 0; color: "#3A3C43" }
                GradientStop { position: 1; color: "#232429" }
            }
            Rectangle {
                anchors.horizontalCenter: parent.horizontalCenter
                width: Math.max(24, parent.width * 0.14)
                height: 3
                radius: 1.5
                color: "#1A1B1F"
            }
        }

        // The ring around the chosen one. It settles in from a little wider.
        Rectangle {
            anchors.fill: bezel
            anchors.margins: -6
            radius: bezel.radius + 6
            color: "transparent"
            border.width: 2.5
            border.color: Theme.accent
            opacity: monitor.chosen ? 1 : 0
            scale: monitor.chosen ? 1 : 1.05
            Behavior on opacity { NumberAnimation { duration: 200; easing.type: Easing.OutCubic } }
            Behavior on scale { NumberAnimation { duration: 320; easing.type: Easing.OutBack } }
        }

        Rectangle {
            id: bezel
            anchors.fill: parent
            radius: 9
            color: "#08080A"
            border.width: 1
            border.color: area.containsMouse ? "#5A5D66" : "#34363D"
            Behavior on border.color { ColorAnimation { duration: 120 } }
        }

        Item {
            id: glass
            anchors.fill: parent
            anchors.margins: monitor.modelData.laptop ? 4 : 5
            layer.enabled: true
            layer.effect: MultiEffect {
                maskEnabled: true
                maskSource: mask
            }

            // Without a known picture: a calm one of our own.
            Item {
                anchors.fill: parent
                clip: true
                Rectangle {
                    anchors.centerIn: parent
                    width: Math.max(parent.width, parent.height) * 1.6
                    height: width
                    rotation: -24
                    gradient: Gradient {
                        GradientStop { position: 0.2; color: "#0B1530" }
                        GradientStop { position: 0.5; color: "#1C4A93" }
                        GradientStop { position: 0.8; color: "#5B45B8" }
                    }
                }
            }
            Image {
                id: picture
                anchors.fill: parent
                source: monitor.modelData.wallpaper
                fillMode: Image.PreserveAspectCrop
                sourceSize: Qt.size(width * 2, height * 2)
                asynchronous: true
                opacity: status === Image.Ready ? 1 : 0
                Behavior on opacity { NumberAnimation { duration: 200 } }
            }

            // The others go a little dark while one is picked.
            Rectangle {
                anchors.fill: parent
                color: "#000000"
                opacity: window.fixedIndex >= 0 && !monitor.chosen ? 0.45 : 0
                Behavior on opacity { NumberAnimation { duration: 250 } }
            }

            // Its name in the middle, as on a phone's display settings.
            Rectangle {
                id: label
                readonly property real room: parent.width - 36
                // A little low, out of the way of the bubble at the top.
                anchors.centerIn: parent
                anchors.verticalCenterOffset: parent.height * 0.12
                width: Math.min(room, Math.max(title.implicitWidth, detail.implicitWidth)) + 20
                height: title.implicitHeight + detail.implicitHeight + 13
                radius: 8
                color: "#A6000000"
                visible: parent.height > 54
                Text {
                    id: title
                    anchors.horizontalCenter: parent.horizontalCenter
                    y: 6
                    width: Math.min(implicitWidth, label.room)
                    horizontalAlignment: Text.AlignHCenter
                    elide: Text.ElideRight
                    color: Theme.textPrimary
                    font.pixelSize: 12
                    font.weight: Font.DemiBold
                    text: monitor.modelData.title
                }
                Text {
                    id: detail
                    anchors.horizontalCenter: parent.horizontalCenter
                    anchors.top: title.bottom
                    anchors.topMargin: 1
                    width: Math.min(implicitWidth, label.room)
                    horizontalAlignment: Text.AlignHCenter
                    elide: Text.ElideRight
                    color: Theme.textDetail
                    font.pixelSize: 10
                    text: monitor.modelData.detail
                }
            }
        }
        Rectangle {
            id: mask
            width: glass.width
            height: glass.height
            radius: 4
            visible: false
            layer.enabled: true
        }

        MouseArea {
            id: area
            anchors.fill: parent
            anchors.margins: -6
            hoverEnabled: true
            cursorShape: Qt.PointingHandCursor
            onClicked: window.selected = monitor.modelData.name
            onDoubleClicked: {
                window.selected = monitor.modelData.name
                window.pick()
            }
        }
    }

    // -- one choice under the picture: a radio button, a name, a line.
    component Choice: Rectangle {
        id: card

        required property var modelData
        readonly property bool chosen: window.selected === modelData.name

        width: 236
        height: 64
        radius: 14
        color: area.containsMouse && !chosen ? Qt.lighter(Theme.surface, 1.25) : Theme.surface
        border.width: 2
        border.color: chosen ? Theme.accent : "transparent"
        Behavior on border.color { ColorAnimation { duration: 150 } }
        Behavior on color { ColorAnimation { duration: 120 } }

        Rectangle {
            id: radio
            x: 16
            anchors.verticalCenter: parent.verticalCenter
            width: 18
            height: 18
            radius: 9
            color: "transparent"
            border.width: 2
            border.color: card.chosen ? Theme.accent : Theme.textSecondary
            Rectangle {
                anchors.centerIn: parent
                width: 8
                height: 8
                radius: 4
                color: Theme.accent
                scale: card.chosen ? 1 : 0
                Behavior on scale { NumberAnimation { duration: 180; easing.type: Easing.OutBack } }
            }
        }
        Column {
            anchors.verticalCenter: parent.verticalCenter
            x: radio.x + radio.width + 12
            width: parent.width - x - 14
            spacing: 2
            Text {
                width: parent.width
                elide: Text.ElideRight
                color: Theme.textPrimary
                font.pixelSize: 14
                font.weight: Font.DemiBold
                text: card.modelData.title
            }
            Text {
                width: parent.width
                elide: Text.ElideRight
                color: Theme.textSecondary
                font.pixelSize: 12
                text: card.modelData.detail
            }
        }

        MouseArea {
            id: area
            anchors.fill: parent
            hoverEnabled: true
            cursorShape: Qt.PointingHandCursor
            onClicked: window.selected = card.modelData.name
            onDoubleClicked: {
                window.selected = card.modelData.name
                window.pick()
            }
        }
    }

    Column {
        id: head
        anchors.top: parent.top
        anchors.topMargin: 36
        anchors.horizontalCenter: parent.horizontalCenter
        width: parent.width - 96
        spacing: 10

        Text {
            id: heading
            width: parent.width
            horizontalAlignment: Text.AlignHCenter
            color: Theme.textPrimary
            font.pixelSize: 26
            font.weight: Font.Bold
            text: i18n("Where the bubble shows up")
        }
        Text {
            id: intro
            width: parent.width
            horizontalAlignment: Text.AlignHCenter
            wrapMode: Text.WordWrap
            color: Theme.textSecondary
            font.pixelSize: 13
            font.weight: Font.Medium
            lineHeight: 1.15
            text: i18n("Let the bubble follow your mouse, or keep it on one screen. Click a screen to pick it. You can change it later under Settings.")
        }
    }

    // -- the desk with the screens on it
    Rectangle {
        id: stage
        anchors.top: head.bottom
        anchors.topMargin: 26
        anchors.horizontalCenter: parent.horizontalCenter
        width: parent.width - 96
        height: 300
        radius: 20
        border.width: 1
        border.color: "#202227"
        gradient: Gradient {
            GradientStop { position: 0; color: "#16181D" }
            GradientStop { position: 1; color: "#0C0D10" }
        }
        clip: true

        // All screens together, made to fit with room for the stands.
        readonly property var bounds: {
            if (window.screens.length === 0) {
                return Qt.rect(0, 0, 1920, 1080)
            }
            let x0 = Infinity, y0 = Infinity, x1 = -Infinity, y1 = -Infinity
            for (const s of window.screens) {
                x0 = Math.min(x0, s.x)
                y0 = Math.min(y0, s.y)
                x1 = Math.max(x1, s.x + s.width)
                y1 = Math.max(y1, s.y + s.height)
            }
            return Qt.rect(x0, y0, x1 - x0, y1 - y0)
        }
        readonly property real fit: Math.min((width - 80) / bounds.width, (height - 80) / bounds.height)
        readonly property real offsetX: (width - bounds.width * fit) / 2 - bounds.x * fit
        readonly property real offsetY: 26 + (height - 80 - bounds.height * fit) / 2 - bounds.y * fit

        Repeater {
            id: monitors
            model: window.screens
            delegate: Monitor {}
        }

        // The bubble, on the screen the choice puts it, at about the size it
        // has there. It only moves while it is closed.
        Bubble {
            id: preview
            readonly property Item host: {
                const m = monitors.count > film.bubbleIndex ? monitors.itemAt(film.bubbleIndex) : null
                return m ? m.glass : null
            }
            parent: host ? host : stage
            visible: host !== null
            bubble: window.bubble
            transformOrigin: Item.Top
            scale: host ? Math.max(0.18, Math.min(0.32, host.width / 900)) : 0.2
            x: host ? (host.width - width) / 2 : 0
            y: 0
        }

        // A ring where the pointer stops, as if it was the one moving.
        Rectangle {
            id: ripple
            x: pointer.x - width / 2
            y: pointer.y - height / 2
            width: 34
            height: 34
            radius: 17
            color: "transparent"
            border.width: 2
            border.color: "#FFFFFF"
            opacity: 0
            scale: 0.3
            ParallelAnimation {
                id: rippleAnimation
                NumberAnimation { target: ripple; property: "scale"; from: 0.3; to: 1.3; duration: 520; easing.type: Easing.OutCubic }
                NumberAnimation { target: ripple; property: "opacity"; from: 0.7; to: 0; duration: 520; easing.type: Easing.OutQuad }
            }
        }

        // The mouse pointer. Its tip is at x, y.
        Item {
            id: pointer
            width: 1
            height: 1
            z: 10
            Shape {
                x: -1
                y: -1
                width: 16
                height: 24
                preferredRendererType: Shape.CurveRenderer
                layer.enabled: true
                layer.effect: MultiEffect {
                    shadowEnabled: true
                    shadowColor: "#000000"
                    shadowOpacity: 0.55
                    shadowBlur: 0.35
                    shadowVerticalOffset: 1.5
                }
                ShapePath {
                    fillColor: "#FFFFFF"
                    strokeColor: "#000000"
                    strokeWidth: 1.3
                    joinStyle: ShapePath.RoundJoin
                    startX: 1; startY: 1
                    PathLine { x: 1; y: 19 }
                    PathLine { x: 5.6; y: 14.8 }
                    PathLine { x: 8.8; y: 22 }
                    PathLine { x: 11.6; y: 20.8 }
                    PathLine { x: 8.5; y: 13.8 }
                    PathLine { x: 14.4; y: 13.6 }
                    PathLine { x: 1; y: 1 }
                }
            }
        }

        // -- the film: the pointer goes to a screen, a scan shows its bubble,
        // the bubble closes, on to the next screen.
        Item {
            id: film

            // The screen the pointer is on, and the one with the bubble.
            property int pointerIndex: 0
            property int bubbleIndex: 0
            property int round: 0
            // After a new choice: show it straight away, without a walk first.
            property bool quick: false
            property bool playing: false
            readonly property real pace: window.bubble ? window.bubble.pace : 1

            // A spot on screen i, a different one each round.
            function spot(i) {
                const m = monitors.itemAt(i)
                if (!m) {
                    return Qt.point(stage.width / 2, stage.height / 2)
                }
                const fx = [0.64, 0.36, 0.72, 0.44][round % 4]
                const fy = [0.62, 0.7, 0.5, 0.76][round % 4]
                return Qt.point(m.x + m.width * fx, m.y + m.height * fy)
            }
            function walk() {
                round += 1
                const count = window.screens.length
                walkTo.screen = count > 1 ? (pointerIndex + 1) % count : 0
                const p = spot(walkTo.screen)
                walkX.to = p.x
                walkY.to = p.y
                walkTo.restart()
            }
            function scan() {
                bubbleIndex = window.fixedIndex >= 0 ? window.fixedIndex : pointerIndex
                window.bubble.scanStarted()
                faceLater.restart()
            }
            // A new choice: close what is open, then show the new place.
            function replay() {
                if (!playing) {
                    return
                }
                walkTo.stop()
                scanLater.stop()
                faceLater.stop()
                doneLater.stop()
                quick = true
                if (window.bubble.shown) {
                    window.bubble.dismiss()
                } else {
                    restLater.restart()
                }
            }

            ParallelAnimation {
                id: walkTo
                property int screen: 0
                NumberAnimation { id: walkX; target: pointer; property: "x"; duration: 1150; easing.type: Easing.InOutCubic }
                NumberAnimation { id: walkY; target: pointer; property: "y"; duration: 1150; easing.type: Easing.InOutCubic }
                onFinished: {
                    film.pointerIndex = screen
                    rippleAnimation.restart()
                    scanLater.restart()
                }
            }
            Timer {
                id: scanLater
                interval: 380
                onTriggered: film.scan()
            }
            Timer {
                id: faceLater
                interval: 700 * film.pace
                onTriggered: {
                    window.bubble.faceFound()
                    doneLater.restart()
                }
            }
            Timer {
                id: doneLater
                interval: 1000 * film.pace
                onTriggered: window.bubble.succeeded()
            }
            Connections {
                target: window.bubble
                function onShownChanged() {
                    if (!window.bubble.shown) {
                        restLater.restart()
                    }
                }
            }
            Timer {
                id: restLater
                interval: film.quick ? 120 : 650
                onTriggered: {
                    if (film.quick) {
                        film.quick = false
                        film.scan()
                    } else {
                        film.walk()
                    }
                }
            }
        }

        // Once the screens are laid out.
        Timer {
            running: true
            interval: 1
            onTriggered: {
                const p = film.spot(0)
                pointer.x = p.x
                pointer.y = p.y
                film.playing = true
                scanLater.interval = 600
                scanLater.restart()
                scanLater.interval = 380
            }
        }
    }

    onSelectedChanged: film.replay()

    // What the film shows, in words.
    Text {
        id: caption
        anchors.top: stage.bottom
        anchors.topMargin: 14
        anchors.horizontalCenter: parent.horizontalCenter
        width: stage.width - 40
        height: 2 * fontMetrics.height + 4
        horizontalAlignment: Text.AlignHCenter
        verticalAlignment: Text.AlignVCenter
        wrapMode: Text.WordWrap
        color: Theme.textDetail
        font.pixelSize: 14
        text: {
            if (window.selected === "auto") {
                return i18n("The bubble shows up on the screen with your mouse pointer.")
            }
            const i = window.screenIndex(window.selected)
            if (i < 0) {
                return i18n("%1 is not connected. Until it is, the bubble follows your mouse.", window.selected)
            }
            return i18n("The bubble always shows up on %1, wherever your mouse is.", window.screens[i].title)
        }
        FontMetrics {
            id: fontMetrics
            font: caption.font
        }
    }

    // Three to a row, each row in the middle.
    Column {
        id: cards
        anchors.top: caption.bottom
        anchors.topMargin: 18
        anchors.horizontalCenter: parent.horizontalCenter
        spacing: 12
        focus: true

        Keys.onLeftPressed: window.step(-1)
        Keys.onRightPressed: window.step(1)
        Keys.onUpPressed: window.step(-1)
        Keys.onDownPressed: window.step(1)
        Keys.onReturnPressed: window.pick()
        Keys.onEnterPressed: window.pick()
        Keys.onEscapePressed: window.close()

        Repeater {
            model: window.rows
            Row {
                required property int index
                anchors.horizontalCenter: parent.horizontalCenter
                spacing: 12
                Repeater {
                    model: window.options.slice(parent.index * 3, parent.index * 3 + 3)
                    delegate: Choice {}
                }
            }
        }
    }

    Row {
        anchors.bottom: parent.bottom
        anchors.bottomMargin: 30
        anchors.horizontalCenter: parent.horizontalCenter
        spacing: 12

        PillButton {
            primary: false
            text: i18n("Cancel")
            onClicked: window.close()
        }
        PillButton {
            enabled: window.selected.length > 0
            text: i18n("Continue")
            onClicked: window.pick()
        }
    }
}
