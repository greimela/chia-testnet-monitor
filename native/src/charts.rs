//! Canvas-drawn charts. Each one paints quads and paths straight into the scene: no element
//! per bar, no layout, nothing allocated per frame beyond a few strings for labels.

use std::sync::Arc;

use gpui_kit::*;

use crate::fmtx::*;
use crate::model::Snapshot;
use crate::paint::*;
use crate::theme::*;

fn xy(b: Bounds<Pixels>) -> (f32, f32, f32, f32) {
    (
        b.origin.x.into(),
        b.origin.y.into(),
        b.size.width.into(),
        b.size.height.into(),
    )
}

pub fn logo(t: Theme) -> impl IntoElement {
    canvas(
        |_, _, _| (),
        move |b, _, win, _| {
            let (ox, oy, _, _) = xy(b);
            // 24 x 26 viewBox: five bars shifting from v1 to v2.
            let bar = |win: &mut Window, x: f32, y: f32, h: f32, c: Hsla| {
                rrect(win, ox + x, oy + y, 3.4, h, 1.0, c)
            };
            bar(win, 0.0, 3.0, 20.0, t.v1);
            bar(win, 5.0, 3.0, 15.0, t.v1);
            bar(win, 5.0, 18.8, 4.2, t.v2);
            bar(win, 10.0, 3.0, 10.0, t.v1);
            bar(win, 10.0, 13.8, 9.2, t.v2);
            bar(win, 15.0, 3.0, 5.0, t.v1);
            bar(win, 15.0, 8.8, 14.2, t.v2);
            bar(win, 20.0, 3.0, 20.0, t.v2);
        },
    )
    .w(px(24.))
    .h(px(26.))
}

