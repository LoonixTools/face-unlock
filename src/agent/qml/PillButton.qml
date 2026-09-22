// SPDX-License-Identifier: GPL-3.0-or-later

import QtQuick

Rectangle {
    id: root

    property string text
    property bool primary: true
    signal clicked

    implicitWidth: Math.max(140, label.implicitWidth + 44)
    implicitHeight: 38
    radius: height / 2
    color: primary ? (area.pressed ? Qt.darker(Theme.accent, 1.2) : area.containsMouse ? Qt.lighter(Theme.accent, 1.1) : Theme.accent)
                   : (area.pressed ? Theme.surface : area.containsMouse ? Qt.lighter(Theme.surfaceRaised, 1.2) : Theme.surfaceRaised)
    opacity: enabled ? 1 : 0.4
    Behavior on color { ColorAnimation { duration: 120 } }

    activeFocusOnTab: true
    Keys.onReturnPressed: root.clicked()
    Keys.onSpacePressed: root.clicked()

    Text {
        id: label
        anchors.centerIn: parent
        text: root.text
        color: Theme.textPrimary
        font.pixelSize: 13
        font.weight: Font.Medium
    }

    MouseArea {
        id: area
        anchors.fill: parent
        hoverEnabled: true
        cursorShape: Qt.PointingHandCursor
        onClicked: root.clicked()
    }
}
