// SPDX-License-Identifier: GPL-3.0-or-later
//
// face-unlock's bubble on GNOME.
//
// GNOME lets no program draw above its windows, and nothing at all above its
// lock screen. An extension runs inside GNOME Shell and may. This one draws
// the bubble the agent draws everywhere else, with the same shapes and the
// same timing: a port of src/agent/qml (Bubble, FaceGlyph, LockGlyph,
// Checkmark). What to show comes from the agent over the session bus.

import Clutter from 'gi://Clutter';
import Gio from 'gi://Gio';
import GLib from 'gi://GLib';
import Pango from 'gi://Pango';
import PangoCairo from 'gi://PangoCairo';
import St from 'gi://St';

import * as Main from 'resource:///org/gnome/shell/ui/main.js';
import {Extension} from 'resource:///org/gnome/shell/extensions/extension.js';

const BUS_NAME = 'io.github.loonixtools.FaceUnlock';
const OBJECT_PATH = '/io/github/loonixtools/FaceUnlock';
const BubbleProxy = Gio.DBusProxy.makeProxyWrapper(`
<node>
  <interface name="io.github.loonixtools.FaceUnlock.Bubble">
    <method name="GetState"><arg type="a{sv}" direction="out"/></method>
    <signal name="StateChanged"><arg type="a{sv}"/></signal>
  </interface>
</node>`);

// Theme.qml
const Theme = {
    success: [0x30, 0xd1, 0x58],
    failure: [0xff, 0x45, 0x3a],
    panel: [0, 0, 0],
    textPrimary: [0xff, 0xff, 0xff],
    textSecondary: [0x94, 0x94, 0x94],
    textDetail: [0xbd, 0xbd, 0xbd],
    closedWidth: 80,
    closedHeight: 24,
    openWidth: 180,
    openHeight: 180,
    openRadius: 48,
    minimalWidth: 150,
    minimalHeight: 40,
    topGap: 6,
    slideInDuration: 280,
    expandDelay: 150,
    growDuration: 460,
    shrinkDuration: 280,
    slideOutDelay: 150,
    slideOutDuration: 240,
};
const WIDTH = 420;
const HEIGHT = 300;

// Qt's easing curves, as the QML uses them.
const Ease = {
    linear: p => p,
    outQuad: p => 1 - (1 - p) * (1 - p),
    inOutQuad: p => p < 0.5 ? 2 * p * p : 1 - (-2 * p + 2) ** 2 / 2,
    outCubic: p => 1 - (1 - p) ** 3,
    inCubic: p => p ** 3,
    inOutSine: p => -(Math.cos(Math.PI * p) - 1) / 2,
    outBack: s => p => {
        const q = p - 1;
        return q * q * ((s + 1) * q + s) + 1;
    },
};
const OUT_BACK = Ease.outBack(1.70158);

function now() {
    return GLib.get_monotonic_time() / 1000;
}

function smooth(from, to, p) {
    const x = Math.max(0, Math.min(1, (p - from) / (to - from)));
    return x * x * (3 - 2 * x);
}

// A value that eases to a new target from wherever it is, like a QML
// Behavior. Colours are arrays and ease per channel.
class Tween {
    constructor(value) {
        this._from = value;
        this._to = value;
        this._start = 0;
        this._duration = 0;
        this._ease = Ease.linear;
    }

    set(to, duration, ease, at) {
        if (this._same(to, this._to))
            return;
        this._from = this.get(at);
        this._to = to;
        this._start = at;
        this._duration = duration;
        this._ease = ease;
    }

    jump(value) {
        this._from = this._to = value;
        this._duration = 0;
    }

    get(at) {
        const p = this._duration > 0 ? Math.min(1, (at - this._start) / this._duration) : 1;
        const e = this._ease(p);
        if (Array.isArray(this._to))
            return this._to.map((t, i) => this._from[i] + (t - this._from[i]) * e);
        return this._from + (this._to - this._from) * e;
    }

    busy(at) {
        return this._duration > 0 && at - this._start < this._duration;
    }

    _same(a, b) {
        return Array.isArray(a) ? a.every((v, i) => v === b[i]) : a === b;
    }
}

// A run of steps from 0, like a QML SequentialAnimation of NumberAnimations:
// [target, duration, easing]...
function sequence(steps, start, at, pace) {
    if (start === null)
        return [0, false];
    let t = at - start;
    let from = 0;
    for (const [to, duration, ease] of steps) {
        const d = duration * pace;
        if (t < d)
            return [from + (to - from) * ease(t / d), true];
        t -= d;
        from = to;
    }
    return [from, false];
}