/// The chain ribbon under the countdown.
pub fn ribbon(s: Arc<Snapshot>, t: Theme, tip: Entity<Tip>) -> impl IntoElement {
    canvas(
        |_, _, _| (),
        move |b, _, win, cx| {
            let (ox, oy, w, _) = xy(b);
            let (Some(f), Some(node)) = (s.fork, s.node.as_ref()) else {
                return;
            };
            let seq = s.chain.seq.as_bytes();
            let peak = node.height;
            let width = w.max(1.0);
            let step = if width < 600.0 { 3.0 } else { 4.0 };
            let bw = step - 1.0;
            let nb = (width / step).floor().max(1.0);
            let (top, bh) = (20.0f32, 64.0f32);
            let base = top + bh;
            let dmax = (f.v1_cutoff as f32 * 1.1).max((peak as f32 + 1.0) * 1.04);
            let bs = (dmax / nb).ceil().max(1.0) as u32;
            let x = |h: f32| h / bs as f32 * step;
            let end_x = nb * step;
            let last_bin = peak / bs;

            // Mark our wins per bin without a hash set: `ours` is sorted.
            let ours = &s.chain.ours;
            let mut oi = 0usize;

            // Regions behind the bars.
            let fx = x(f.height as f32);
            let cx_ = x(f.v1_cutoff as f32);
            let now_x = x(peak as f32 + 1.0);
            if now_x < end_x {
                line(
                    win,
                    ox + now_x + 2.0,
                    oy + base - 0.5,
                    ox + end_x,
                    oy + base - 0.5,
                    1.0,
                    t.rule,
                );
            }
            let phase_start = fx.max(now_x + 2.0);
            if cx_ > phase_start {
                hatch(win, ox + phase_start, oy + top, cx_ - phase_start, bh, t);
                if cx_ - phase_start > 130.0 {
                    region_label(
                        win,
                        cx,
                        ox + (phase_start + cx_) / 2.0,
                        oy + top + bh / 2.0,
                        "v1 phasing out",
                        t,
                    );
                }
            }
            let v2_start = cx_.max(now_x + 2.0);
            if end_x > v2_start {
                rect(
                    win,
                    ox + v2_start,
                    oy + top,
                    end_x - v2_start,
                    bh,
                    with_alpha(t.v2, 0.16),
                );
                if end_x - v2_start > 64.0 {
                    text(
                        win,
                        cx,
                        "v2 only",
                        REG,
                        12.5,
                        t.ink2,
                        ox + (v2_start + end_x) / 2.0,
                        oy + top + bh / 2.0 + 4.0,
                        TextAnchor::Middle,
                    );
                }
            }

            for i in 0..=last_bin {
                let s0 = i * bs;
                let e0 = (s0 + bs).min(peak + 1);
                let (mut v1, mut v2) = (0u32, 0u32);
                for k in s0..e0 {
                    match seq.get(k as usize) {
                        Some(b'1') => v1 += 1,
                        Some(b'2') => v2 += 1,
                        _ => {}
                    }
                }
                while oi < ours.len() && ours[oi] < s0 {
                    oi += 1;
                }
                let mut mine = 0;
                let mut oj = oi;
                while oj < ours.len() && ours[oj] < e0 {
                    mine += 1;
                    oj += 1;
                }
                let xx = ox + i as f32 * step;
                let hgt = if i == last_bin {
                    6.0f32.max(bh * (e0 - s0) as f32 / bs as f32)
                } else {
                    bh
                };
                let n = v1 + v2;
                if n > 0 {
                    let h2 = hgt * v2 as f32 / n as f32;
                    rect(win, xx, oy + base - hgt, bw, hgt - h2, t.v1);
                    rect(win, xx, oy + base - h2, bw, h2, t.v2);
                } else {
                    rect(win, xx, oy + base - hgt, bw, hgt, t.faint);
                }
                let _ = mine;
            }

            // Marker lines and their labels, below the ribbon or above it when they would collide.
            let ly = base + 26.0;
            let now_label_w = 86.0;
            let now_end = now_x > now_label_w;
            let (nl, nr) = if now_end {
                (now_x - now_label_w, now_x)
            } else {
                (now_x, now_x + now_label_w)
            };
            let mut taken: [Vec<(f32, f32)>; 2] = [vec![(nl, nr)], vec![]];
            let ys = [ly, top - 10.0];
            for (mx, label, lw) in [
                (fx, format!("Fork at {}", n(f.height)), 100.0f32),
                (cx_, format!("v1 cut-off at {}", n(f.v1_cutoff)), 136.0),
            ] {
                line(
                    win,
                    ox + mx,
                    oy + top - 6.0,
                    ox + mx,
                    oy + base + 8.0,
                    1.5,
                    t.ink,
                );
                let anchor_end = mx + lw > width;
                let lx = if anchor_end { mx - lw } else { mx };
                let pick = (0..2).find(|&k| {
                    taken[k]
                        .iter()
                        .all(|&(l, r)| lx > r + 12.0 || lx + lw < l - 12.0)
                });
                let Some(k) = pick else { continue };
                taken[k].push((lx, lx + lw));
                text(
                    win,
                    cx,
                    &label,
                    MED,
                    13.0,
                    t.ink2,
                    ox + if anchor_end { mx } else { mx + 6.0 },
                    oy + ys[k],
                    if anchor_end {
                        TextAnchor::End
                    } else {
                        TextAnchor::Start
                    },
                );
            }
            text(
                win,
                cx,
                &format!("Now {}", n(peak)),
                BOLD,
                13.0,
                t.ink,
                ox + if now_end { now_x - 4.0 } else { now_x },
                oy + ly,
                if now_end {
                    TextAnchor::End
                } else {
                    TextAnchor::Start
                },
            );

            // Hover: one tooltip per bin.
            let seq = s.clone();
            hover_region(
                win,
                1,
                Bounds::new(
                    point(px(ox), px(oy + top)),
                    size(px(end_x.min(width)), px(bh)),
                ),
                tip.clone(),
                move |p| {
                    let px_: f32 = p.x.into();
                    let i = ((px_ - ox) / step).floor();
                    if i < 0.0 || i as u32 > last_bin {
                        return None;
                    }
                    let i = i as u32;
                    let s0 = i * bs;
                    let e0 = (s0 + bs).min(peak + 1);
                    let sq = seq.chain.seq.as_bytes();
                    let (mut v1, mut v2) = (0, 0);
                    for k in s0..e0 {
                        match sq.get(k as usize) {
                            Some(b'1') => v1 += 1,
                            Some(b'2') => v2 += 1,
                            _ => {}
                        }
                    }
                    let mine = seq
                        .chain
                        .ours
                        .iter()
                        .filter(|&&h| h >= s0 && h < e0)
                        .count();
                    let title = if e0 - s0 == 1 {
                        format!("Block {}", n(s0))
                    } else {
                        format!("Blocks {}–{}", n(s0), n(e0 - 1))
                    };
                    let body = format!(
                        "v1 {v1}, v2 {v2}{}",
                        if mine > 0 {
                            format!("\nYou won {mine}")
                        } else {
                            String::new()
                        }
                    );
                    Some((title, body))
                },
            );
        },
    )
    .w_full()
    .h(px(120.))
}

