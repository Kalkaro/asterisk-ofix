//! The animated window backdrop: stars drawn in ASCII on pure black, drifting slowly and
//! turning slightly, each at its own brightness with a gentle twinkle. A few big
//! five-pointed stars are filled in with characters; the small ones are sparkles like  -*-
//! or  \*/.
//!
//! Frames render on a thread at `FPS` into a `rune::Pixels`, which uploads them without
//! re-running the view. The image is laid out larger than the window, in `CHUNK` steps (see
//! `chunked`), so a live resize almost never changes its size: a resized image stretches the
//! old frame until the next one lands, and that aliased the ASCII.

use std::f64::consts::PI;
use std::sync::{Arc, Mutex, Weak};
use std::time::{Duration, Instant};

pub const FILL_RAMP: &str = " .,:-=+*#";
const CELL_W: i32 = 8;
const CELL_H: i32 = 14;
pub const FPS: u32 = 15;
/// The backdrop's size steps, in points.
pub const CHUNK: f32 = 256.0;
/// Opacity of the brightest star character; the backdrop stays well behind the UI.
pub const MAX_STAR_ALPHA: u32 = 105;
const BIG_STARS: usize = 4;
const SPARKLES: usize = 16;

/// The characters used, from X11's 7x13 "fixed" font (Go's basicfont.Face7x13), one byte per
/// row of an 8x14 cell, most significant bit leftmost.
const FONT: &[(char, [u8; 14])] = &[
    (' ', [0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00]),
    ('.', [0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x10, 0x38, 0x10, 0x00, 0x00]),
    (',', [0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x38, 0x30, 0x40, 0x00, 0x00]),
    (':', [0x00, 0x00, 0x00, 0x00, 0x10, 0x38, 0x10, 0x00, 0x00, 0x10, 0x38, 0x10, 0x00, 0x00]),
    ('-', [0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x7c, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00]),
    ('=', [0x00, 0x00, 0x00, 0x00, 0x00, 0xfc, 0x00, 0x00, 0xfc, 0x00, 0x00, 0x00, 0x00, 0x00]),
    ('+', [0x00, 0x00, 0x00, 0x00, 0x10, 0x10, 0x7c, 0x10, 0x10, 0x00, 0x00, 0x00, 0x00, 0x00]),
    ('*', [0x00, 0x00, 0x00, 0x00, 0x48, 0x30, 0xfc, 0x30, 0x48, 0x00, 0x00, 0x00, 0x00, 0x00]),
    ('#', [0x00, 0x00, 0x00, 0x28, 0x28, 0x7c, 0x28, 0x7c, 0x28, 0x28, 0x00, 0x00, 0x00, 0x00]),
    ('|', [0x00, 0x00, 0x10, 0x10, 0x10, 0x10, 0x10, 0x10, 0x10, 0x10, 0x10, 0x00, 0x00, 0x00]),
    ('/', [0x00, 0x00, 0x04, 0x04, 0x08, 0x08, 0x10, 0x20, 0x20, 0x40, 0x40, 0x00, 0x00, 0x00]),
    ('\\', [0x00, 0x00, 0x40, 0x40, 0x20, 0x20, 0x10, 0x08, 0x08, 0x04, 0x04, 0x00, 0x00, 0x00]),
];

/// One character as the pixels it lights.
type Glyph = Vec<(i32, i32)>;

fn glyph(c: char) -> Glyph {
    let rows = FONT.iter().find(|(f, _)| *f == c).map(|(_, rows)| rows).expect("character in FONT");
    (0..CELL_H)
        .flat_map(|y| (0..CELL_W).map(move |x| (x, y)))
        .filter(|&(x, y)| rows[y as usize] & (0x80 >> x) != 0)
        .collect()
}