function rgba(cr, color, alpha = 1) {
    cr.setSourceRGBA(color[0] / 255, color[1] / 255, color[2] / 255, alpha);
}

// A quadratic curve in cairo, which only knows cubic ones.
function quadTo(cr, x0, y0, cx, cy, x, y) {
    cr.curveTo(x0 + 2 / 3 * (cx - x0), y0 + 2 / 3 * (cy - y0),
        x + 2 / 3 * (cx - x), y + 2 / 3 * (cy - y), x, y);
}

function polyline(cr, points) {
    if (points.length < 2)
        return;
    cr.moveTo(points[0][0], points[0][1]);
    for (let i = 1; i < points.length; ++i)
        cr.lineTo(points[i][0], points[i][1]);
}

function roundedRect(cr, x, y, w, h, r) {
    r = Math.max(0, Math.min(r, w / 2, h / 2));
    cr.newSubPath();
    cr.arc(x + w - r, y + r, r, -Math.PI / 2, 0);
    cr.arc(x + w - r, y + h - r, r, 0, Math.PI / 2);
    cr.arc(x + r, y + h - r, r, Math.PI / 2, Math.PI);
    cr.arc(x + r, y + r, r, Math.PI, 3 * Math.PI / 2);
    cr.closePath();
}

// Draws what draw() draws into its own layer, at the given opacity and
// scaled about the middle of the square it sits in.
function layer(cr, opacity, scale, cx, cy, draw) {
    if (opacity <= 0.001)
        return;
    cr.save();
    cr.pushGroup();
    cr.translate(cx, cy);
    cr.scale(scale, scale);
    cr.translate(-cx, -cy);
    draw();
    cr.popGroupToSource();
    cr.paintWithAlpha(Math.min(1, opacity));
    cr.restore();
}

// LockGlyph.qml: a padlock that opens.
class LockGlyph {
    constructor() {
        this._openness = new Tween(0);
        this.open = false;
        this.pace = 1;
    }

    setOpen(open, at) {
        if (open === this.open)
            return;
        this.open = open;
        if (open) {
            this._openness.jump(0);
            this._openness.set(1, 560 * this.pace, Ease.linear, at);
        } else {
            this._openness.jump(0);
        }
    }

    busy(at) {
        return this._openness.busy(at);
    }

    draw(cr, x, y, size, color, at) {
        const u = size / 100;
        const openness = this._openness.get(at);
        const lift = 12 * smooth(0, 0.3, openness);
        const p = Math.max(0, Math.min(1, (openness - 0.22) / 0.78));
        const s = 1.4;
        const swing = 180 * (1 + (s + 1) * (p - 1) ** 3 + s * (p - 1) ** 2);
        const beat = 1 + 0.14 * Math.sin(Math.PI * smooth(0.55, 1, openness));

        cr.save();
        cr.translate(x + size / 2, y + size / 2);
        cr.scale(beat, beat);
        cr.translate(-size / 2, -size / 2);
        rgba(cr, color);
        roundedRect(cr, 14 * u, 46 * u, 72 * u, 50 * u, 12 * u);
        cr.fill();

        const right = 68.5;
        const radius = 18.5;
        const top = 28 - lift;
        const narrow = Math.cos(swing * Math.PI / 180);
        const points = [];
        const add = (px, py) => points.push([(right + (px - right) * narrow) * u, py * u]);
        add(31.5, 56 - 18 * smooth(0, 0.3, openness));
        add(31.5, top);
        for (let i = 1; i < 24; ++i) {
            const a = Math.PI + Math.PI * i / 24;
            add(50 + radius * Math.cos(a), top + radius * Math.sin(a));
        }
        add(right, top);
        add(right, 56);
        cr.setLineWidth(11 * u);
        cr.setLineCap(1 /* round */);
        cr.setLineJoin(1 /* round */);
        polyline(cr, points);
        cr.stroke();
        cr.restore();
    }
}

// FaceGlyph.qml: brackets round a face that looks around, then the rings and
// a tick, or a red shake of the head.
class FaceGlyph {
    constructor(lineWidth = null) {
        this.lineWidth = lineWidth;
        this.color = Theme.textPrimary;
        this.pace = 1;
        this.mode = 'idle';
        this.onSettled = null;
        this._lookStart = 0;
        this._lookFrozen = 0;
        this._lookAmount = new Tween(0);
        this._smile = new Tween(1);
        this._bracket = new Tween(1);
        this._bracketColor = new Tween(Theme.textPrimary);
        this._shown = new Tween(1);
        this._lockOpacity = new Tween(0);
        this._lockScale = new Tween(0.7);
        this._lock = new LockGlyph();
        this._successStart = null;
        this._shakeStart = null;
        this._settle = 0;
    }