fn region_label(win: &mut Window, cx: &mut App, x: f32, y: f32, label: &str, t: Theme) {
    let w = text_width(win, label, REG, 12.5) + 16.0;
    rrect(win, x - w / 2.0, y - 11.0, w, 22.0, 4.0, t.band);
    text(
        win,
        cx,
        label,
        REG,
        12.5,
        t.ink2,
        x,
        y + 4.0,
        TextAnchor::Middle,
    );
}

/// 45-degree hatch: faint v2 wash with rule-coloured stripes every 7px (perpendicular).
fn hatch(win: &mut Window, x: f32, y: f32, w: f32, h: f32, t: Theme) {
    rect(win, x, y, w, h, with_alpha(t.v2, 0.06));
    let gap = 7.0 * std::f32::consts::SQRT_2;
    let mut b = PathBuilder::stroke(px(2.5));
    let mut k = -h;
    // Lines "/" : from (x0, y+h) up-right to (x0+h, y).
    while k < w {
        let (x0, x1) = (k, k + h);
        let (cx0, cx1, cy0, cy1) = {
            // clip to [0, w]
            let (mut ax, mut ay, mut bx, mut by) = (x0, h, x1, 0.0f32);
            if ax < 0.0 {
                ay -= -ax;
                ax = 0.0;
            }
            if bx > w {
                by += bx - w;
                bx = w;
            }
            (ax, bx, ay, by)
        };
        if cx1 > cx0 {
            b.move_to(point(px(x + cx0), px(y + cy0)));
            b.line_to(point(px(x + cx1), px(y + cy1)));
        }
        k += gap;
    }
    if let Ok(p) = b.build() {
        win.paint_path(p, t.rule);
    }
}

/// Network-space sparkline.
pub fn sparkline(s: Arc<Snapshot>, t: Theme, tip: Entity<Tip>) -> impl IntoElement {
    canvas(
        |_, _, _| (),
        move |b, _, win, _| {
            let h = &s.history;
            if h.len() < 4 {
                return;
            }
            let (ox, oy, w, ht) = xy(b);
            let (mut lo, mut hi) = (f64::MAX, f64::MIN);
            for p in h {
                lo = lo.min(p.space);
                hi = hi.max(p.space);
            }
            let span = if hi - lo == 0.0 { 1.0 } else { hi - lo };
            let (t0, t1) = (h[0].t, h[h.len() - 1].t);
            let tspan = if t1 - t0 == 0.0 { 1.0 } else { t1 - t0 };
            let mut pb = PathBuilder::stroke(px(1.5));
            for (i, p) in h.iter().enumerate() {
                let x = ox + ((p.t - t0) / tspan) as f32 * w;
                let y = oy + (36.0 - ((p.space - lo) / span) as f32 * 32.0) / 40.0 * ht;
                if i == 0 {
                    pb.move_to(point(px(x), px(y)))
                } else {
                    pb.line_to(point(px(x), px(y)))
                }
            }
            if let Ok(p) = pb.build() {
                win.paint_path(p, t.ink2);
            }
            let body = format!("Low {}\nHigh {}", bytes_str(lo), bytes_str(hi));
            let title = format!("Since {}", clock(t0));
            hover_region(win, 2, b, tip.clone(), move |_| {
                Some((title.clone(), body.clone()))
            });
        },
    )
    .flex_1()
    .min_w(px(0.))
    .h(px(34.))
}