/// One drifting star; its numbers are fixed at start.
struct Star {
    /// Position at t=0, 0..1 of the frame.
    x: f64,
    y: f64,
    /// Outer radius in pixels; 0 for a sparkle.
    r: f64,
    /// A sparkle with arms, rather than a lone character.
    arms: bool,
    /// Drift, pixels per second.
    vx: f64,
    vy: f64,
    angle: f64,
    /// Rotation, radians per second (small, either way).
    spin: f64,
    /// Phase of a slight back-and-forth turn.
    sway: f64,
    bright: f64,
    /// Phase of the brightness twinkle.
    twinkle: f64,
}

pub struct Stars {
    /// FILL_RAMP (indexed by how much of a cell a big star covers), then SPARKLE.
    glyphs: Vec<Glyph>,
    stars: Vec<Star>,
}

/// Sparkle characters, as indices into `Stars::glyphs` after the ramp.
const SPARKLE: [char; 6] = ['*', '+', '-', '|', '/', '\\'];
const RAMP_LEN: usize = FILL_RAMP.len();

impl Stars {
    pub fn new() -> Stars {
        let rnd = |i: usize, k: usize| (hash2(i as u32, k as u32) % 10000) as f64 / 10000.0;
        let stars = (0..BIG_STARS + SPARKLES)
            .map(|i| {
                let mut s = Star {
                    x: rnd(i, 1),
                    y: rnd(i, 2),
                    r: 0.0,
                    arms: false,
                    vx: 3.0 + 6.0 * rnd(i, 4),
                    vy: -2.0 + 4.0 * rnd(i, 5),
                    angle: 2.0 * PI * rnd(i, 6),
                    spin: (0.03 + 0.05 * rnd(i, 7)) * if i % 2 == 0 { 1.0 } else { -1.0 },
                    sway: 2.0 * PI * rnd(i, 8),
                    bright: 0.35 + 0.65 * rnd(i, 9),
                    twinkle: 2.0 * PI * rnd(i, 10),
                };
                if i < BIG_STARS {
                    // Below ~60 px a five-pointed star is too few cells tall to read as one.
                    s.r = 64.0 + 64.0 * rnd(i, 3);
                } else {
                    s.arms = rnd(i, 3) < 0.6;
                    s.spin *= 4.0; // sparkles turn visibly between + and x
                }
                s
            })
            .collect();
        Stars { glyphs: FILL_RAMP.chars().chain(SPARKLE).map(glyph).collect(), stars }
    }

