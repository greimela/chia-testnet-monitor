//! Low-level helpers for the canvas-drawn charts: SVG-style text, hover regions, the tooltip.

use gpui_kit::*;

use crate::theme::{REG, Theme};

#[derive(Clone, Copy, PartialEq)]
pub enum TextAnchor {
    Start,
    Middle,
    End,
}

/// Draw one line of text the way SVG `<text x y>` does: `y` is the baseline.
#[allow(clippy::too_many_arguments)]
pub fn text(
    win: &mut Window,
    cx: &mut App,
    s: &str,
    family: &'static str,
    size: f32,
    color: Hsla,
    x: f32,
    y: f32,
    anchor: TextAnchor,
) {
    if s.is_empty() {
        return;
    }
    let run = TextRun {
        len: s.len(),
        font: font(family),
        color,
        background_color: None,
        underline: None,
        strikethrough: None,
    };
    let line = win
        .text_system()
        .shape_line(s.to_string().into(), px(size), &[run], None);
    let w: f32 = line.width.into();
    let left = match anchor {
        TextAnchor::Start => x,
        TextAnchor::Middle => x - w / 2.0,
        TextAnchor::End => x - w,
    };
    let lh = size * 1.25;
    // Baseline sits about 0.8em below the top of a 1.25em line box.
    let top = y - size * 0.93;
    let _ = line.paint(
        point(px(left), px(top)),
        px(lh),
        TextAlign::Left,
        None,
        win,
        cx,
    );
}

pub fn text_width(win: &mut Window, s: &str, family: &'static str, size: f32) -> f32 {
    let run = TextRun {
        len: s.len(),
        font: font(family),
        color: black(),
        background_color: None,
        underline: None,
        strikethrough: None,
    };
    win.text_system()
        .shape_line(s.to_string().into(), px(size), &[run], None)
        .width
        .into()
}

pub fn rect(win: &mut Window, x: f32, y: f32, w: f32, h: f32, color: Hsla) {
    if w <= 0.0 || h <= 0.0 {
        return;
    }
    win.paint_quad(fill(
        Bounds::new(point(px(x), px(y)), size(px(w), px(h))),
        color,
    ));
}

pub fn rrect(win: &mut Window, x: f32, y: f32, w: f32, h: f32, r: f32, color: Hsla) {
    if w <= 0.0 || h <= 0.0 {
        return;
    }
    win.paint_quad(
        fill(Bounds::new(point(px(x), px(y)), size(px(w), px(h))), color).corner_radii(px(r)),
    );
}

pub fn line(win: &mut Window, x1: f32, y1: f32, x2: f32, y2: f32, width: f32, color: Hsla) {
    let mut b = PathBuilder::stroke(px(width));
    b.move_to(point(px(x1), px(y1)));
    b.line_to(point(px(x2), px(y2)));
    if let Ok(p) = b.build() {
        win.paint_path(p, color);
    }
}

pub fn diamond(win: &mut Window, cx_: f32, top: f32, color: Hsla, outline: Hsla) {
    let pts = [
        (cx_, top),
        (cx_ + 5.0, top + 5.0),
        (cx_, top + 10.0),
        (cx_ - 5.0, top + 5.0),
    ];
    let mut f = PathBuilder::fill();
    f.move_to(point(px(pts[0].0), px(pts[0].1)));
    for p in &pts[1..] {
        f.line_to(point(px(p.0), px(p.1)));
    }
    f.close();
    if let Ok(p) = f.build() {
        win.paint_path(p, color);
    }
    let mut s = PathBuilder::stroke(px(1.5));
    s.move_to(point(px(pts[0].0), px(pts[0].1)));
    for p in &pts[1..] {
        s.line_to(point(px(p.0), px(p.1)));
    }
    s.close();
    if let Ok(p) = s.build() {
        win.paint_path(p, outline);
    }
}

// ---------------- tooltip ----------------

pub struct Tip {
    pub theme: Theme,
    pub shown: Option<(SharedString, SharedString)>,
    pub pos: Point<Pixels>,
    /// Which hover region currently owns the tooltip, so only that region can hide it.
    pub owner: usize,
}

impl Tip {
    pub fn new(theme: Theme) -> Self {
        Self {
            theme,
            shown: None,
            pos: point(px(0.), px(0.)),
            owner: 0,
        }
    }
}

impl Render for Tip {
    fn render(&mut self, win: &mut Window, _cx: &mut Context<Self>) -> impl IntoElement {
        let Some((title, body)) = self.shown.clone() else {
            return div().into_any_element();
        };
        let vp = win.viewport_size();
        let (vw, vh): (f32, f32) = (vp.width.into(), vp.height.into());
        let (px_, py_): (f32, f32) = (self.pos.x.into(), self.pos.y.into());
        // Estimate the box so it can flip to the other side of the pointer near the window edge.
        let lines = 1 + body.lines().count();
        let est_w = 260.0f32.min(
            body.lines()
                .map(|l| l.len())
                .max()
                .unwrap_or(0)
                .max(title.len()) as f32
                * 6.6
                + 20.0,
        );
        let est_h = lines as f32 * 18.2 + 14.0;
        let mut x = px_ + 14.0;
        let mut y = py_ + 16.0;
        if x + est_w > vw - 8.0 {
            x = px_ - est_w - 14.0;
        }
        if y + est_h > vh - 8.0 {
            y = py_ - est_h - 16.0;
        }
        let t = self.theme;
        div()
            .absolute()
            .left(px(x.max(8.0)))
            .top(px(y.max(8.0)))
            .max_w(px(260.))
            .px(px(10.))
            .py(px(7.))
            .rounded(px(6.))
            .bg(t.ink)
            .text_color(t.paper)
            .text_size(px(13.))
            .line_height(px(18.2))
            .font_family(REG)
            .child(div().font_family(crate::theme::BOLD).child(title))
            .children(
                body.lines()
                    .map(|l| div().child(l.to_string()))
                    .collect::<Vec<_>>(),
            )
            .into_any_element()
    }
}

/// Make `bounds` a hover region: `hit` maps the pointer to tooltip text, or None for no tooltip.
pub fn hover_region(
    win: &mut Window,
    id: usize,
    bounds: Bounds<Pixels>,
    tip: Entity<Tip>,
    hit: impl Fn(Point<Pixels>) -> Option<(String, String)> + 'static,
) {
    win.on_mouse_event(move |e: &MouseMoveEvent, phase, _win, cx| {
        if phase != DispatchPhase::Bubble {
            return;
        }
        let inside = bounds.contains(&e.position);
        let content = if inside { hit(e.position) } else { None };
        tip.update(cx, |t, cx| match content {
            Some((a, b)) => {
                t.shown = Some((a.into(), b.into()));
                t.pos = e.position;
                t.owner = id;
                cx.notify();
            }
            None => {
                if t.owner == id && t.shown.is_some() {
                    t.shown = None;
                    cx.notify();
                }
            }
        });
    });
}