    _looking() {
        return this.mode === 'scanning';
    }

    _lookAngle(at) {
        if (!this._looking())
            return this._lookFrozen;
        const period = 2000 * this.pace;
        return 2 * Math.PI * (((at - this._lookStart) % period) / period);
    }

    _breathe(at) {
        if (!this._looking())
            return 0;
        const half = 700 * this.pace;
        const p = ((at - this._lookStart) % (2 * half)) / half;
        return p < 1 ? Ease.inOutSine(p) : 1 - Ease.inOutSine(p - 1);
    }

    setMode(mode, at) {
        if (mode === this.mode)
            return;
        const wasLooking = this._looking();
        if (wasLooking)
            this._lookFrozen = this._lookAngle(at);
        this.mode = mode;
        const pace = this.pace;
        if (this._looking() && !wasLooking)
            this._lookStart = at;

        this._lookAmount.set(this._looking() ? 1 : 0, 350 * pace, Ease.inOutQuad, at);
        this._smile.set(mode === 'failure' ? 0 : 1, 220 * pace, Ease.outQuad, at);
        this._bracket.set(mode === 'tracking' ? 0.9 : 1, 260 * pace, OUT_BACK, at);
        this._bracketColor.set(this._colorFor(mode), 220 * pace, Ease.linear, at);
        this._shown.set(mode === 'lockout' ? 0 : 1, 180 * pace, Ease.linear, at);
        this._lockOpacity.set(mode === 'lockout' ? 1 : 0, 200 * pace, Ease.linear, at);
        this._lockScale.set(mode === 'lockout' ? 1 : 0.7, 260 * pace, OUT_BACK, at);

        this._successStart = null;
        if (this._settle) {
            GLib.source_remove(this._settle);
            this._settle = 0;
        }
        if (mode === 'failure') {
            this._shakeStart = at;
            this.onSettled?.(false);
        } else if (mode === 'success') {
            this._successStart = at;
            this._settle = GLib.timeout_add(GLib.PRIORITY_DEFAULT, 850 * pace, () => {
                this._settle = 0;
                this.onSettled?.(true);
                return GLib.SOURCE_REMOVE;
            });
        }
    }

    _colorFor(mode) {
        if (mode === 'success')
            return Theme.success;
        if (mode === 'failure')
            return Theme.failure;
        return this.color;
    }

    destroy() {
        if (this._settle)
            GLib.source_remove(this._settle);
        this._settle = 0;
    }

    busy(at) {
        return this._looking() || this._lookAmount.busy(at) || this._smile.busy(at) ||
            this._bracket.busy(at) || this._bracketColor.busy(at) || this._shown.busy(at) ||
            this._lockOpacity.busy(at) || this._lockScale.busy(at) ||
            (this._successStart !== null && at - this._successStart < 900 * this.pace) ||
            sequence(FaceGlyph.SHAKE, this._shakeStart, at, this.pace)[1];
    }