/// Plots passing the filter, one bar per signage point.
pub fn sp_chart(s: Arc<Snapshot>, t: Theme, tip: Entity<Tip>) -> impl IntoElement {
    canvas(
        |_, _, _| (),
        move |b, _, win, cx| {
            let sps = &s.sps;
            let (ox, oy, w, _) = xy(b);
            let h = 170.0f32;
            let (tp, bt) = (0.5f32, 22.0f32);
            let max_e = sps.iter().map(|p| p.eligible).max().unwrap_or(0).max(1);
            let step_y = if max_e <= 4 {
                1
            } else {
                (max_e as f32 / 4.0).ceil() as u32
            };
            let top_v = ((max_e as f32 / step_y as f32).ceil() as u32 * step_y) as f32;
            let y = |v: f32| tp + (h - tp - bt) * (1.0 - v / top_v);
            let count = sps.len().max(60);
            let slot = w / count as f32;
            let bw = (slot * 0.7).min(slot - 1.0).max(1.5);
            let x0 = (count - sps.len()) as f32 * slot;

            let mut v = 0;
            while v as f32 <= top_v {
                line(
                    win,
                    ox,
                    oy + y(v as f32),
                    ox + w,
                    oy + y(v as f32),
                    1.0,
                    if v > 0 { t.faint } else { t.rule },
                );
                v += step_y;
            }
            for (i, p) in sps.iter().enumerate() {
                let xx = ox + x0 + i as f32 * slot;
                if p.eligible > 0 {
                    let yt = y(p.eligible as f32);
                    rrect(
                        win,
                        xx,
                        oy + yt,
                        bw,
                        y(0.0) - yt,
                        2.0f32.min(bw / 2.0),
                        t.ink2,
                    );
                }
                let mut my = y(p.eligible as f32) - 9.0;
                for (n_, c) in [(p.v1_count(), t.v1), (p.v2, t.v2)] {
                    if n_ > 0 {
                        diamond(win, xx + bw / 2.0, oy + my - 5.0, c, t.paper);
                        my -= 12.0;
                    }
                }
            }
            // y-axis labels, over a paper-coloured plate so bars don't run through them.
            let mut v = step_y;
            while v as f32 <= top_v {
                let lbl = format!("{v} plot{}", if v == 1 { "" } else { "s" });
                rect(
                    win,
                    ox,
                    oy + y(v as f32) + 3.0,
                    if v == 1 { 40.0 } else { 48.0 },
                    17.0,
                    t.paper,
                );
                text(
                    win,
                    cx,
                    &lbl,
                    REG,
                    12.0,
                    t.ink3,
                    ox,
                    oy + y(v as f32) + 16.0,
                    TextAnchor::Start,
                );
                v += step_y;
            }
            if let (Some(a), Some(z)) = (sps.first(), sps.last()) {
                let mins = ((z.t - a.t) / 60.0).round() as i64;
                text(
                    win,
                    cx,
                    &format!("{mins} min ago"),
                    REG,
                    12.0,
                    t.ink3,
                    ox + x0,
                    oy + h - 4.0,
                    TextAnchor::Start,
                );
                text(
                    win,
                    cx,
                    "now",
                    REG,
                    12.0,
                    t.ink3,
                    ox + w,
                    oy + h - 4.0,
                    TextAnchor::End,
                );
            }
            let s2 = s.clone();
            let region = Bounds::new(
                point(px(ox), px(oy + tp - 12.0)),
                size(px(w), px(h - tp - bt + 12.0)),
            );
            hover_region(win, 3, region, tip.clone(), move |p| {
                let px_: f32 = p.x.into();
                let i = ((px_ - ox - x0 + (slot - bw) / 2.0) / slot).floor();
                if i < 0.0 {
                    return None;
                }
                let p = s2.sps.get(i as usize)?;
                let mut body = format!("{} of {} plots passed", p.eligible, p.plots);
                if p.v1_count() > 0 {
                    body += &format!("\n{} v1 proof", p.v1_count());
                }
                if p.v2 > 0 {
                    body += &format!("\n{} v2 quality", p.v2);
                }
                body += &format!("\nLookup {:.1} ms", p.s * 1000.0);
                Some((clock(p.t), body))
            });
        },
    )
    .w_full()
    .h(px(170.))
}

