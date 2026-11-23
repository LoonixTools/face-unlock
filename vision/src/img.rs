//! The little image processing the checks need, done the way OpenCV does it
//! where the numbers depend on it: grey and YCrCb with OpenCV's fixed-point
//! weights, area resizing, bilinear warps, the 5x5 Gaussian, Canny, contours.

/// A picture in BGR order, three bytes a pixel, rows without padding.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Bgr {
    pub w: usize,
    pub h: usize,
    pub data: Vec<u8>,
}

/// One channel of bytes.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Grey {
    pub w: usize,
    pub h: usize,
    pub data: Vec<u8>,
}

#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct Pt {
    pub x: f32,
    pub y: f32,
}

impl Pt {
    pub const fn new(x: f32, y: f32) -> Pt {
        Pt { x, y }
    }
    pub fn norm(self) -> f32 {
        (self.x * self.x + self.y * self.y).sqrt()
    }
}

impl std::ops::Add for Pt {
    type Output = Pt;
    fn add(self, o: Pt) -> Pt {
        Pt::new(self.x + o.x, self.y + o.y)
    }
}
impl std::ops::Sub for Pt {
    type Output = Pt;
    fn sub(self, o: Pt) -> Pt {
        Pt::new(self.x - o.x, self.y - o.y)
    }
}
impl std::ops::Mul<f32> for Pt {
    type Output = Pt;
    fn mul(self, s: f32) -> Pt {
        Pt::new(self.x * s, self.y * s)
    }
}

/// An integer rectangle, like cv::Rect.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Rect {
    pub x: i32,
    pub y: i32,
    pub w: i32,
    pub h: i32,
}

impl Rect {
    pub fn intersect(self, o: Rect) -> Rect {
        let x = self.x.max(o.x);
        let y = self.y.max(o.y);
        let r = (self.x + self.w).min(o.x + o.w);
        let b = (self.y + self.h).min(o.y + o.h);
        if r <= x || b <= y { Rect::default() } else { Rect { x, y, w: r - x, h: b - y } }
    }
}

impl Bgr {
    pub fn new(w: usize, h: usize) -> Bgr {
        Bgr { w, h, data: vec![0; w * h * 3] }
    }

    pub fn px(&self, x: usize, y: usize) -> [u8; 3] {
        let i = (y * self.w + x) * 3;
        [self.data[i], self.data[i + 1], self.data[i + 2]]
    }

    pub fn crop(&self, r: Rect) -> Bgr {
        let mut out = Bgr::new(r.w as usize, r.h as usize);
        for y in 0..r.h as usize {
            let src = ((r.y as usize + y) * self.w + r.x as usize) * 3;
            let dst = y * r.w as usize * 3;
            out.data[dst..dst + r.w as usize * 3].copy_from_slice(&self.data[src..src + r.w as usize * 3]);
        }
        out
    }

    /// Mirrored left to right.
    pub fn flip(&self) -> Bgr {
        let mut out = Bgr::new(self.w, self.h);
        for y in 0..self.h {
            for x in 0..self.w {
                let s = (y * self.w + x) * 3;
                let d = (y * self.w + (self.w - 1 - x)) * 3;
                out.data[d..d + 3].copy_from_slice(&self.data[s..s + 3]);
            }
        }
        out
    }

    pub fn grey(&self) -> Grey {
        // OpenCV's fixed-point weights for BGR to grey.
        let data = self.data.as_chunks::<3>().0.iter().map(|p| ((p[0] as u32 * 1868 + p[1] as u32 * 9617 + p[2] as u32 * 4899 + (1 << 13)) >> 14) as u8).collect();
        Grey { w: self.w, h: self.h, data }
    }

    /// YCrCb, the three channels in that order, with OpenCV's weights.
    pub fn ycrcb(&self) -> Bgr {
        let data = self
            .data
            .as_chunks::<3>()
            .0
            .iter()
            .flat_map(|p| {
                let (b, g, r) = (p[0] as i32, p[1] as i32, p[2] as i32);
                let y = (b * 1868 + g * 9617 + r * 4899 + (1 << 13)) >> 14;
                let cr = ((r - y) * 11682 + (128 << 14) + (1 << 13)) >> 14;
                let cb = ((b - y) * 9241 + (128 << 14) + (1 << 13)) >> 14;
                [y.clamp(0, 255) as u8, cr.clamp(0, 255) as u8, cb.clamp(0, 255) as u8]
            })
            .collect();
        Bgr { w: self.w, h: self.h, data }
    }