    draw(cr, x, y, size, at) {
        const u = size / 100;
        const lw = this.lineWidth ?? size * 0.055;
        const pace = this.pace;
        const color = this._bracketColor.get(at);

        const lookAmount = this._lookAmount.get(at);
        const angle = this._lookAngle(at);
        const yaw = 0.34 * Math.cos(angle) * lookAmount;
        const pitch = 0.2 * Math.sin(angle) * lookAmount;
        const fx = (px, py, pz) => (50 + px * Math.cos(yaw) + pz * Math.sin(yaw)) * u;
        const fy = (px, py, pz) => {
            const depth = pz * Math.cos(yaw) - px * Math.sin(yaw);
            return (50 + py * Math.cos(pitch) - depth * Math.sin(pitch)) * u;
        };

        const t = this._successStart === null ? 0 : Math.min(1, (at - this._successStart) / (850 * pace));
        const tickStart = this._successStart === null ? null : this._successStart + 470 * pace;
        const tick = tickStart === null || at < tickStart ? 0 : Ease.outQuad(Math.min(1, (at - tickStart) / (260 * pace)));
        const [shake] = sequence(FaceGlyph.SHAKE, this._shakeStart, at, pace);

        const faceGone = smooth(0, 0.22, t);
        const ringsIn = smooth(0.04, 0.26, t);
        const turn = FaceGlyph.turned(Math.min(1, t / 0.7));
        const ringSize = 40 * (0.7 + 0.3 * ringsIn);
        const ringA = FaceGlyph.ring(0, -360 * turn, 90 * turn, ringSize);
        const ringB = FaceGlyph.ring(70 + 360 * turn, 30 + 360 * turn, -90 * turn, ringSize);
        const ringBShown = 1 - smooth(0.36, 0.58, t);
        const ringABack = 1 - 0.6 * FaceGlyph.tilt(ringA);
        const ringBBack = 1 - 0.6 * FaceGlyph.tilt(ringB);
        const glow = 1 - smooth(0.42, 0.75, t);
        const onScreen = p => [(50 + p[0]) * u, (50 + p[1]) * u];

        const bracketScale = this._bracket.get(at) - 0.04 * this._breathe(at);
        const c = 50 * u;

        cr.save();
        cr.translate(x + shake * u, y);
        cr.setLineCap(1);
        cr.setLineJoin(1);

        // Brackets, giving way to the rings
        layer(cr, 1 - faceGone, 1 - 0.15 * faceGone, c, c, () => {
            cr.translate(c, c);
            cr.scale(bracketScale, bracketScale);
            cr.translate(-c, -c);
            rgba(cr, color);
            cr.setLineWidth(lw);
            const corner = (sx, sy, lx, ly, qx, qy, cx, cy, ex, ey) => {
                cr.moveTo(sx * u, sy * u);
                cr.lineTo(lx * u, ly * u);
                quadTo(cr, lx * u, ly * u, cx * u, cy * u, qx * u, qy * u);
                cr.lineTo(ex * u, ey * u);
            };
            corner(6, 30, 6, 19, 19, 6, 6, 6, 30, 6);
            corner(70, 6, 81, 6, 94, 19, 94, 6, 94, 30);
            corner(94, 70, 94, 81, 81, 94, 94, 94, 70, 94);
            corner(30, 94, 19, 94, 6, 81, 6, 94, 6, 70);
            cr.stroke();
        });

        // The rings: a soft glow under them, their far halves dimmed, the
        // near halves on top.
        if (t > 0) {
            layer(cr, ringsIn, 1, c, c, () => {
                if (glow > 0) {
                    // MultiEffect's blur, drawn as wider and fainter strokes.
                    for (let i = 0; i < 6; ++i) {
                        cr.setLineWidth(lw * 1.6 + i * lw * 0.9);
                        const a = 0.8 * glow * 0.16;
                        rgba(cr, color, a);
                        polyline(cr, [...ringA.map(onScreen), onScreen(ringA[0])]);
                        cr.stroke();
                        rgba(cr, color, a * ringBShown);
                        polyline(cr, [...ringB.map(onScreen), onScreen(ringB[0])]);
                        cr.stroke();
                    }
                }
                cr.setLineWidth(lw);
                const half = (points, front) => FaceGlyph.half(points, front).map(onScreen);
                rgba(cr, color, ringABack);
                polyline(cr, half(ringA, false));
                cr.stroke();
                rgba(cr, color, ringBBack * ringBShown);
                polyline(cr, half(ringB, false));
                cr.stroke();
                rgba(cr, color, ringBShown);
                polyline(cr, half(ringB, true));
                cr.stroke();
                rgba(cr, color);
                polyline(cr, half(ringA, true));
                cr.stroke();
            });
        }

        // The face itself
        layer(cr, this._shown.get(at) * (1 - faceGone), 1 - 0.2 * faceGone, c, c, () => {
            rgba(cr, color);
            cr.setLineWidth(lw);
            const smile = this._smile.get(at);
            cr.moveTo(fx(-16, -14, 26), fy(-16, -14, 26));
            cr.lineTo(fx(-16, -5, 26), fy(-16, -5, 26));
            cr.moveTo(fx(16, -14, 26), fy(16, -14, 26));
            cr.lineTo(fx(16, -5, 26), fy(16, -5, 26));
            cr.moveTo(fx(0, -13, 29), fy(0, -13, 29));
            cr.lineTo(fx(0, 6, 37), fy(0, 6, 37));
            quadTo(cr, fx(0, 6, 37), fy(0, 6, 37), fx(0, 11, 35), fy(0, 11, 35), fx(-5, 11, 31), fy(-5, 11, 31));
            cr.moveTo(fx(-15, 20, 21), fy(-15, 20, 21));
            quadTo(cr, fx(-15, 20, 21), fy(-15, 20, 21), fx(0, 20 + 11 * smile, 24), fy(0, 20 + 11 * smile, 24),
                fx(15, 20, 21), fy(15, 20, 21));
            cr.stroke();
        });

        // The tick, drawn inside the ring as it comes to rest
        if (tick > 0.001) {
            layer(cr, Math.min(1, tick * 8), 0.75, c, c, () => {
                cr.translate(1.5 * u, 0);
                rgba(cr, Theme.success);
                cr.setLineWidth(lw / 0.75);
                const a = [28 * u, 52 * u];
                const b = [43 * u, 67 * u];
                const e = [73 * u, 35 * u];
                const lerp = (p, q, k) => [p[0] + (q[0] - p[0]) * k, p[1] + (q[1] - p[1]) * k];
                const split = 0.33;
                polyline(cr, tick <= split ? [a, lerp(a, b, tick / split)]
                    : [a, b, lerp(b, e, (tick - split) / (1 - split))]);
                cr.stroke();
            });
        }

        // A lock instead of the face, after too many tries
        const lockSize = size * 0.42;
        layer(cr, this._lockOpacity.get(at), this._lockScale.get(at), c, c, () => {
            this._lock.draw(cr, c - lockSize / 2, c - lockSize / 2, lockSize, this.color, at);
        });

        cr.restore();
    }

