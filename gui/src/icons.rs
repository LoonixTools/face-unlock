//! Line icons on a 24 point grid, drawn with the text colour.

use rune::Icon;

macro_rules! line_icon {
    ($static:ident, $name:literal, [$($d:literal),+ $(,)?]) => {
        line_icon!($static, $name, "1.75", [$($d),+]);
    };
    ($static:ident, $name:literal, $width:literal, [$($d:literal),+ $(,)?]) => {
        pub static $static: Icon = Icon {
            name: $name,
            view: 24.0,
            body: concat!($(
                "<path d=\"", $d,
                "\" fill=\"none\" stroke=\"currentColor\" stroke-width=\"", $width, "\" stroke-linecap=\"round\" stroke-linejoin=\"round\"/>"
            ),+),
        };
    };
}

line_icon!(SCAN_FACE, "fu-scan-face", [
    "M3 7V5a2 2 0 0 1 2-2h2",
    "M17 3h2a2 2 0 0 1 2 2v2",
    "M21 17v2a2 2 0 0 1-2 2h-2",
    "M7 21H5a2 2 0 0 1-2-2v-2",
    "M8 14s1.5 2 4 2 4-2 4-2",
    "M9 9h.01",
    "M15 9h.01",
]);

line_icon!(FACE, "fu-face", [
    "M12 2a10 10 0 1 0 0 20a10 10 0 1 0 0-20z",
    "M8 14s1.5 2 4 2 4-2 4-2",
    "M9 9h.01",
    "M15 9h.01",
]);

line_icon!(USERS, "fu-users", [
    "M16 21v-2a4 4 0 0 0-4-4H6a4 4 0 0 0-4 4v2",
    "M9 3a4 4 0 1 0 0 8a4 4 0 1 0 0-8z",
    "M22 21v-2a4 4 0 0 0-3-3.87",
    "M16 3.13a4 4 0 0 1 0 7.75",
]);

line_icon!(LOCK, "fu-lock", [
    "M5 11h14a2 2 0 0 1 2 2v7a2 2 0 0 1-2 2H5a2 2 0 0 1-2-2v-7a2 2 0 0 1 2-2z",
    "M7 11V7a5 5 0 0 1 10 0v4",
]);

line_icon!(SHIELD, "fu-shield", [
    "M20 13c0 5-3.5 7.5-7.66 8.95a1 1 0 0 1-.67-.01C7.5 20.5 4 18 4 13V6a1 1 0 0 1 1-1c2 0 4.5-1.2 6.24-2.72a1.17 1.17 0 0 1 1.52 0C14.51 3.81 17 5 19 5a1 1 0 0 1 1 1z",
    "M9 12l2 2 4-4",
]);

line_icon!(EYE, "fu-eye", [
    "M2.062 12.348a1 1 0 0 1 0-.696 10.75 10.75 0 0 1 19.876 0 1 1 0 0 1 0 .696 10.75 10.75 0 0 1-19.876 0",
    "M12 9a3 3 0 1 0 0 6a3 3 0 1 0 0-6z",
]);

line_icon!(BUBBLE, "fu-bubble", [
    "M5 3h14a2 2 0 0 1 2 2v14a2 2 0 0 1-2 2H5a2 2 0 0 1-2-2V5a2 2 0 0 1 2-2z",
    "M9.5 7h5",
]);

line_icon!(PLAY, "fu-play", [
    "M6 3l14 9-14 9V3z",
]);

line_icon!(CAMERA, "fu-camera", [
    "M14.5 4h-5L7 7H4a2 2 0 0 0-2 2v9a2 2 0 0 0 2 2h16a2 2 0 0 0 2-2V9a2 2 0 0 0-2-2h-3l-2.5-3z",
    "M12 10a3 3 0 1 0 0 6a3 3 0 1 0 0-6z",
]);

line_icon!(MONITOR, "fu-monitor", [
    "M4 3h16a2 2 0 0 1 2 2v10a2 2 0 0 1-2 2H4a2 2 0 0 1-2-2V5a2 2 0 0 1 2-2z",
    "M8 21h8",
    "M12 17v4",
]);

line_icon!(PENCIL, "fu-pencil", [
    "M21.174 6.812a1 1 0 0 0-3.986-3.987L3.842 16.174a2 2 0 0 0-.5.83l-1.321 4.352a.5.5 0 0 0 .623.622l4.353-1.32a2 2 0 0 0 .83-.497z",
    "M15 5l4 4",
]);

line_icon!(TRASH, "fu-trash", [
    "M3 6h18",
    "M19 6v14a2 2 0 0 1-2 2H7a2 2 0 0 1-2-2V6",
    "M8 6V4a2 2 0 0 1 2-2h4a2 2 0 0 1 2 2v2",
]);

line_icon!(WARNING, "fu-warning", [
    "M21.73 18l-8-14a2 2 0 0 0-3.48 0l-8 14A2 2 0 0 0 4 21h16a2 2 0 0 0 1.73-3",
    "M12 9v4",
    "M12 17h.01",
]);