    /// Area resampling, as cv::INTER_AREA does for shrinking.
    pub fn resize_area(&self, w: usize, h: usize) -> Bgr {
        let data = resize_area(&self.data, self.w, self.h, 3, w, h);
        Bgr { w, h, data }
    }
}

impl Grey {
    pub fn new(w: usize, h: usize) -> Grey {
        Grey { w, h, data: vec![0; w * h] }
    }

    pub fn at(&self, x: usize, y: usize) -> u8 {
        self.data[y * self.w + x]
    }

    pub fn resize_area(&self, w: usize, h: usize) -> Grey {
        Grey { w, h, data: resize_area(&self.data, self.w, self.h, 1, w, h) }
    }

    /// The mean of a rectangle inside the picture.
    pub fn mean(&self, r: Rect) -> f32 {
        let mut sum = 0u64;
        for y in r.y..r.y + r.h {
            for x in r.x..r.x + r.w {
                sum += self.at(x as usize, y as usize) as u64;
            }
        }
        sum as f32 / (r.w * r.h).max(1) as f32
    }
}

/// Each output pixel is the area-weighted mean of the input pixels it covers.
fn resize_area(src: &[u8], sw: usize, sh: usize, ch: usize, dw: usize, dh: usize) -> Vec<u8> {
    // Weights of the source cells along one axis for every output cell.
    fn spans(s: usize, d: usize) -> Vec<Vec<(usize, f32)>> {
        let scale = s as f64 / d as f64;
        (0..d)
            .map(|i| {
                let a = i as f64 * scale;
                let b = ((i + 1) as f64 * scale).min(s as f64);
                let mut out = Vec::new();
                let mut k = a.floor() as usize;
                while (k as f64) < b && k < s {
                    let lo = a.max(k as f64);
                    let hi = b.min(k as f64 + 1.0);
                    if hi > lo {
                        out.push((k, ((hi - lo) / (b - a)) as f32));
                    }
                    k += 1;
                }
                out
            })
            .collect()
    }
    let xs = spans(sw, dw);
    let ys = spans(sh, dh);
    // Rows first, into floats, then columns.
    let mut rows = vec![0f32; dw * sh * ch];
    for y in 0..sh {
        for (x, span) in xs.iter().enumerate() {
            for c in 0..ch {
                let mut v = 0f32;
                for &(k, wt) in span {
                    v += src[(y * sw + k) * ch + c] as f32 * wt;
                }
                rows[(y * dw + x) * ch + c] = v;
            }
        }
    }
    let mut out = vec![0u8; dw * dh * ch];
    for (y, span) in ys.iter().enumerate() {
        for x in 0..dw {
            for c in 0..ch {
                let mut v = 0f32;
                for &(k, wt) in span {
                    v += rows[(k * dw + x) * ch + c] * wt;
                }
                out[(y * dw + x) * ch + c] = v.round().clamp(0.0, 255.0) as u8;
            }
        }
    }
    out
}

/// A 2x3 affine matrix, as cv::warpAffine takes it.
pub type Affine = [[f64; 3]; 2];

/// cv::getRotationMatrix2D: about `centre`, `angle` degrees, scaled.
pub fn rotation(centre: Pt, angle: f64, scale: f64) -> Affine {
    let a = scale * angle.to_radians().cos();
    let b = scale * angle.to_radians().sin();
    let (cx, cy) = (centre.x as f64, centre.y as f64);
    [[a, b, (1.0 - a) * cx - b * cy], [-b, a, b * cx + (1.0 - a) * cy]]
}

fn invert(m: &Affine) -> Affine {
    let det = m[0][0] * m[1][1] - m[0][1] * m[1][0];
    let d = if det != 0.0 { 1.0 / det } else { 0.0 };
    let a = m[1][1] * d;
    let b = -m[0][1] * d;
    let c = -m[1][0] * d;
    let e = m[0][0] * d;
    [[a, b, -a * m[0][2] - b * m[1][2]], [c, e, -c * m[0][2] - e * m[1][2]]]
}