    // How far the rings have turned at p: speeding up from rest, fastest a
    // quarter of the way in, then settling softly, facing forward.
    static turned(p) {
        return p * p * (10 - p * (20 - p * (15 - 4 * p)));
    }

    // A ring turned in 3D (degrees about x, then y, then z).
    static ring(ax, ay, az, radius) {
        const d = Math.PI / 180;
        const ca = Math.cos(ax * d), sa = Math.sin(ax * d);
        const cb = Math.cos(ay * d), sb = Math.sin(ay * d);
        const cg = Math.cos(az * d), sg = Math.sin(az * d);
        const points = [];
        for (let i = 0; i < 64; ++i) {
            const phi = 2 * Math.PI * i / 64;
            let px = radius * Math.cos(phi);
            let py = radius * Math.sin(phi) * ca;
            let pz = radius * Math.sin(phi) * sa;
            const x1 = px * cb + pz * sb;
            pz = pz * cb - px * sb;
            px = x1;
            const x2 = px * cg - py * sg;
            py = px * sg + py * cg;
            px = x2;
            points.push([px, py, pz]);
        }
        return points;
    }

    static tilt(points) {
        let most = 0;
        for (const p of points)
            most = Math.max(most, Math.abs(p[2]));
        return most / Math.max(1, Math.hypot(points[0][0], points[0][1], points[0][2]));
    }

    // The half of a ring in front of the middle, or behind it, in one piece
    // and reaching one point into the other half so the two meet.
    static half(points, front) {
        const n = points.length;
        const inFront = i => points[(i + n) % n][2] >= -1e-6;
        let start = -1;
        for (let i = 0; i < n; ++i) {
            if (inFront(i) === front && inFront(i - 1) !== front) {
                start = i;
                break;
            }
        }
        const out = [];
        if (start < 0) {
            if (inFront(0) === front) {
                for (let i = 0; i <= n; ++i)
                    out.push(points[i % n]);
            }
            return out;
        }
        out.push(points[(start + n - 1) % n]);
        let i = start;
        while (inFront(i) === front && i < start + n) {
            out.push(points[i % n]);
            ++i;
        }
        out.push(points[i % n]);
        return out;
    }
}
FaceGlyph.SHAKE = [
    [-9, 55, Ease.outQuad], [8, 70, Ease.inOutQuad], [-6, 65, Ease.inOutQuad],
    [4, 60, Ease.inOutQuad], [-2, 55, Ease.inOutQuad], [0, 50, Ease.outQuad],
];

const PILL_SHAKE = [
    [-10, 55, Ease.outQuad], [9, 70, Ease.linear], [-6, 65, Ease.linear],
    [4, 60, Ease.linear], [0, 55, Ease.outQuad],
];