    /// One frame at time `t`: `w * h` opaque, grey RGBA pixels.
    pub fn render(&self, w: u32, h: u32, t: f64) -> Vec<u8> {
        let (w, h) = (w as i32, h as i32);
        let mut dst = vec![0u8; (w * h * 4) as usize];
        for px in dst.as_chunks_mut::<4>().0 {
            px[3] = 0xff;
        }
        if w <= 0 || h <= 0 {
            return dst;
        }

        // One character per cell; the brightest star in a cell wins it.
        let (cols, rows) = ((w + CELL_W - 1) / CELL_W, (h + CELL_H - 1) / CELL_H);
        let mut cells: Vec<Option<(usize, f64)>> = vec![None; (cols * rows) as usize];
        let mut put = |col: i32, row: i32, glyph: usize, v: f64| {
            if col >= 0 && row >= 0 && col < cols && row < rows {
                let cell = &mut cells[(row * cols + col) as usize];
                if cell.is_none_or(|(_, cv)| v > cv) {
                    *cell = Some((glyph, v));
                }
            }
        };
        let (cw, ch) = (CELL_W as f64, CELL_H as f64);
        for s in &self.stars {
            // Drift and wrap around; turn slowly with a slight sway.
            let pad = s.r.max(ch) * 1.5;
            let (span_x, span_y) = (w as f64 + 2.0 * pad, h as f64 + 2.0 * pad);
            let cx = wrap(s.x * span_x + s.vx * t, span_x) - pad;
            let cy = wrap(s.y * span_y + s.vy * t, span_y) - pad;
            let rot = s.angle + s.spin * t + 0.12 * (t * 0.3 + s.sway).sin();
            let bright = s.bright * (0.8 + 0.2 * (t * 0.9 + s.twinkle).sin());

            if s.r == 0.0 {
                sparkle((cx / cw) as i32, (cy / ch) as i32, rot, s.arms, bright, &mut put);
                continue;
            }
            let (sin, cos) = (-rot).sin_cos();
            let (c0, c1) = (0.max(((cx - s.r) / cw) as i32), (cols - 1).min(((cx + s.r) / cw) as i32));
            let (r0, r1) = (0.max(((cy - s.r) / ch) as i32), (rows - 1).min(((cy + s.r) / ch) as i32));
            for row in r0..=r1 {
                for col in c0..=c1 {
                    // How much of the cell the star covers, from a 2x3 grid of samples in the
                    // star's own (unrotated, y-up) frame.
                    let mut covered = 0;
                    for sy in 0..3 {
                        for sx in 0..2 {
                            let dx = (col * CELL_W) as f64 + (sx as f64 + 0.5) * cw / 2.0 - cx;
                            let dy = (row * CELL_H) as f64 + (sy as f64 + 0.5) * ch / 3.0 - cy;
                            if sd_star5(dx * cos - dy * sin, -(dx * sin + dy * cos), s.r, 0.45) < 0.0 {
                                covered += 1;
                            }
                        }
                    }
                    if covered > 0 {
                        let i = (covered as f64 / 6.0 * (RAMP_LEN - 1) as f64).round() as usize;
                        put(col, row, i, bright);
                    }
                }
            }
        }
        for row in 0..rows {
            for col in 0..cols {
                if let Some((g, v)) = cells[(row * cols + col) as usize] {
                    let alpha = (MAX_STAR_ALPHA as f64 * (0.25 + 0.75 * v)) as u32;
                    blit(&mut dst, w, h, &self.glyphs[g], col * CELL_W, row * CELL_H, alpha);
                }
            }
        }
        dst
    }
}

/// A small star: a bright * with arms that are  - |  or  / \  depending on how far it has
/// turned, or just a lone * or +.
fn sparkle(col: i32, row: i32, rot: f64, arms: bool, v: f64, put: &mut impl FnMut(i32, i32, usize, f64)) {
    let [star, plus, dash, bar, slash, backslash] = std::array::from_fn(|i| RAMP_LEN + i);
    let quarter = PI / 2.0;
    let diagonal = ((rot % quarter) + quarter) % quarter > PI / 4.0;
    if !arms {
        put(col, row, if diagonal { star } else { plus }, v);
        return;
    }
    put(col, row, star, v);
    let arm = v * 0.55;
    if diagonal {
        put(col - 1, row - 1, backslash, arm);
        put(col + 1, row + 1, backslash, arm);
        put(col + 1, row - 1, slash, arm);
        put(col - 1, row + 1, slash, arm);
    } else {
        put(col - 1, row, dash, arm);
        put(col + 1, row, dash, arm);
        put(col, row - 1, bar, arm);
        put(col, row + 1, bar, arm);
    }
}

/// The signed distance from (x, y) to a five-pointed star with outer radius r and inner
/// radius r*rf, pointing up (y up): negative inside. After Inigo Quilez's sdStar5.
pub fn sd_star5(x: f64, y: f64, r: f64, rf: f64) -> f64 {
    const K1X: f64 = 0.809016994375;
    const K1Y: f64 = -0.587785252292;
    const K2X: f64 = -K1X;
    const K2Y: f64 = K1Y;
    let (mut x, mut y) = (x.abs(), y);
    let d = K1X * x + K1Y * y;
    if d > 0.0 {
        (x, y) = (x - 2.0 * d * K1X, y - 2.0 * d * K1Y);
    }
    let d = K2X * x + K2Y * y;
    if d > 0.0 {
        (x, y) = (x - 2.0 * d * K2X, y - 2.0 * d * K2Y);
    }
    x = x.abs();
    y -= r;
    let (bax, bay) = (rf * -K1Y, rf * K1X - 1.0);
    let h = ((x * bax + y * bay) / (bax * bax + bay * bay)).clamp(0.0, r);
    let d = (x - bax * h).hypot(y - bay * h);
    if y * bax - x * bay < 0.0 { -d } else { d }
}