#[derive(Clone, Copy, PartialEq, Eq)]
pub enum Border {
    /// Black outside.
    Constant,
    /// The edge pixel repeated.
    Replicate,
}

/// Bilinear sampling of channel `c` of an image with `ch` channels.
#[inline]
#[allow(clippy::too_many_arguments)]
fn sample(src: &[u8], w: usize, h: usize, ch: usize, c: usize, x: f64, y: f64, border: Border) -> f64 {
    let x0 = x.floor();
    let y0 = y.floor();
    let fx = x - x0;
    let fy = y - y0;
    let get = |xi: i64, yi: i64| -> f64 {
        let (xi, yi) = match border {
            Border::Replicate => (xi.clamp(0, w as i64 - 1), yi.clamp(0, h as i64 - 1)),
            Border::Constant => {
                if xi < 0 || yi < 0 || xi >= w as i64 || yi >= h as i64 {
                    return 0.0;
                }
                (xi, yi)
            }
        };
        src[(yi as usize * w + xi as usize) * ch + c] as f64
    };
    let (x0, y0) = (x0 as i64, y0 as i64);
    let top = get(x0, y0) * (1.0 - fx) + get(x0 + 1, y0) * fx;
    let bottom = get(x0, y0 + 1) * (1.0 - fx) + get(x0 + 1, y0 + 1) * fx;
    top * (1.0 - fy) + bottom * fy
}

#[allow(clippy::too_many_arguments)]
fn warp(src: &[u8], w: usize, h: usize, ch: usize, m: &Affine, dw: usize, dh: usize, border: Border) -> Vec<u8> {
    let inv = invert(m);
    let mut out = vec![0u8; dw * dh * ch];
    for y in 0..dh {
        for x in 0..dw {
            let sx = inv[0][0] * x as f64 + inv[0][1] * y as f64 + inv[0][2];
            let sy = inv[1][0] * x as f64 + inv[1][1] * y as f64 + inv[1][2];
            // Wholly outside: the border, without sampling.
            if border == Border::Constant && (sx <= -1.0 || sy <= -1.0 || sx >= w as f64 || sy >= h as f64) {
                continue;
            }
            for c in 0..ch {
                out[(y * dw + x) * ch + c] = sample(src, w, h, ch, c, sx, sy, border).round().clamp(0.0, 255.0) as u8;
            }
        }
    }
    out
}

impl Bgr {
    pub fn warp(&self, m: &Affine, w: usize, h: usize, border: Border) -> Bgr {
        Bgr { w, h, data: warp(&self.data, self.w, self.h, 3, m, w, h, border) }
    }
}

impl Grey {
    pub fn warp(&self, m: &Affine, w: usize, h: usize, border: Border) -> Grey {
        Grey { w, h, data: warp(&self.data, self.w, self.h, 1, m, w, h, border) }
    }

    /// The 5x5 Gaussian OpenCV uses for sigma 0: 1 4 6 4 1, both ways.
    pub fn gaussian5(&self) -> Grey {
        let (w, h) = (self.w, self.h);
        let k = [1u32, 4, 6, 4, 1];
        let reflect = |i: i64, n: usize| -> usize {
            // BORDER_REFLECT_101: gfedcb|abcdefgh|gfedcba
            let n = n as i64;
            if n == 1 {
                return 0;
            }
            let mut i = i;
            while i < 0 || i >= n {
                if i < 0 {
                    i = -i;
                }
                if i >= n {
                    i = 2 * n - 2 - i;
                }
            }
            i as usize
        };
        let mut rows = vec![0u32; w * h];
        for y in 0..h {
            for x in 0..w {
                let mut s = 0;
                for (j, kv) in k.iter().enumerate() {
                    s += kv * self.data[y * w + reflect(x as i64 + j as i64 - 2, w)] as u32;
                }
                rows[y * w + x] = s;
            }
        }
        let mut out = Grey::new(w, h);
        for y in 0..h {
            for x in 0..w {
                let mut s = 0;
                for (j, kv) in k.iter().enumerate() {
                    s += kv * rows[reflect(y as i64 + j as i64 - 2, h) * w + x];
                }
                out.data[y * w + x] = ((s + 128) >> 8) as u8;
            }
        }
        out
    }