// Bubble.qml: the island that slides down, opens up, shows the face and
// closes again.
class Bubble {
    constructor() {
        this.actor = new St.DrawingArea({reactive: false, visible: false});
        this.actor.connect('repaint', area => this._repaint(area));
        this._timeline = new Clutter.Timeline({actor: this.actor, duration: 1000, repeat_count: -1});
        this._timeline.connect('new-frame', () => this._frame());

        this.phase = 'hidden';
        this.message = '';
        this.faceSeen = false;
        this.style = 'full';
        this.pace = 1;
        this._shownPhase = 'idle';
        this._positioned = false;
        this._expanded = false;
        this._opening = false;
        this._timers = {};

        this._width = new Tween(Theme.closedWidth);
        this._height = new Tween(Theme.closedHeight);
        this._y = new Tween(-Theme.closedHeight - 20);
        this._shadow = new Tween(0);
        this._fullOpacity = new Tween(0);
        this._smallOpacity = new Tween(0);
        this._glyphY = new Tween(40);
        this._messageOpacity = new Tween(0);
        this._pulseStart = null;
        this._settle = new Tween(0);
        this._pillShakeStart = null;

        this._face = new FaceGlyph();
        this._smallFace = new FaceGlyph(2.4);
        this._smallFace.onSettled = ok => {
            if (!ok)
                this._pillShakeStart = now();
        };
        this._lock = new LockGlyph();
    }

    destroy() {
        for (const id of Object.values(this._timers))
            GLib.source_remove(id);
        this._timers = {};
        this._face.destroy();
        this._smallFace.destroy();
        this._timeline.stop();
        this.actor.destroy();
    }

    setState(state) {
        const at = now();
        const wasOpen = this.phase !== 'hidden';
        this.phase = state.phase ?? 'hidden';
        this.message = state.message ?? '';
        this.faceSeen = !!state.faceSeen;
        this.style = state.style === 'minimal' ? 'minimal' : 'full';
        const pace = state.pace ?? 1;
        if (pace !== this.pace) {
            this.pace = pace;
            this._face.pace = this._smallFace.pace = this._lock.pace = pace;
        }

        if (this.phase !== 'hidden')
            this._shownPhase = this.phase;
        const glyphMode = this._shownPhase === 'scanning'
            ? this.faceSeen ? 'tracking' : 'scanning'
            : this._shownPhase;
        this._face.setMode(glyphMode, at);
        this._smallFace.setMode(glyphMode === 'lockout' ? 'failure' : glyphMode, at);
        this._lock.setOpen(this._shownPhase === 'success', at);

        const hasMessage = this.message.length > 0;
        this._glyphY.set(hasMessage ? 28 : 40, 220 * this.pace, Ease.outCubic, at);
        this._messageOpacity.set(hasMessage ? 1 : 0, 200 * this.pace, Ease.linear, at);
        this._updatePulse(at);

        const wantOpen = this.phase !== 'hidden';
        if (wantOpen !== wasOpen)
            this._choreograph(wantOpen, at);
        this._wake();
    }

    _timer(name, ms, callback) {
        if (this._timers[name])
            GLib.source_remove(this._timers[name]);
        this._timers[name] = GLib.timeout_add(GLib.PRIORITY_DEFAULT, Math.max(0, Math.round(ms)), () => {
            delete this._timers[name];
            callback(now());
            this._wake();
            return GLib.SOURCE_REMOVE;
        });
    }

    _cancel(name) {
        if (this._timers[name]) {
            GLib.source_remove(this._timers[name]);
            delete this._timers[name];
        }
    }

    _choreograph(wantOpen, at) {
        if (wantOpen) {
            this._opening = true;
            this._cancel('slideOut');
            this._cancel('closeDone');
            this._setPositioned(true, at);
            if (!this._expanded)
                this._timer('expand', Theme.expandDelay * this.pace, t => this._setExpanded(true, t));
            this.actor.show();
        } else {
            this._opening = false;
            this._cancel('expand');
            this._setExpanded(false, at);
            this._timer('slideOut', Theme.slideOutDelay * this.pace, t => {
                this._setPositioned(false, t);
                this._timer('closeDone', Theme.slideOutDuration * this.pace + 60, () => {
                    this.actor.hide();
                    this._timeline.stop();
                });
            });
        }
    }

    _setPositioned(positioned, at) {
        this._positioned = positioned;
        this._y.set(positioned ? Theme.topGap : -Theme.closedHeight - 20,
            (this._opening ? Theme.slideInDuration : Theme.slideOutDuration) * this.pace,
            this._opening ? Ease.outCubic : Ease.inCubic, at);
    }

