// SPDX-License-Identifier: GPL-3.0-or-later
//
// Colours and sizes, in one place. The palette and the island's proportions
// follow Glance's (github.com/jonnyoo/glance): black panel, white glyph, one
// blue for anything that is done.

pragma Singleton

import QtQuick

QtObject {
    readonly property color accent: "#3499FF"
    readonly property color accentPale: "#CFE7FF"
    readonly property color accentBright: "#7FC2FF"
    readonly property color success: "#30D158"
    readonly property color failure: "#FF453A"

    readonly property color panel: "#000000"
    readonly property color surface: "#1E1E1E"
    readonly property color surfaceRaised: "#323232"
    readonly property color placeholder: "#2F2F2F"

    readonly property color textPrimary: "#FFFFFF"
    readonly property color textSecondary: "#949494"
    readonly property color textDetail: "#BDBDBD"

    // The island, closed and open. It hangs this far below the top edge.
    readonly property int closedWidth: 80
    readonly property int closedHeight: 24
    readonly property int openWidth: 180
    readonly property int openHeight: 180
    readonly property int openRadius: 48
    readonly property int minimalWidth: 150
    readonly property int minimalHeight: 40
    readonly property int topGap: 6

    // Enter: slide down, then grow. Leave: shrink, then slide up.
    readonly property int slideDuration: 250
    readonly property int expandDelay: 160
    readonly property int slideOutDelay: 180
}
