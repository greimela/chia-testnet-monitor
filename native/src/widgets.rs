//! Small self-contained entities: the 1 Hz countdown and the confetti cannons. Each re-renders on its
//! own, so a tick never touches the rest of the page.

use std::time::{Duration, Instant};

use gpui_kit::*;

use crate::fmtx::now_secs;
use crate::theme::*;

pub struct Countdown {
    pub theme: Theme,
    pub target: Option<f64>,
    pub fallback: String,
    shown: i64,
}

impl Countdown {
    pub fn new(theme: Theme, cx: &mut Context<Self>) -> Self {
        // One timer, aligned to the second; it only wakes the view when the displayed value changes.
        cx.spawn(async move |this, cx| {
            loop {
                let frac = 1.0 - now_secs().fract();
                cx.background_executor()
                    .timer(Duration::from_secs_f64(frac + 0.005))
                    .await;
                if this.update(cx, |c, cx| c.tick(cx)).is_err() {
                    break;
                }
            }
        })
        .detach();
        Self {
            theme,
            target: None,
            fallback: "–".into(),
            shown: i64::MIN,
        }
    }

    fn remaining(&self) -> Option<i64> {
        self.target
            .map(|t| (t - now_secs()).max(0.0).round() as i64)
    }

    fn tick(&mut self, cx: &mut Context<Self>) {
        let r = self.remaining().unwrap_or(i64::MIN);
        if r != self.shown {
            self.shown = r;
            cx.notify();
        }
    }
}

impl Render for Countdown {
    fn render(&mut self, win: &mut Window, _cx: &mut Context<Self>) -> impl IntoElement {
        let t = self.theme;
        let vw: f32 = win.viewport_size().width.into();
        let fs = (vw * 0.15).clamp(68.0, 156.0);
        let base = div()
            .flex()
            .flex_row()
            .items_baseline()
            .h(px(fs * 0.9))
            .line_height(px(fs * 0.9))
            .text_size(px(fs))
            .mt(px(8.))
            .mb(px(11.))
            .ml(px(-4.));
        let Some(sec) = self.remaining() else {
            return base
                .font_family(CD)
                .text_color(t.ink)
                .child(self.fallback.clone())
                .into_any_element();
        };
        let (d, h, m, s) = (
            sec / 86400,
            (sec % 86400) / 3600,
            (sec % 3600) / 60,
            sec % 60,
        );
        let colon = || {
            div()
                .font_family(CD_LIGHT)
                .text_color(t.ink3)
                .px(px(fs * 0.02))
                .child(":")
        };
        let mut row = base.font_features(tnum()).font_family(CD).text_color(t.ink);
        if d > 0 {
            row = row.child(
                div()
                    .font_family(CD_MED)
                    .text_color(t.ink2)
                    .text_size(px(fs * 0.5))
                    .mr(px(fs * 0.07))
                    // baseline-aligned with the digits in the page; flex baselines don't apply here
                    .relative()
                    .top(px(fs * 0.167))
                    .child(format!("{d}d")),
            );
        }
        let ls = px(-fs * 0.01);
        let glyphs = |text: String| {
            text.chars()
                .map(move |ch| div().mr(ls).child(ch.to_string()))
                .collect::<Vec<_>>()
        };
        row.children(glyphs(if d > 0 {
            format!("{h:02}")
        } else {
            format!("{h}")
        }))
        .child(colon())
        .children(glyphs(format!("{m:02}")))
        .child(colon())
        .children(glyphs(format!("{s:02}")))
        .into_any_element()
    }
}

// ---------------- confetti ----------------

struct Bit {
    x: f32,
    y: f32,
    angle: f32,
    vel: f32,
    wobble: f32,
    wobble_speed: f32,
    tilt: f32,
    spin: f32,
    color: Hsla,
    size: f32,
    tick: f32,
}

pub struct Confetti {
    /// While set, both cannons keep firing until the instant passes (the full celebration lasts a second).
    stream: Option<(Instant, Theme)>,
    bits: Vec<Bit>,
    last: Option<Instant>,
    seed: u64,
}

impl Confetti {
    pub fn new() -> Self {
        Self {
            stream: None,
            bits: Vec::new(),
            last: None,
            seed: 0x9E3779B97F4A7C15 ^ (now_secs().to_bits()),
        }
    }

    fn rand(&mut self) -> f32 {
        // xorshift64*: plenty for particle jitter
        self.seed ^= self.seed >> 12;
        self.seed ^= self.seed << 25;
        self.seed ^= self.seed >> 27;
        ((self.seed.wrapping_mul(0x2545F4914F6CDD1D) >> 40) as f32) / (1u64 << 24) as f32
    }