    _setExpanded(expanded, at) {
        this._expanded = expanded;
        const minimal = this.style === 'minimal';
        const w = expanded ? minimal ? Theme.minimalWidth : Theme.openWidth : Theme.closedWidth;
        const h = expanded ? minimal ? Theme.minimalHeight : Theme.openHeight : Theme.closedHeight;
        const duration = (this._opening ? Theme.growDuration : Theme.shrinkDuration) * this.pace;
        // Growing overshoots sideways more than down, so the bottom edge does
        // not sag.
        this._width.set(w, duration, this._opening ? Ease.outBack(1.6) : Ease.outCubic, at);
        this._height.set(h, duration, this._opening ? Ease.outBack(0.9) : Ease.outCubic, at);
        this._shadow.set(expanded ? 0.35 : 0, (this._opening ? 200 : 120) * this.pace, Ease.linear, at);
        this._fullOpacity.set(expanded ? 1 : 0, (this._opening ? 260 : 120) * this.pace, Ease.linear, at);
        this._smallOpacity.set(expanded ? 1 : 0, (this._opening ? 200 : 120) * this.pace, Ease.linear, at);
        this._updatePulse(at);
    }

    // The breathing while it scans: 1500 ms a breath, settling when it stops.
    _updatePulse(at) {
        const running = this.phase === 'scanning' && this._expanded;
        if (running && this._pulseStart === null) {
            this._pulseStart = at;
        } else if (!running && this._pulseStart !== null) {
            this._settle.jump(this._pulse(at));
            this._settle.set(0, 200 * this.pace, Ease.outQuad, at);
            this._pulseStart = null;
        }
    }

    _pulse(at) {
        if (this._pulseStart === null)
            return this._settle.get(at);
        const p = this.pace;
        let t = (at - this._pulseStart) % (1500 * p);
        if (t < 600 * p)
            return 0;
        t -= 600 * p;
        if (t < 400 * p)
            return Ease.inOutQuad(t / (400 * p));
        t -= 400 * p;
        if (t < 50 * p)
            return 1;
        t -= 50 * p;
        if (t < 400 * p)
            return 1 - Ease.inOutQuad(t / (400 * p));
        return 0;
    }

    _wake() {
        if (!this.actor.visible)
            return;
        if (!this._timeline.is_playing())
            this._timeline.start();
        this.actor.queue_repaint();
    }

    _frame() {
        const at = now();
        this.actor.queue_repaint();
        const busy = this.phase !== 'hidden' || Object.keys(this._timers).length > 0 ||
            this._width.busy(at) || this._y.busy(at) || this._face.busy(at) || this._smallFace.busy(at);
        if (!busy)
            this._timeline.stop();
    }

    _repaint(area) {
        const cr = area.get_context();
        const scale = St.ThemeContext.get_for_stage(global.stage).scale_factor;
        const at = now();
        cr.save();
        cr.setOperator(0 /* clear */);
        cr.paint();
        cr.restore();
        cr.scale(scale, scale);
        try {
            this._draw(cr, at);
        } catch (e) {
            logError(e, 'face-unlock bubble');
        }
        cr.$dispose();
    }

    _draw(cr, at) {
        const minimal = this.style === 'minimal';
        const w = this._width.get(at);
        const h = this._height.get(at);
        const [pillShake] = sequence(PILL_SHAKE, this._pillShakeStart, at, this.pace);
        const x = (WIDTH - w) / 2 + pillShake;
        const y = this._y.get(at);
        const radius = minimal ? h / 2 : Math.min(Theme.openRadius, h / 2);

        // The shadow: MultiEffect's soft one, as rings that fade outwards.
        const shadow = this._shadow.get(at);
        if (shadow > 0.001) {
            for (let i = 8; i >= 1; --i) {
                const grow = i * 2;
                rgba(cr, [0, 0, 0], shadow * 0.18 * (1 - i / 9));
                roundedRect(cr, x - grow, y + 3 - grow, w + 2 * grow, h + 2 * grow, radius + grow);
                cr.fill();
            }
        }

        rgba(cr, Theme.panel);
        roundedRect(cr, x, y, w, h, radius);
        cr.fill();

        const pulse = this._pulse(at);
        if (!minimal) {
            // The face and a line of text, laid out at the open size and
            // scaled with the island.
            const fit = Math.min(w / Theme.openWidth, h / Theme.openHeight);
            const ox = x + w / 2 - Theme.openWidth / 2;
            const oy = y + h / 2 - Theme.openHeight / 2;
            const cx = ox + Theme.openWidth / 2;
            const cy = oy + Theme.openHeight / 2;
            layer(cr, this._fullOpacity.get(at), fit, cx, cy, () => {
                layer(cr, 1 - 0.35 * pulse, 1 - 0.03 * pulse, cx, cy, () => {
                    this._face.draw(cr, ox + 40, oy + this._glyphY.get(at), 100, at);
                    this._drawMessage(cr, ox, oy, at);
                });
            });
        } else {
            layer(cr, this._smallOpacity.get(at), 1, x + w / 2, y + h / 2, () => {
                const failed = this._shownPhase === 'failure' || this._shownPhase === 'lockout';
                this._lock.draw(cr, x + 16, y + (h - 18) / 2, 18, failed ? Theme.failure : Theme.textPrimary, at);
                const fx = x + w - 14 - 24;
                const fy = y + (h - 24) / 2;
                layer(cr, 1 - 0.35 * pulse, 1 - 0.03 * pulse, fx + 12, fy + 12, () => {
                    this._smallFace.draw(cr, fx, fy, 24, at);
                });
            });
        }
    }