    /// The variance of the Laplacian (the 4-neighbour kernel, border
    /// reflected), which says how sharp the picture is.
    pub fn laplacian_variance(&self) -> f32 {
        let (w, h) = (self.w as i64, self.h as i64);
        let r = |i: i64, n: i64| -> i64 {
            if n == 1 {
                0
            } else if i < 0 {
                -i
            } else if i >= n {
                2 * n - 2 - i
            } else {
                i
            }
        };
        let at = |x: i64, y: i64| self.data[(r(y, h) * w + r(x, w)) as usize] as f64;
        let (mut sum, mut sq) = (0f64, 0f64);
        for y in 0..h {
            for x in 0..w {
                let v = at(x - 1, y) + at(x + 1, y) + at(x, y - 1) + at(x, y + 1) - 4.0 * at(x, y);
                sum += v;
                sq += v * v;
            }
        }
        let n = (w * h).max(1) as f64;
        let mean = sum / n;
        ((sq / n) - mean * mean).max(0.0) as f32
    }

    /// The mean of each row of a rectangle.
    pub fn row_means(&self, r: Rect) -> Vec<f32> {
        (r.y..r.y + r.h)
            .map(|y| {
                let s: u32 = (r.x..r.x + r.w).map(|x| self.at(x as usize, y as usize) as u32).sum();
                s as f32 / r.w.max(1) as f32
            })
            .collect()
    }

    /// Canny with a 3x3 Sobel and the L1 gradient, as cv::Canny does by
    /// default. 255 on an edge, 0 elsewhere.
    pub fn canny(&self, low: i32, high: i32) -> Grey {
        let (w, h) = (self.w, self.h);
        let at = |x: i64, y: i64| self.data[(y.clamp(0, h as i64 - 1) as usize) * w + x.clamp(0, w as i64 - 1) as usize] as i32;
        let mut dx = vec![0i32; w * h];
        let mut dy = vec![0i32; w * h];
        let mut mag = vec![0i32; w * h];
        for y in 0..h as i64 {
            for x in 0..w as i64 {
                let gx = (at(x + 1, y - 1) + 2 * at(x + 1, y) + at(x + 1, y + 1)) - (at(x - 1, y - 1) + 2 * at(x - 1, y) + at(x - 1, y + 1));
                let gy = (at(x - 1, y + 1) + 2 * at(x, y + 1) + at(x + 1, y + 1)) - (at(x - 1, y - 1) + 2 * at(x, y - 1) + at(x + 1, y - 1));
                let i = y as usize * w + x as usize;
                dx[i] = gx;
                dy[i] = gy;
                mag[i] = gx.abs() + gy.abs();
            }
        }
        // 0 none, 1 weak, 2 strong.
        let mut state = vec![0u8; w * h];
        const SHIFT: i64 = 15;
        let tg22 = (0.414_213_562_373_095_f64 * (1 << SHIFT) as f64 + 0.5) as i64;
        let m = |x: i64, y: i64| -> i32 {
            if x < 0 || y < 0 || x >= w as i64 || y >= h as i64 { 0 } else { mag[y as usize * w + x as usize] }
        };
        let mut stack = Vec::new();
        for y in 0..h as i64 {
            for x in 0..w as i64 {
                let i = y as usize * w + x as usize;
                let v = mag[i];
                if v <= low {
                    continue;
                }
                let xs = dx[i].abs() as i64;
                let ys = dy[i].abs() as i64;
                let tg22x = xs * tg22;
                let ys_s = ys << SHIFT;
                let keep = if ys_s < tg22x {
                    v > m(x - 1, y) && v >= m(x + 1, y)
                } else {
                    let tg67x = tg22x + (xs << (SHIFT + 1));
                    if ys_s > tg67x {
                        v > m(x, y - 1) && v >= m(x, y + 1)
                    } else {
                        let s: i64 = if (dx[i] ^ dy[i]) < 0 { -1 } else { 1 };
                        v > m(x - s, y - 1) && v > m(x + s, y + 1)
                    }
                };
                if keep {
                    if v > high {
                        state[i] = 2;
                        stack.push(i);
                    } else {
                        state[i] = 1;
                    }
                }
            }
        }
        // Weak edges connected to a strong one become strong.
        while let Some(i) = stack.pop() {
            let (x, y) = ((i % w) as i64, (i / w) as i64);
            for oy in -1..=1 {
                for ox in -1..=1 {
                    let (nx, ny) = (x + ox, y + oy);
                    if nx < 0 || ny < 0 || nx >= w as i64 || ny >= h as i64 {
                        continue;
                    }
                    let j = ny as usize * w + nx as usize;
                    if state[j] == 1 {
                        state[j] = 2;
                        stack.push(j);
                    }
                }
            }
        }
        Grey { w, h, data: state.into_iter().map(|s| if s == 2 { 255 } else { 0 }).collect() }
    }