    /// Two cannons in the bottom corners aimed inward, as canvas-confetti did on the web page.
    /// A win streams from both cannons for a second; the toggle's preview is a single small puff.
    pub fn burst(&mut self, theme: &Theme, vw: f32, vh: f32, full: bool) {
        if full {
            self.stream = Some((Instant::now() + std::time::Duration::from_secs(1), *theme));
        } else {
            self.spawn(theme, vw, vh, 45, false);
        }
        self.last = None;
    }

    fn spawn(&mut self, theme: &Theme, vw: f32, vh: f32, count: usize, full: bool) {
        let decay = 0.92f32;
        let reach = (vw * 0.75).hypot(vh) * if full { 1.0 } else { 0.5 };
        let velocity = (reach * (1.0 - decay)).min(150.0);
        let colors = [theme.v1, theme.v2, theme.yours, theme.ink];
        for side in 0..2 {
            for _ in 0..count {
                let base = if side == 0 { 58.0 } else { 112.0 };
                let angle = (base + self.rand() * 10.0 + (self.rand() - 0.5) * 50.0).to_radians();
                // canvas-confetti launches each piece at 0.5x to 1.5x of the cannon's start velocity.
                let start = velocity * (0.85 + self.rand() * 0.25);
                let vel = start * (0.5 + self.rand());
                let c = colors[(self.rand() * 4.0) as usize % 4];
                let ox = if side == 0 { 0.0 } else { vw };
                let (w1, w2, w3, w4) = (self.rand(), self.rand(), self.rand(), self.rand());
                self.bits.push(Bit {
                    x: ox,
                    y: vh,
                    angle,
                    vel,
                    wobble: w1 * 10.0,
                    wobble_speed: (0.1 + w2 * 0.1).min(0.11),
                    tilt: w3 * std::f32::consts::PI,
                    spin: 0.1 + w4 * 0.1,
                    color: c,
                    size: 8.8,
                    tick: 0.0,
                });
            }
        }
        self.last = None;
    }

    pub fn active(&self) -> bool {
        !self.bits.is_empty() || self.stream.is_some()
    }
}

impl Render for Confetti {
    fn render(&mut self, _win: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let this = cx.entity();
        canvas(
            |_, _, _| (),
            move |b, _, win, cx| {
                this.update(cx, |c, _| c.step_and_paint(b, win));
                if this.read(cx).active() {
                    win.request_animation_frame();
                }
            },
        )
        .absolute()
        .size_full()
    }
}

impl Confetti {
    fn step_and_paint(&mut self, b: Bounds<Pixels>, win: &mut Window) {
        let now = Instant::now();
        // Physics is written per 60 Hz frame; scale by elapsed time so 120 Hz screens match.
        let frames = self
            .last
            .map(|l| (now - l).as_secs_f32() * 60.0)
            .unwrap_or(1.0)
            .clamp(0.0, 4.0);
        self.last = Some(now);
        if let Some((until, theme)) = self.stream {
            if now >= until {
                self.stream = None;
            } else {
                let (w, h): (f32, f32) = (b.size.width.into(), b.size.height.into());
                self.spawn(&theme, w, h, (5.0 * frames).round().max(1.0) as usize, true);
            }
        }
        let (ox, oy): (f32, f32) = (b.origin.x.into(), b.origin.y.into());
        let decay = 0.92f32;
        for bit in &mut self.bits {
            bit.vel *= decay.powf(frames);
            bit.x += bit.angle.cos() * bit.vel * frames + 0.0;
            bit.y += (-bit.angle.sin()) * bit.vel * frames + 0.9 * 3.0 * frames;
            bit.wobble += bit.wobble_speed * frames;
            bit.tilt += bit.spin * frames;
            bit.tick += frames;
        }
        self.bits.retain(|b| b.tick < 340.0);
        for bit in &self.bits {
            let fade = 1.0 - bit.tick / 340.0;
            let mut col = bit.color;
            col.a *= fade;
            // A tumbling rectangle, approximated by squashing its height with the tilt.
            let w = bit.size * 1.1;
            let h = (bit.size * bit.tilt.sin().abs()).max(1.5);
            let x = ox + bit.x + bit.wobble.cos() * 4.0;
            let y = oy + bit.y;
            win.paint_quad(fill(
                Bounds::new(point(px(x - w / 2.0), px(y - h / 2.0)), size(px(w), px(h))),
                col,
            ));
        }
        if self.bits.is_empty() {
            self.last = None;
        }
    }
}
