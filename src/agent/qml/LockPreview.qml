// SPDX-License-Identifier: GPL-3.0-or-later
//
// The user's own lock screen, drawn from what LockPreview (C++) read out of
// its config, at the size of the screen. Whoever shows it scales it down.
// Placement as in hyprlock: from the side or middle halign and valign name,
// x to the right and y upwards. One Repeater per kind of widget, stacked in
// the order hyprlock draws them.

import QtQuick
import QtQuick.Effects

Item {
    id: root

    required property var widgets

    readonly property var numbered: widgets.map((w, i) => Object.assign({order: i}, w))
    function only(type) {
        return numbered.filter(w => w.type === type)
    }
    function alignX(halign, size, offset) {
        return halign === "center" ? (width - size) / 2 + offset
             : halign === "right" ? width - size + offset
             : offset
    }
    function alignY(valign, size, offset) {
        const up = valign === "center" ? (height - size) / 2 + offset
                 : valign === "top" ? height - size + offset
                 : offset
        return height - up - size
    }

    clip: true

    Repeater {
        model: root.only("background")

        Item {
            id: back
            required property var modelData
            readonly property var w: modelData
            z: w.order
            width: root.width
            height: root.height

            Rectangle {
                anchors.fill: parent
                color: back.w.color
            }
            Image {
                // Past the edges when blurred: the blur would pull in the
                // nothing beyond them and darken the rim.
                anchors.fill: parent
                anchors.margins: back.w.blur > 0 ? -Math.round(64 * back.w.blur) : 0
                visible: back.w.fill !== "solid_color"
                source: back.w.source
                asynchronous: true
                sourceSize: Qt.size(root.width, root.height)
                fillMode: back.w.fill === "fit" ? Image.PreserveAspectFit
                        : back.w.fill === "stretch" ? Image.Stretch
                        : back.w.fill === "center" ? Image.Pad
                        : back.w.fill === "tile" ? Image.Tile
                        : Image.PreserveAspectCrop
                layer.enabled: back.w.blur > 0
                layer.effect: MultiEffect {
                    blurEnabled: true
                    blur: 1
                    blurMax: Math.round(64 * back.w.blur)
                }
            }
            Rectangle {
                anchors.fill: parent
                color: "black"
                opacity: back.w.dim
            }
        }
    }

    Repeater {
        model: root.only("label")

        Text {
            id: line
            required property var modelData
            readonly property var w: modelData
            z: w.order
            x: root.alignX(w.halign, width, w.x)
            y: root.alignY(w.valign, height, w.y)
            rotation: w.rotate
            text: w.text
            textFormat: Text.StyledText
            color: w.color
            font.family: w.family
            font.weight: w.weight
            font.italic: w.italic
            font.pointSize: w.size
            horizontalAlignment: w.align === "left" ? Text.AlignLeft : w.align === "right" ? Text.AlignRight : Text.AlignHCenter
            layer.enabled: w.shadow
            layer.effect: MultiEffect {
                shadowEnabled: true
                shadowColor: line.w.shadowColor
                shadowBlur: Math.min(1, line.w.shadowSize / 8)
                shadowHorizontalOffset: 0
                shadowVerticalOffset: 0
            }
        }
    }

    Repeater {
        model: root.only("input")

        // The outline is the outer colour around the inner one. A gradient
        // outline needs an opaque inside, which leaves it showing only at
        // the edge.
        Rectangle {
            id: field
            required property var modelData
            readonly property var w: modelData
            readonly property var outer: w.outer.colors
            readonly property bool blend: outer.length > 1 && w.inner.a >= 0.99
            z: w.order
            x: root.alignX(w.halign, width, w.x)
            y: root.alignY(w.valign, height, w.y)
            rotation: w.rotate
            width: w.width
            height: w.height
            radius: w.rounding < 0 ? height / 2 : Math.min(w.rounding, height / 2)
            color: blend ? "transparent" : w.inner
            border.width: blend ? 0 : w.thickness
            border.color: outer.length > 0 ? outer[0] : "transparent"
            gradient: blend ? outline : null

            Gradient {
                id: outline
                orientation: Math.abs(Math.cos(field.w.outer.angle * Math.PI / 180)) >= Math.abs(Math.sin(field.w.outer.angle * Math.PI / 180))
                             ? Gradient.Horizontal : Gradient.Vertical
                GradientStop { position: 0; color: field.outer.length > 0 ? field.outer[0] : "transparent" }
                GradientStop { position: 1; color: field.outer.length > 0 ? field.outer[field.outer.length - 1] : "transparent" }
            }
            Rectangle {
                visible: field.blend
                anchors.fill: parent
                anchors.margins: field.w.thickness
                radius: Math.max(0, field.radius - field.w.thickness)
                color: field.w.inner
            }
            Text {
                anchors.centerIn: parent
                text: field.w.placeholder
                textFormat: Text.StyledText
                color: field.w.fontColor
                font.family: field.w.family
                font.weight: field.w.weight
                font.italic: field.w.italic
                font.pointSize: Math.max(1, field.height / 4)
            }
        }
    }

    Repeater {
        model: root.only("shape")

        Rectangle {
            required property var modelData
            readonly property var w: modelData
            z: w.order
            x: root.alignX(w.halign, width, w.x)
            y: root.alignY(w.valign, height, w.y)
            rotation: w.rotate
            width: w.width
            height: w.height
            radius: w.rounding < 0 ? Math.min(width, height) / 2 : Math.min(w.rounding, Math.min(width, height) / 2)
            color: w.color
            border.width: w.borderSize
            border.color: w.border.colors.length > 0 ? w.border.colors[0] : "transparent"
        }
    }

    Repeater {
        model: root.only("image")

        Item {
            id: frame
            required property var modelData
            readonly property var w: modelData
            readonly property real round: w.rounding < 0 ? width / 2 : Math.min(w.rounding, width / 2)
            z: w.order
            x: root.alignX(w.halign, width, w.x)
            y: root.alignY(w.valign, height, w.y)
            rotation: w.rotate
            width: w.size + 2 * w.borderSize
            height: width

            Rectangle {
                anchors.fill: parent
                radius: frame.round
                color: frame.w.border.colors.length > 0 ? frame.w.border.colors[0] : "transparent"
            }
            Image {
                id: photo
                anchors.fill: parent
                anchors.margins: frame.w.borderSize
                source: frame.w.source
                fillMode: Image.PreserveAspectCrop
                asynchronous: true
                layer.enabled: true
                layer.effect: MultiEffect {
                    maskEnabled: true
                    maskSource: photoMask
                }
            }
            Rectangle {
                id: photoMask
                width: photo.width
                height: photo.height
                radius: Math.max(0, frame.round - frame.w.borderSize)
                visible: false
                layer.enabled: true
            }
        }
    }

    Repeater {
        model: root.only("ring")

        // swaylock's indicator, in the middle.
        Rectangle {
            id: circle
            required property var modelData
            readonly property var w: modelData
            z: w.order
            anchors.centerIn: parent
            width: 2 * w.radius + 2 * w.thickness
            height: width
            radius: width / 2
            color: w.inside
            border.width: w.thickness
            border.color: w.ring

            Text {
                anchors.centerIn: parent
                horizontalAlignment: Text.AlignHCenter
                text: circle.w.text
                color: circle.w.textColor
                font.pixelSize: circle.w.radius / 3
            }
        }
    }
}