    /// One pass of 3x3 dilation.
    pub fn dilate3(&self) -> Grey {
        let (w, h) = (self.w, self.h);
        let mut out = Grey::new(w, h);
        for y in 0..h {
            for x in 0..w {
                let mut v = 0;
                for yy in y.saturating_sub(1)..(y + 2).min(h) {
                    for xx in x.saturating_sub(1)..(x + 2).min(w) {
                        v = v.max(self.data[yy * w + xx]);
                    }
                }
                out.data[y * w + x] = v;
            }
        }
        out
    }
}

// ---------------------------------------------------------------------------
// Contours
// ---------------------------------------------------------------------------

pub type Contour = Vec<(i32, i32)>;

/// Every border in a binary picture, outer ones and those of holes, with only
/// the corners of straight runs kept: cv::findContours with RETR_LIST and
/// CHAIN_APPROX_SIMPLE (Suzuki and Abe's border following).
pub fn find_contours(img: &Grey) -> Vec<Contour> {
    // One pixel of background around the picture, as OpenCV does.
    let (w, h) = (img.w as i64 + 2, img.h as i64 + 2);
    let mut f = vec![0i32; (w * h) as usize];
    for y in 0..img.h {
        for x in 0..img.w {
            if img.data[y * img.w + x] != 0 {
                f[((y as i64 + 1) * w + x as i64 + 1) as usize] = 1;
            }
        }
    }
    // Clockwise from east, in image coordinates (y down).
    const DIRS: [(i64, i64); 8] = [(1, 0), (1, -1), (0, -1), (-1, -1), (-1, 0), (-1, 1), (0, 1), (1, 1)];
    let idx = |x: i64, y: i64| (y * w + x) as usize;
    let dir_of = |dx: i64, dy: i64| DIRS.iter().position(|&d| d == (dx, dy)).unwrap_or(0);
    let mut nbd = 1i32;
    let mut out = Vec::new();
    for y in 1..h - 1 {
        for x in 1..w - 1 {
            let v = f[idx(x, y)];
            let (start_from, outer) = if v == 1 && f[idx(x - 1, y)] == 0 {
                ((x - 1, y), true)
            } else if v >= 1 && f[idx(x + 1, y)] == 0 {
                ((x + 1, y), false)
            } else {
                continue;
            };
            let _ = outer;
            nbd += 1;
            // Step 3.1: look clockwise around (x, y) from start_from for a
            // pixel that is not background.
            let d0 = dir_of(start_from.0 - x, start_from.1 - y);
            let mut found = None;
            for k in 0..8 {
                // Clockwise in image coordinates is counting down.
                let d = (d0 + 8 - k) % 8;
                let (nx, ny) = (x + DIRS[d].0, y + DIRS[d].1);
                if f[idx(nx, ny)] != 0 {
                    found = Some((nx, ny));
                    break;
                }
            }
            let Some(p1) = found else {
                // A single pixel.
                f[idx(x, y)] = -nbd;
                out.push(vec![(x as i32 - 1, y as i32 - 1)]);
                continue;
            };
            let mut points: Vec<(i64, i64)> = Vec::new();
            let (mut p2, mut p3) = (p1, (x, y));
            loop {
                // Step 3.3: counter-clockwise around p3 from the one after p2.
                let d2 = dir_of(p2.0 - p3.0, p2.1 - p3.1);
                let mut p4 = p3;
                let mut east_zero = false;
                for k in 1..=8 {
                    let d = (d2 + k) % 8;
                    let (nx, ny) = (p3.0 + DIRS[d].0, p3.1 + DIRS[d].1);
                    if d == 0 && f[idx(nx, ny)] == 0 {
                        east_zero = true;
                    }
                    if f[idx(nx, ny)] != 0 {
                        p4 = (nx, ny);
                        break;
                    }
                }
                let i3 = idx(p3.0, p3.1);
                if east_zero {
                    f[i3] = -nbd;
                } else if f[i3] == 1 {
                    f[i3] = nbd;
                }
                points.push(p3);
                if p4 == (x, y) && p3 == p1 {
                    break;
                }
                p2 = p3;
                p3 = p4;
                if points.len() > (w * h) as usize * 4 {
                    break;
                }
            }
            out.push(simplify(&points));
        }
    }
    out
}