line_icon!(INFO, "fu-info", [
    "M12 2a10 10 0 1 0 0 20a10 10 0 1 0 0-20z",
    "M12 16v-4",
    "M12 8h.01",
]);

line_icon!(CHECK_CIRCLE, "fu-check-circle", [
    "M21.801 10A10 10 0 1 1 17 3.335",
    "M9 11l3 3L22 4",
]);

line_icon!(POWER, "fu-power", [
    "M12 2v10",
    "M18.4 6.6a9 9 0 1 1-12.77.04",
]);

line_icon!(SUDO, "fu-sudo", [
    "M4 4h16a2 2 0 0 1 2 2v12a2 2 0 0 1-2 2H4a2 2 0 0 1-2-2V6a2 2 0 0 1 2-2z",
    "M7 10l3 2-3 2",
    "M12 15h5",
]);

line_icon!(KEY, "fu-key", [
    "M2.586 17.414A2 2 0 0 0 2 18.828V21a1 1 0 0 0 1 1h3a1 1 0 0 0 1-1v-1a1 1 0 0 1 1-1h1a1 1 0 0 0 1-1v-1a1 1 0 0 1 1-1h.172a2 2 0 0 0 1.414-.586l.814-.814a6.5 6.5 0 1 0-4-4z",
    "M16.5 7.5h.01",
]);

line_icon!(SPARKLE, "fu-sparkle", [
    "M9.937 15.5A2 2 0 0 0 8.5 14.063l-6.135-1.582a.5.5 0 0 1 0-.962L8.5 9.936A2 2 0 0 0 9.937 8.5l1.582-6.135a.5.5 0 0 1 .963 0L14.063 8.5A2 2 0 0 0 15.5 9.937l6.135 1.581a.5.5 0 0 1 0 .964L15.5 14.063a2 2 0 0 0-1.437 1.437l-1.582 6.135a.5.5 0 0 1-.963 0z",
]);

line_icon!(LAPTOP, "fu-laptop", [
    "M18 5a2 2 0 0 1 2 2v8.526a2 2 0 0 0 .212.897l1.068 2.127a1 1 0 0 1-.9 1.45H3.62a1 1 0 0 1-.9-1.45l1.068-2.127A2 2 0 0 0 4 15.526V7a2 2 0 0 1 2-2z",
    "M20.054 15.987H3.946",
]);

line_icon!(PHONE, "fu-phone", [
    "M7 2h10a2 2 0 0 1 2 2v16a2 2 0 0 1-2 2H7a2 2 0 0 1-2-2V4a2 2 0 0 1 2-2z",
    "M12 18h.01",
]);

line_icon!(PHOTO, "fu-photo", [
    "M5 3h14a2 2 0 0 1 2 2v14a2 2 0 0 1-2 2H5a2 2 0 0 1-2-2V5a2 2 0 0 1 2-2z",
    "M9 7a2 2 0 1 0 0 4a2 2 0 1 0 0-4z",
    "m21 15-3.086-3.086a2 2 0 0 0-2.828 0L6 21",
]);

line_icon!(GLASSES, "fu-glasses", [
    "M6 11a4 4 0 1 0 0 8a4 4 0 1 0 0-8z",
    "M18 11a4 4 0 1 0 0 8a4 4 0 1 0 0-8z",
    "M14 15a2 2 0 0 0-4 0",
    "M2.5 13 5 7c.7-1.3 1.4-2 3-2",
    "M21.5 13 19 7c-.7-1.3-1.5-2-3-2",
]);

line_icon!(EYE_OFF, "fu-eye-off", [
    "M10.733 5.076a10.744 10.744 0 0 1 11.205 6.575 1 1 0 0 1 0 .696 10.747 10.747 0 0 1-1.444 2.49",
    "M14.084 14.158a3 3 0 0 1-4.242-4.242",
    "M17.479 17.499a10.75 10.75 0 0 1-15.417-5.151 1 1 0 0 1 0-.696 10.75 10.75 0 0 1 4.446-5.143",
    "m2 2 20 20",
]);

// What the camera does with something held up to it: a mark in a small
// round badge, and the corners of a viewfinder around the thing.

line_icon!(CHECK, "fu-check", "2.5", ["M20 6 9 17l-5-5"]);

line_icon!(CROSS, "fu-cross", "2.5", ["M18 6 6 18", "m6 6 12 12"]);

line_icon!(QUESTION, "fu-question", "2.5", ["M8 8.5a4 4 0 1 1 5.5 3.7c-.9.4-1.5 1.1-1.5 2.1v.7", "M12 19.5h.01"]);

line_icon!(MINUS, "fu-minus", "2.5", ["M5 12h14"]);

line_icon!(VIEWFINDER, "fu-viewfinder", "1.0", [
    "M2 7V5a3 3 0 0 1 3-3h2",
    "M17 2h2a3 3 0 0 1 3 3v2",
    "M22 17v2a3 3 0 0 1-3 3h-2",
    "M7 22H5a3 3 0 0 1-3-3v-2",
]);