    _drawMessage(cr, ox, oy, at) {
        const opacity = this._messageOpacity.get(at);
        if (opacity <= 0.001 || !this.message)
            return;
        const layout = PangoCairo.create_layout(cr);
        const font = Pango.FontDescription.from_string(Bubble._fontName());
        font.set_absolute_size(13 * Pango.SCALE);
        font.set_weight(Pango.Weight.MEDIUM);
        layout.set_font_description(font);
        layout.set_width((Theme.openWidth - 32) * Pango.SCALE);
        layout.set_ellipsize(Pango.EllipsizeMode.END);
        layout.set_alignment(Pango.Alignment.CENTER);
        layout.set_text(this.message, -1);
        const [, logical] = layout.get_pixel_extents();
        const failed = this._shownPhase === 'failure' || this._shownPhase === 'lockout';
        rgba(cr, failed ? Theme.textDetail : Theme.textSecondary, opacity);
        cr.moveTo(ox + 16, oy + Theme.openHeight - 20 - logical.height);
        PangoCairo.show_layout(cr, layout);
    }

    static _fontName() {
        if (!Bubble._font) {
            const name = new Gio.Settings({schema_id: 'org.gnome.desktop.interface'}).get_string('font-name');
            Bubble._font = name.replace(/\s+[\d.]+$/, '');
        }
        return Bubble._font;
    }
}

export default class FaceUnlockExtension extends Extension {
    enable() {
        this._bubble = new Bubble();
        const ui = Main.layoutManager.uiGroup;
        ui.add_child(this._bubble.actor);
        // Above everything, the lock screen and its dialogs included.
        this._raise = ui.connect('child-added', () => ui.set_child_above_sibling(this._bubble.actor, null));
        this._monitors = Main.layoutManager.connect('monitors-changed', () => this._place());
        this._scale = St.ThemeContext.get_for_stage(global.stage).connect('notify::scale-factor', () => this._place());
        this._place();

        this._watch = Gio.bus_watch_name(Gio.BusType.SESSION, BUS_NAME, Gio.BusNameWatcherFlags.NONE,
            () => this._connect(), () => this._disconnect());
    }

    disable() {
        Gio.bus_unwatch_name(this._watch);
        this._disconnect();
        Main.layoutManager.uiGroup.disconnect(this._raise);
        Main.layoutManager.disconnect(this._monitors);
        St.ThemeContext.get_for_stage(global.stage).disconnect(this._scale);
        this._bubble.destroy();
        this._bubble = null;
    }

    _place() {
        const monitor = Main.layoutManager.primaryMonitor;
        if (!monitor)
            return;
        const scale = St.ThemeContext.get_for_stage(global.stage).scale_factor;
        this._bubble.actor.set_size(WIDTH * scale, HEIGHT * scale);
        this._bubble.actor.set_position(monitor.x + Math.round((monitor.width - WIDTH * scale) / 2), monitor.y);
        Main.layoutManager.uiGroup.set_child_above_sibling(this._bubble.actor, null);
    }

    _connect() {
        this._disconnect();
        const unpack = state => {
            const out = {};
            for (const [key, value] of Object.entries(state))
                out[key] = value instanceof GLib.Variant ? value.recursiveUnpack() : value;
            return out;
        };
        this._proxy = new BubbleProxy(Gio.DBus.session, BUS_NAME, OBJECT_PATH, (proxy, error) => {
            if (error) {
                logError(error, 'face-unlock');
                return;
            }
            this._signal = proxy.connectSignal('StateChanged', (p, sender, [state]) => {
                this._place();
                this._bubble?.setState(unpack(state));
            });
            proxy.GetStateRemote(([state], getError) => {
                if (!getError)
                    this._bubble?.setState(unpack(state));
            });
        }, null, Gio.DBusProxyFlags.DO_NOT_AUTO_START);
    }

    _disconnect() {
        if (this._proxy && this._signal)
            this._proxy.disconnectSignal(this._signal);
        this._proxy = null;
        this._signal = null;
        this._bubble?.setState({phase: 'hidden'});
    }
}