/// Keeps the points where the direction changes.
fn simplify(points: &[(i64, i64)]) -> Contour {
    let n = points.len();
    if n < 3 {
        return points.iter().map(|&(x, y)| (x as i32 - 1, y as i32 - 1)).collect();
    }
    let mut out = Vec::new();
    for i in 0..n {
        let prev = points[(i + n - 1) % n];
        let cur = points[i];
        let next = points[(i + 1) % n];
        let d1 = (cur.0 - prev.0, cur.1 - prev.1);
        let d2 = (next.0 - cur.0, next.1 - cur.1);
        if d1 != d2 {
            out.push((cur.0 as i32 - 1, cur.1 as i32 - 1));
        }
    }
    if out.is_empty() {
        out.push((points[0].0 as i32 - 1, points[0].1 as i32 - 1));
    }
    out
}

pub fn contour_area(c: &[(i32, i32)]) -> f64 {
    let n = c.len();
    if n < 3 {
        return 0.0;
    }
    let mut a = 0f64;
    for i in 0..n {
        let (x0, y0) = c[i];
        let (x1, y1) = c[(i + 1) % n];
        a += x0 as f64 * y1 as f64 - x1 as f64 * y0 as f64;
    }
    a / 2.0
}

pub fn arc_length(c: &[(i32, i32)]) -> f64 {
    let n = c.len();
    (0..n)
        .map(|i| {
            let (x0, y0) = c[i];
            let (x1, y1) = c[(i + 1) % n];
            (((x1 - x0) as f64).powi(2) + ((y1 - y0) as f64).powi(2)).sqrt()
        })
        .sum::<f64>()
        * if n > 1 { 1.0 } else { 0.0 }
}

/// Douglas-Peucker on a closed contour: split at the point furthest from the
/// first, then simplify both halves.
pub fn approx_poly(c: &[(i32, i32)], eps: f64) -> Contour {
    let n = c.len();
    if n < 3 {
        return c.to_vec();
    }
    let dist2 = |a: (i32, i32), b: (i32, i32)| ((a.0 - b.0) as f64).powi(2) + ((a.1 - b.1) as f64).powi(2);
    // The start: the point furthest from an arbitrary one, then the point
    // furthest from that, as OpenCV does for closed curves.
    let mut a = 0;
    for _ in 0..3 {
        let far = (0..n).max_by(|&i, &j| dist2(c[i], c[a]).total_cmp(&dist2(c[j], c[a]))).unwrap_or(0);
        a = far;
    }
    let b = (0..n).max_by(|&i, &j| dist2(c[i], c[a]).total_cmp(&dist2(c[j], c[a]))).unwrap_or(0);
    fn dp(c: &[(i32, i32)], idx: &[usize], eps: f64, keep: &mut Vec<usize>) {
        if idx.len() < 3 {
            return;
        }
        let (s, e) = (c[idx[0]], c[*idx.last().unwrap()]);
        let (dx, dy) = ((e.0 - s.0) as f64, (e.1 - s.1) as f64);
        let len = (dx * dx + dy * dy).sqrt();
        let mut best = (0usize, -1f64);
        for (k, &i) in idx.iter().enumerate().take(idx.len() - 1).skip(1) {
            let p = c[i];
            let d = if len > 0.0 {
                ((p.0 - s.0) as f64 * dy - (p.1 - s.1) as f64 * dx).abs() / len
            } else {
                (((p.0 - s.0) as f64).powi(2) + ((p.1 - s.1) as f64).powi(2)).sqrt()
            };
            if d > best.1 {
                best = (k, d);
            }
        }
        if best.1 > eps {
            dp(c, &idx[..=best.0], eps, keep);
            keep.push(idx[best.0]);
            dp(c, &idx[best.0..], eps, keep);
        }
    }
    let (lo, hi) = (a.min(b), a.max(b));
    let first: Vec<usize> = (lo..=hi).collect();
    let second: Vec<usize> = (hi..n).chain(0..=lo).collect();
    let mut keep = vec![lo];
    dp(c, &first, eps, &mut keep);
    keep.push(hi);
    dp(c, &second, eps, &mut keep);
    let mut out: Contour = Vec::new();
    for i in keep {
        if out.last() != Some(&c[i]) {
            out.push(c[i]);
        }
    }
    if out.len() > 1 && out.first() == out.last() {
        out.pop();
    }
    out
}