/// Adds a white glyph at (x0, y0) with the given opacity (0..255).
fn blit(dst: &mut [u8], w: i32, h: i32, g: &Glyph, x0: i32, y0: i32, alpha: u32) {
    for &(gx, gy) in g {
        let (x, y) = (x0 + gx, y0 + gy);
        if x >= w || y >= h {
            continue;
        }
        let i = ((y * w + x) * 4) as usize;
        let v = dst[i] as u32;
        let v = (v + (0xff - v) * alpha / 0xff) as u8;
        dst[i..i + 3].fill(v);
    }
}

fn wrap(v: f64, span: f64) -> f64 {
    (v % span + span) % span
}

/// A cheap, stable integer hash.
fn hash2(x: u32, y: u32) -> u32 {
    let mut h = x.wrapping_mul(0x27d4eb2d) ^ y.wrapping_mul(0x165667b1);
    h ^= h >> 15;
    h = h.wrapping_mul(0x85ebca6b);
    h ^= h >> 13;
    h = h.wrapping_mul(0xc2b2ae35);
    h ^= h >> 16;
    h
}

/// A window size rounded up to whole `CHUNK`s.
pub fn chunked(size: [f32; 2]) -> [f32; 2] {
    size.map(|v| (v / CHUNK).ceil().max(1.0) * CHUNK)
}

/// The running backdrop: its image, and a thread drawing into it while the `Background`
/// lives.
#[derive(Clone)]
pub struct Background(Arc<Shared>);

struct Shared {
    pixels: rune::Pixels,
    stars: Stars,
    state: Mutex<Clock>,
}

#[derive(Default)]
pub struct Clock {
    /// Animation time in seconds; stops while paused.
    pub time: f64,
    last: Option<Instant>,
    paused: bool,
    /// Device pixels to draw at.
    size: (u32, u32),
}

impl Clock {
    /// Moves the clock to `now` unless paused, and says the time and whether it's running.
    /// Paused time doesn't count, so the animation picks up where it stopped instead of
    /// jumping ahead.
    pub fn advance(&mut self, now: Instant) -> (f64, bool) {
        if let (false, Some(last)) = (self.paused, self.last) {
            // A stalled tick doesn't jump.
            self.time += now.duration_since(last).as_secs_f64().min(0.25);
        }
        self.last = Some(now);
        (self.time, !self.paused)
    }

    pub fn set_paused(&mut self, paused: bool) {
        self.paused = paused;
    }
}

impl Background {
    pub fn new() -> Background {
        Background(Arc::new(Shared { pixels: rune::Pixels::new(), stars: Stars::new(), state: Mutex::default() }))
    }

    pub fn pixels(&self) -> &rune::Pixels {
        &self.0.pixels
    }

    /// The size to draw at, in device pixels. Draws a first frame right away when it changes.
    pub fn set_size(&self, w: u32, h: u32) {
        let mut st = self.state();
        if st.size != (w, h) {
            st.size = (w, h);
            let t = st.time;
            drop(st);
            self.0.pixels.set(w, h, self.0.stars.render(w, h, t));
        }
    }

    #[cfg(test)]
    pub fn is_paused(&self) -> bool {
        self.state().paused
    }

    /// Stops the clock, e.g. while the window isn't focused.
    pub fn set_paused(&self, paused: bool) {
        self.state().set_paused(paused);
    }