/// Actual pass rate on a track, with a tick where your plots should land on average.
pub fn pass_meter(
    pass: f64,
    expected: f64,
    count: u32,
    t: Theme,
    tip: Entity<Tip>,
) -> impl IntoElement {
    canvas(
        |_, _, _| (),
        move |b, _, win, _| {
            let (ox, oy, w, _) = xy(b);
            let max = (expected * 2.0).max(pass * 1.1);
            let x = |v: f64| (v / max).min(1.0) as f32 * w;
            rrect(win, ox, oy + 5.0, w, 8.0, 4.0, t.faint);
            rrect(
                win,
                ox,
                oy + 5.0,
                x(pass).max(if pass > 0.0 { 4.0 } else { 0.0 }),
                8.0,
                4.0,
                t.ink2,
            );
            let ex = ox + x(expected);
            // 1..17 of the 26px viewBox, stroke 2px
            rect(win, ex - 1.0, oy + 1.0, 2.0, 16.0, t.ink);
            let body = format!(
                "{:.1}% of {} signage points\nExpected {:.1}% for your plots at this height",
                pass * 100.0,
                n(count),
                expected * 100.0
            );
            hover_region(win, 4, b, tip.clone(), move |_| {
                Some(("Passed filter".into(), body.clone()))
            });
        },
    )
    .w_full()
    .h(px(26.))
    .mt(px(8.))
}

/// Every recent lookup as a faint tick on a log scale up to the harvester's 8 s warning; the average in ink.
pub fn lookup_strip(vals: Vec<f64>, avg: f64, t: Theme, tip: Entity<Tip>) -> impl IntoElement {
    canvas(
        |_, _, _| (),
        move |b, _, win, cx| {
            let (ox, oy, w, _) = xy(b);
            let (lo, hi, limit) = ((0.0001f64).log10(), (10.0f64).log10(), 8.0f64);
            let x = |sec: f64| ox + (((sec.max(0.0001)).log10() - lo) / (hi - lo)) as f32 * w;
            rect(win, ox, oy + 8.0, w, 2.0, t.faint);
            rect(
                win,
                x(limit),
                oy,
                ox + w - x(limit),
                18.0,
                with_alpha(t.warn, 0.14),
            );
            let tick = with_alpha(t.ink2, 0.3);
            for v in &vals {
                rect(win, x(*v) - 0.75, oy + 3.0, 1.5, 12.0, tick);
            }
            rect(win, x(avg) - 1.0, oy, 2.0, 18.0, t.ink);
            text(
                win,
                cx,
                "1 ms",
                REG,
                11.5,
                t.ink3,
                x(0.001),
                oy + 26.0,
                TextAnchor::Middle,
            );
            text(
                win,
                cx,
                "100 ms",
                REG,
                11.5,
                t.ink3,
                x(0.1),
                oy + 26.0,
                TextAnchor::Middle,
            );
            text(
                win,
                cx,
                "8 s",
                REG,
                11.5,
                t.ink3,
                ox + w,
                oy + 26.0,
                TextAnchor::End,
            );
            let max = vals.iter().cloned().fold(0.0, f64::max);
            let body = format!(
                "{} signage points\nAverage {}, slowest {}\nThe harvester warns above 8 s",
                vals.len(),
                ms(avg),
                ms(max)
            );
            hover_region(
                win,
                5,
                Bounds::new(point(px(ox), px(oy)), size(px(w), px(18.))),
                tip.clone(),
                move |_| Some(("Lookup time".into(), body.clone())),
            );
        },
    )
    .w_full()
    .h(px(30.))
    .mt(px(8.))
}