pub fn is_convex(c: &[(i32, i32)]) -> bool {
    let n = c.len();
    if n < 3 {
        return false;
    }
    let mut sign = 0i64;
    for i in 0..n {
        let a = c[i];
        let b = c[(i + 1) % n];
        let d = c[(i + 2) % n];
        let cross = (b.0 - a.0) as i64 * (d.1 - b.1) as i64 - (b.1 - a.1) as i64 * (d.0 - b.0) as i64;
        if cross != 0 {
            if sign == 0 {
                sign = cross.signum();
            } else if sign != cross.signum() {
                return false;
            }
        }
    }
    true
}

/// The sides of the smallest rectangle around a convex polygon, in any turn.
pub fn min_area_rect(c: &[(i32, i32)]) -> (f32, f32) {
    let n = c.len();
    let mut best = (f64::MAX, 0.0, 0.0);
    for i in 0..n {
        let a = c[i];
        let b = c[(i + 1) % n];
        let (ex, ey) = ((b.0 - a.0) as f64, (b.1 - a.1) as f64);
        let len = (ex * ex + ey * ey).sqrt();
        if len == 0.0 {
            continue;
        }
        let (ux, uy) = (ex / len, ey / len);
        let (mut min_u, mut max_u, mut min_v, mut max_v) = (f64::MAX, f64::MIN, f64::MAX, f64::MIN);
        for p in c {
            let u = p.0 as f64 * ux + p.1 as f64 * uy;
            let v = -(p.0 as f64) * uy + p.1 as f64 * ux;
            min_u = min_u.min(u);
            max_u = max_u.max(u);
            min_v = min_v.min(v);
            max_v = max_v.max(v);
        }
        let area = (max_u - min_u) * (max_v - min_v);
        if area < best.0 {
            best = (area, max_u - min_u, max_v - min_v);
        }
    }
    (best.1 as f32, best.2 as f32)
}

/// Whether `p` is inside the polygon or on its edge.
pub fn inside_polygon(c: &[(i32, i32)], p: Pt) -> bool {
    let n = c.len();
    let (px, py) = (p.x as f64, p.y as f64);
    let mut inside = false;
    for i in 0..n {
        let (x0, y0) = (c[i].0 as f64, c[i].1 as f64);
        let (x1, y1) = (c[(i + 1) % n].0 as f64, c[(i + 1) % n].1 as f64);
        // On the edge counts as inside.
        let cross = (x1 - x0) * (py - y0) - (y1 - y0) * (px - x0);
        if cross.abs() < 1e-9 && px >= x0.min(x1) - 1e-9 && px <= x0.max(x1) + 1e-9 && py >= y0.min(y1) - 1e-9 && py <= y0.max(y1) + 1e-9 {
            return true;
        }
        if (y0 > py) != (y1 > py) && px < (x1 - x0) * (py - y0) / (y1 - y0) + x0 {
            inside = !inside;
        }
    }
    inside
}