    /// Draws new frames at `FPS` on a thread until every handle to this background is gone.
    pub fn animate(&self) {
        let weak: Weak<Shared> = Arc::downgrade(&self.0);
        std::thread::spawn(move || {
            let tick = Duration::from_secs(1) / FPS;
            let mut next = Instant::now();
            loop {
                next += tick;
                std::thread::sleep(next.saturating_duration_since(Instant::now()));
                let Some(shared) = weak.upgrade() else { return };
                let (t, running, (w, h)) = {
                    let mut st = shared.state.lock().unwrap_or_else(|e| e.into_inner());
                    let (t, running) = st.advance(Instant::now());
                    (t, running, st.size)
                };
                if running && w > 0 && h > 0 {
                    shared.pixels.set(w, h, shared.stars.render(w, h, t));
                }
            }
        });
    }

    fn state(&self) -> std::sync::MutexGuard<'_, Clock> {
        self.0.state.lock().unwrap_or_else(|e| e.into_inner())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn black_with_dim_grey_stars() {
        let img = Stars::new().render(1024, 768, 0.0);
        let (mut black, mut lit, mut brightest) = (0, 0, 0u8);
        for px in img.as_chunks::<4>().0 {
            assert_eq!(px[3], 0xff, "the backdrop is opaque");
            assert!(px[0] == px[1] && px[1] == px[2], "stars are black and white, got {px:?}");
            brightest = brightest.max(px[0]);
            match px[0] {
                0 => black += 1,
                v if v > 0x18 => lit += 1,
                _ => {}
            }
        }
        let total = img.len() / 4;
        assert!(black >= total * 85 / 100, "only {}% is pure black; it should be mostly empty", black * 100 / total);
        assert!(lit > 0, "no stars drawn");
        assert!(brightest as u32 <= MAX_STAR_ALPHA, "brightest pixel {brightest}; stars must stay dim");
    }

    #[test]
    fn star_shape() {
        let r = 100.0;
        let pi = PI;
        for (name, x, y, inside) in [
            ("centre", 0.0, 0.0, true),
            ("just inside the top point", 0.0, r * 0.95, true),
            ("beyond the top point", 0.0, r * 1.05, false),
            ("notch between two points", r * 0.6 * (pi / 5.0).sin(), r * 0.6 * (pi / 5.0).cos(), false),
            ("inside the lower-left point", -r * 0.8 * (4.0 * pi / 5.0).sin(), r * 0.8 * (4.0 * pi / 5.0).cos(), true),
        ] {
            assert_eq!(sd_star5(x, y, r, 0.45) < 0.0, inside, "{name}");
        }
    }

    #[test]
    fn animates_deterministically() {
        let stars = Stars::new();
        let a = stars.render(512, 512, 0.0);
        assert_ne!(a, stars.render(512, 512, 4.0), "frames four seconds apart are identical");
        assert_eq!(a, stars.render(512, 512, 0.0), "rendering is deterministic for a given time");
    }

    /// While paused (window unfocused) the clock stands still, so the animation carries on
    /// from the same frame when the window comes back.
    #[test]
    fn paused_clock_does_not_jump() {
        let mut c = Clock::default();
        let t0 = Instant::now();
        c.advance(t0);
        c.advance(t0 + Duration::from_millis(50));
        c.set_paused(true);
        let (t, running) = c.advance(t0 + Duration::from_secs(10));
        assert!(!running && (t - 0.05).abs() < 1e-9, "paused: clock {t}");
        c.set_paused(false);
        let (t, running) = c.advance(t0 + Duration::from_millis(10_050));
        assert!(running && (t - 0.1).abs() < 1e-9, "resumed at {t}, want 0.1 (no jump for the paused time)");
        let (t, _) = c.advance(t0 + Duration::from_secs(20));
        assert!(t < 0.36, "a stalled tick jumped the clock to {t}");
    }

    /// The backdrop only changes size in CHUNK steps, so a live resize doesn't stretch a
    /// stale frame over the window.
    #[test]
    fn resizes_in_chunks() {
        let mut w = 700.0;
        while w <= 760.0 {
            assert_eq!(chunked([w, 600.0]), [768.0, 768.0], "window {w} wide");
            w += 3.0;
        }
        assert_eq!(chunked([1000.0, 600.0]), [1024.0, 768.0]);
    }
}