/// The homography that takes four points to four others, or None when they
/// do not span one.
pub fn perspective(src: &[Pt; 4], dst: &[Pt; 4]) -> Option<[[f64; 3]; 3]> {
    let mut a = [[0f64; 9]; 8];
    for i in 0..4 {
        let (x, y) = (src[i].x as f64, src[i].y as f64);
        let (u, v) = (dst[i].x as f64, dst[i].y as f64);
        a[i] = [x, y, 1.0, 0.0, 0.0, 0.0, -x * u, -y * u, u];
        a[i + 4] = [0.0, 0.0, 0.0, x, y, 1.0, -x * v, -y * v, v];
    }
    // Gaussian elimination with partial pivoting.
    for col in 0..8 {
        let piv = (col..8).max_by(|&i, &j| a[i][col].abs().total_cmp(&a[j][col].abs()))?;
        if a[piv][col].abs() < 1e-12 {
            return None;
        }
        a.swap(col, piv);
        for r in 0..8 {
            if r != col {
                let f = a[r][col] / a[col][col];
                for k in col..9 {
                    a[r][k] -= f * a[col][k];
                }
            }
        }
    }
    let s: Vec<f64> = (0..8).map(|i| a[i][8] / a[i][i]).collect();
    Some([[s[0], s[1], s[2]], [s[3], s[4], s[5]], [s[6], s[7], 1.0]])
}

pub fn apply_perspective(m: &[[f64; 3]; 3], p: Pt) -> Pt {
    let (x, y) = (p.x as f64, p.y as f64);
    let w = m[2][0] * x + m[2][1] * y + m[2][2];
    let w = if w.abs() > f64::EPSILON { 1.0 / w } else { 0.0 };
    Pt::new(((m[0][0] * x + m[0][1] * y + m[0][2]) * w) as f32, ((m[1][0] * x + m[1][1] * y + m[1][2]) * w) as f32)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn square(img: &mut Grey, x0: usize, y0: usize, x1: usize, y1: usize) {
        for y in y0..=y1 {
            for x in x0..=x1 {
                img.data[y * img.w + x] = 255;
            }
        }
    }

    #[test]
    fn a_filled_square_has_one_border_with_four_corners() {
        let mut img = Grey::new(20, 20);
        square(&mut img, 4, 5, 12, 14);
        let c = find_contours(&img);
        assert_eq!(c.len(), 1, "{c:?}");
        assert_eq!(c[0].len(), 4, "{c:?}");
        assert_eq!(contour_area(&c[0]).abs(), 8.0 * 9.0);
        assert!(is_convex(&c[0]));
    }

    #[test]
    fn a_ring_has_an_outer_and_an_inner_border() {
        let mut img = Grey::new(30, 30);
        square(&mut img, 3, 3, 25, 25);
        for y in 8..=20 {
            for x in 8..=20 {
                img.data[y * 30 + x] = 0;
            }
        }
        let c = find_contours(&img);
        assert_eq!(c.len(), 2, "{c:?}");
        let approx: Vec<usize> = c.iter().map(|c| approx_poly(c, 0.03 * arc_length(c)).len()).collect();
        assert_eq!(approx, vec![4, 4]);
    }

    #[test]
    fn homography_of_a_square_onto_itself_is_the_identity() {
        let s = [Pt::new(0., 0.), Pt::new(10., 0.), Pt::new(10., 10.), Pt::new(0., 10.)];
        let m = perspective(&s, &s).unwrap();
        let p = apply_perspective(&m, Pt::new(3., 7.));
        assert!((p.x - 3.).abs() < 1e-4 && (p.y - 7.).abs() < 1e-4);
        assert!(inside_polygon(&[(0, 0), (10, 0), (10, 10), (0, 10)], Pt::new(5., 5.)));
        assert!(inside_polygon(&[(0, 0), (10, 0), (10, 10), (0, 10)], Pt::new(10., 5.)));
        assert!(!inside_polygon(&[(0, 0), (10, 0), (10, 10), (0, 10)], Pt::new(11., 5.)));
    }

    #[test]
    fn area_resize_averages() {
        let img = Grey { w: 4, h: 2, data: vec![0, 100, 200, 200, 0, 100, 200, 200] };
        let s = img.resize_area(2, 1);
        assert_eq!(s.data, vec![50, 200]);
    }
}
