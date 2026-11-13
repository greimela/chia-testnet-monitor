//! The dashboard view. Layout mirrors index.html section by section; all sizes are the page's CSS values.

use std::collections::HashMap;
use std::sync::Arc;
use std::time::Duration;

use gpui_kit::prelude::FluentBuilder as _;
use gpui_kit::*;

use crate::charts;
use crate::fmtx::*;
use crate::model::*;
use crate::paint::Tip;
use crate::theme::*;
use crate::widgets::{Confetti, Countdown};

/// (level: 1 warn / 2 crit, text, optional tooltip)
type HealthItem = (u8, String, Option<(String, String)>);

pub const POLL: Duration = Duration::from_secs(2);

pub struct Monitor {
    endpoint: String,
    snap: Arc<Snapshot>,
    last_ok: f64,
    dark: bool,
    tip: Entity<Tip>,
    countdown: Entity<Countdown>,
    confetti: Entity<Confetti>,
    confetti_on: bool,
    last_wins: Option<usize>,
    won_at: Option<(u32, f64)>,
    peer_samples: HashMap<String, Vec<(f64, u64, u64)>>,
    title: String,
}

fn fetch(url: &str) -> Option<Snapshot> {
    let resp = ureq::get(url)
        .set("Cache-Control", "no-store")
        .timeout(Duration::from_secs(5))
        .call()
        .ok()?;
    serde_json::from_reader(std::io::BufReader::new(resp.into_reader())).ok()
}

impl Monitor {
    pub fn new(url: String, win: &mut Window, cx: &mut Context<Self>) -> Self {
        let dark = system_dark(win);
        let theme = Theme::new(dark);
        let tip = cx.new(|_| Tip::new(theme));
        let countdown = cx.new(|cx| Countdown::new(theme, cx));
        let confetti = cx.new(|_| Confetti::new());
        cx.observe_window_appearance(win, |this, win, cx| {
            this.dark = system_dark(win);
            let th = Theme::new(this.dark);
            this.tip.update(cx, |t, _| t.theme = th);
            this.countdown.update(cx, |c, cx| {
                c.theme = th;
                cx.notify();
            });
            cx.notify();
        })
        .detach();

        let endpoint = format!("{}/api/state", url.trim_end_matches('/'));
        cx.spawn(async move |this, cx| {
            loop {
                let ep = endpoint.clone();
                let res = cx
                    .background_executor()
                    .spawn(async move { fetch(&ep) })
                    .await;
                if this.update(cx, |m, cx| m.apply(res, cx)).is_err() {
                    break;
                }
                cx.background_executor().timer(POLL).await;
            }
        })
        .detach();

        let confetti_on = std::fs::read_to_string(pref_path())
            .map(|s| s.trim() != "off")
            .unwrap_or(true);
        Self {
            endpoint: url,
            snap: Arc::new(Snapshot::default()),
            last_ok: 0.0,
            dark,
            tip,
            countdown,
            confetti,
            confetti_on,
            last_wins: None,
            won_at: None,

            peer_samples: HashMap::new(),
            title: String::new(),
        }
    }

    fn apply(&mut self, res: Option<Snapshot>, cx: &mut Context<Self>) {
        if let Some(s) = res {
            self.last_ok = now_secs();
            self.check_for_win(&s, cx);
            self.sample_peers(&s);
            let target = s.fork.zip(s.node.as_ref()).and_then(|(f, n)| {
                if n.height < f.height {
                    Some(f.eta)
                } else if n.height < f.v1_cutoff {
                    Some(f.cutoff_eta)
                } else {
                    None
                }
            });
            self.countdown.update(cx, |c, cx| {
                c.target = target;
                cx.notify();
            });
            self.snap = Arc::new(s);
        }
        cx.notify();
    }

    fn check_for_win(&mut self, s: &Snapshot, cx: &mut Context<Self>) {
        // Only compare once the server has loaded the whole chain; a backfill after a restart isn't a win.
        let complete = s.node.as_ref().is_some_and(|n| s.chain.count > n.height);
        if !complete {
            self.last_wins = None;
            return;
        }
        let wins = s.chain.ours.len();
        if self.last_wins.is_some_and(|l| wins > l) {
            self.won_at = Some((s.chain.ours[wins - 1], now_secs()));
            if self.confetti_on {
                self.celebrate(1.0, cx);
            }
        }
        self.last_wins = Some(wins);
    }

    fn celebrate(&mut self, scale: f32, cx: &mut Context<Self>) {
        let theme = Theme::new(self.dark);
        let confetti = self.confetti.clone();
        // Window size is only known with a window; the spawn below borrows one.
        cx.defer(move |cx| {
            let Some(win) = cx.active_window().or_else(|| cx.windows().first().copied()) else {
                return;
            };
            let _ = win.update(cx, |_, win, cx| {
                let vp = win.viewport_size();
                confetti.update(cx, |c, cx| {
                    c.burst(
                        &theme,
                        vp.width.into(),
                        vp.height.into(),
                        if scale >= 1.0 { 60 } else { 22 },
                        scale >= 1.0,
                    );
                    cx.notify();
                });
            });
        });
    }

    /// Recent byte counts per connection, for the "current traffic over the last ~10 s" figures.
    fn sample_peers(&mut self, s: &Snapshot) {
        let mut live: Vec<String> = Vec::with_capacity(s.peers.len());
        for p in &s.peers {
            let key = peer_key(p);
            let list = self.peer_samples.entry(key.clone()).or_default();
            if list.last().map(|l| l.0) != Some(s.now) {
                list.push((s.now, p.written, p.read));
            }
            while list.len() > 2 && s.now - list[1].0 >= 10.0 {
                list.remove(0);
            }
            live.push(key);
        }
        self.peer_samples.retain(|k, _| live.contains(k));
    }

    fn rates(&self, p: &Peer) -> Option<(f64, f64)> {
        let list = self.peer_samples.get(&peer_key(p))?;
        let (a, b) = (list.first()?, list.last()?);
        let dt = b.0 - a.0;
        (dt >= 3.0).then(|| ((b.1 - a.1) as f64 / dt, (b.2 - a.2) as f64 / dt))
    }
}

/// `--theme light|dark` pins the palette; otherwise it follows the system, like the page's prefers-color-scheme.
fn system_dark(win: &Window) -> bool {
    match std::env::args()
        .skip_while(|a| a != "--theme")
        .nth(1)
        .as_deref()
    {
        Some("dark") => true,
        Some("light") => false,
        _ => matches!(
            win.appearance(),
            WindowAppearance::Dark | WindowAppearance::VibrantDark
        ),
    }
}

fn peer_key(p: &Peer) -> String {
    format!("{}|{}|{}|{}", p.host, p.port, p.kind, p.since)
}

pub fn home_file(name: &str) -> Option<std::path::PathBuf> {
    let home = std::env::var_os("HOME").or_else(|| std::env::var_os("USERPROFILE"))?;
    Some(std::path::PathBuf::from(home).join(name))
}

fn pref_path() -> std::path::PathBuf {
    home_file(".chia-monitor-native-confetti").unwrap_or_default()
}

// ------------------------------------------------------------------------------------------------
// small building blocks

fn hover_tip(
    id: impl Into<ElementId>,
    owner: usize,
    tip: &Entity<Tip>,
    title: &str,
    body: &str,
) -> Stateful<Div> {
    let (t1, t2) = (tip.clone(), tip.clone());
    let (title, body) = (
        SharedString::from(title.to_string()),
        SharedString::from(body.to_string()),
    );
    div()
        .id(id)
        .on_mouse_move(move |e, _, cx| {
            t1.update(cx, |t, cx| {
                t.shown = Some((title.clone(), body.clone()));
                t.pos = e.position;
                t.owner = owner;
                cx.notify();
            });
        })
        .on_hover(move |over, _, cx| {
            if !*over {
                t2.update(cx, |t, cx| {
                    if t.owner == owner && t.shown.is_some() {
                        t.shown = None;
                        cx.notify();
                    }
                });
            }
        })
}

/// A fact value: number large, unit small after it.
fn num(t: &Theme, n: impl Into<SharedString>, unit: impl Into<SharedString>) -> Div {
    let unit: SharedString = unit.into();
    let mut d = div().flex().flex_row().items_baseline().child(
        div()
            .font_family(FACT)
            .text_size(px(18.))
            .line_height(px(25.2))
            .text_color(t.ink)
            .child(n.into()),
    );
    if !unit.is_empty() {
        d = d.child(div().ml(px(4.)).text_color(t.ink2).child(unit));
    }
    d
}

fn swatch(t: &Theme, v: &str) -> Div {
    div()
        .flex_none()
        .w(px(10.))
        .h(px(10.))
        .rounded(px(2.))
        .mr(px(8.))
        .bg(t.ver(v))
}

fn head(t: &Theme, title: impl Into<SharedString>, key: Option<Div>) -> Div {
    let mut d = div()
        .flex()
        .flex_row()
        .flex_wrap()
        .justify_between()
        .items_baseline()
        .min_h(px(24.))
        .mb(px(16.))
        .child(
            div()
                .font_family(BOLD)
                .text_size(px(15.))
                .line_height(px(24.))
                .text_color(t.ink)
                .child(title.into()),
        );
    if let Some(k) = key {
        d = d.child(
            k.flex()
                .flex_row()
                .gap(px(14.))
                .text_size(px(13.5))
                .line_height(px(24.))
                .text_color(t.ink2),
        );
    }
    d
}

fn fact_row(t: &Theme, dt: impl IntoElement, dd: impl IntoElement) -> Div {
    div()
        .flex()
        .flex_row()
        .justify_between()
        .min_h(px(42.))
        .py(px(8.))
        .border_t_1()
        .border_color(t.rule)
        .child(div().flex().items_center().text_color(t.ink3).child(dt))
        .child(
            div()
                .flex()
                .items_baseline()
                .justify_end()
                .whitespace_nowrap()
                .child(dd),
        )
}

fn ver_dt(t: &Theme, v: &str) -> Div {
    div()
        .flex()
        .flex_row()
        .items_center()
        .child(swatch(t, v))
        .child(v.to_string())
}

fn diamond_text(t: &Theme, which: &str) -> Div {
    div()
        .flex()
        .flex_row()
        .child(div().text_color(t.ver(which)).mr(px(5.)).child("◆"))
}

// ------------------------------------------------------------------------------------------------

impl Monitor {
    fn health(&self, t: &Theme) -> Div {
        let s = &*self.snap;
        let fresh = now_secs() - self.last_ok < 8.0;
        let mut items: Vec<HealthItem> = Vec::new();
        if !fresh {
            items.push((
                2,
                "Dashboard lost its connection to the server".into(),
                None,
            ));
        }
        let sv = &s.services;
        match (&s.node, sv.node) {
            (Some(n), true) if !n.synced => items.push((
                1,
                if n.sync_mode {
                    format!("Node syncing {} of {}", n_(n.height), n_(n.sync_tip))
                } else {
                    "Node not synced".into()
                },
                None,
            )),
            (Some(_), true) => {}
            _ => items.push((2, "Node offline".into(), None)),
        }
        let f = s.farm.as_ref();
        if !sv.farmer {
            items.push((2, "Farmer offline".into(), None));
        } else if f.is_none_or(|f| f.plots == 0) {
            items.push((1, "Farmer has no plots".into(), None));
        }
        if let Some(f) = f {
            if f.failed > 0 || f.no_key > 0 || f.duplicates > 0 {
                let mut parts = vec![];
                if f.failed > 0 {
                    parts.push(format!("{} plots failed to open", f.failed));
                }
                if f.no_key > 0 {
                    parts.push(format!("{} plots missing keys", f.no_key));
                }
                if f.duplicates > 0 {
                    parts.push(format!("{} duplicate plots", f.duplicates));
                }
                items.push((1, parts.join(", "), None));
            }
        }
        let v2plots = f.map_or(0, |f| f.by_version.v2.plots);
        if !sv.solver && v2plots > 0 {
            items.push((2, "Solver offline, so v2 plots can't win".into(), None));
        }
        let nodes: Vec<&Peer> = s.peers.iter().filter(|p| p.kind == "full node").collect();
        if sv.node && nodes.is_empty() {
            items.push((1, "No peers".into(), None));
        }
        if !sv.wallet || s.wallet.is_none() {
            items.push((1, "Wallet offline".into(), None));
        } else if !s.wallet.as_ref().is_some_and(|w| w.synced) {
            items.push((1, "Wallet syncing".into(), None));
        }
        div()
            .flex()
            .flex_row()
            .flex_wrap()
            .gap_x(px(20.))
            .gap_y(px(6.))
            .text_size(px(14.))
            .font_family(SEMI)
            .children(items.into_iter().map(|(lvl, text, _)| {
                let (c, mark) = if lvl == 2 {
                    (t.crit, "✕ ")
                } else {
                    (t.warn, "▲ ")
                };
                div().text_color(c).child(format!("{mark}{text}"))
            }))
    }

    fn hero(&self, t: &Theme) -> Div {
        let s = &*self.snap;
        let (label, caption): (&str, Div) = match (s.fork, s.node.as_ref()) {
            (Some(f), Some(n)) => {
                let cap = |count: u32, tail: String| {
                    div()
                        .flex()
                        .flex_row()
                        .items_baseline()
                        .child(div().font_family(SEMI).text_color(t.ink).child(n_(count)))
                        .child(format!(" {tail}"))
                };
                if n.height < f.height {
                    (
                        "Hard fork in",
                        cap(
                            f.height - n.height,
                            format!("blocks to go, {}", when_phrase(f.eta, s.now)),
                        ),
                    )
                } else if n.height < f.v1_cutoff {
                    (
                        "v1 plots stop winning in",
                        cap(
                            f.v1_cutoff - n.height,
                            format!(
                                "blocks to the v1 cut-off, {}",
                                when_phrase(f.cutoff_eta, s.now)
                            ),
                        ),
                    )
                } else {
                    (
                        "Hard fork complete",
                        div()
                            .flex()
                            .flex_row()
                            .child("v1 proofs stopped counting at block ")
                            .child(
                                div()
                                    .font_family(SEMI)
                                    .text_color(t.ink)
                                    .child(n_(f.v1_cutoff)),
                            ),
                    )
                }
            }
            _ => ("Waiting for the node", div()),
        };
        let mut band = div().w_full().bg(t.band).pt(px(34.)).pb(px(22.));
        let mut inner = wrap(self.pad())
            .child(
                div()
                    .font_family(MED)
                    .text_size(px(19.))
                    .line_height(px(28.5))
                    .text_color(t.ink2)
                    .child(label),
            )
            .child(self.countdown.clone())
            .child(
                caption
                    .text_size(px(17.))
                    .line_height(px(25.5))
                    .text_color(t.ink2),
            );
        if s.fork.is_some() && s.node.is_some() {
            inner = inner.child(div().mt(px(30.)).child(charts::ribbon(
                self.snap.clone(),
                *t,
                self.tip.clone(),
            )));
        }
        band = band.child(inner);
        band
    }

    fn pad(&self) -> f32 {
        32.0
    }

    fn figures(&self, t: &Theme, colw: f32, narrow: bool) -> Div {
        let s = &*self.snap;
        let col = |d: Div| {
            if narrow {
                d.w_full()
            } else {
                d.w(px(colw)).flex_none()
            }
        };

        // Your farm
        let bv = s.farm.as_ref().map(|f| f.by_version);
        let ver_row = |v: &str| {
            let (dd, _) = match bv {
                Some(b) => {
                    let st = if v == "v1" { b.v1 } else { b.v2 };
                    let (sz, u) = bytes(Some(st.size));
                    (
                        div()
                            .flex()
                            .flex_row()
                            .items_baseline()
                            .child(num(t, n(st.plots), plural(st.plots, "plot")))
                            .child(div().mx(px(10.)).text_color(t.ink3).child("·"))
                            .child(num(t, sz, u)),
                        (),
                    )
                }
                None => (div(), ()),
            };
            fact_row(t, ver_dt(t, v), dd)
        };
        let farm = s.farm.as_ref();
        let farm_col = col(div()
            .flex()
            .flex_col()
            .child(head(t, "Your farm", None))
            .child(self.bigrow(t, farm.map_or("–".into(), |f| n(f.plots)), "plots", None))
            .child(
                div()
                    .flex()
                    .flex_col()
                    .text_size(px(15.))
                    .child(ver_row("v1"))
                    .child(ver_row("v2"))
                    .child(fact_row(
                        t,
                        "Share",
                        num(
                            t,
                            farm.and_then(|f| f.share)
                                .map_or("–".into(), |x| format!("{:.2}", x * 100.0)),
                            "%",
                        ),
                    ))
                    .child(fact_row(t, "Time to win", {
                        let (a, u) = duration(farm.and_then(|f| f.etw));
                        num(t, a, u)
                    })),
            ));

        // Network
        let (sa, su) = bytes(s.node.as_ref().and_then(|n| n.space));
        let mix_row = |v: &str| {
            let c = if v == "v1" {
                s.chain.mix.v1
            } else {
                s.chain.mix.v2
            };
            fact_row(t, ver_dt(t, v), num(t, n(c), plural(c, "block")))
        };
        let spark = (s.history.len() >= 4)
            .then(|| charts::sparkline(self.snap.clone(), *t, self.tip.clone()));
        let net_col = col(div()
            .flex()
            .flex_col()
            .child(head(t, "Network", None))
            .child(self.bigrow(t, sa, su, spark.map(|c| c.into_any_element())))
            .child(
                div()
                    .flex()
                    .flex_col()
                    .text_size(px(15.))
                    .child(mix_row("v1"))
                    .child(mix_row("v2"))
                    .child(fact_row(
                        t,
                        "Height",
                        num(t, s.node.as_ref().map_or("–".into(), |x| n(x.height)), ""),
                    )),
            ));

        // Rewards
        let w = s.wallet.as_ref();
        let txch = w.map(|w| w.confirmed / 1e12);
        let big = txch.map_or("–".to_string(), |x| trim_float(x, 4));
        let mut rewards = div().flex().flex_col().text_size(px(15.));
        if let Some(w) = w {
            if w.unconfirmed != w.confirmed {
                rewards = rewards.child(fact_row(
                    t,
                    "Pending",
                    num(t, n(((w.unconfirmed - w.confirmed) / 1e12) as i64), "TXCH"),
                ));
            }
        }
        let won = |v: &str| {
            let c = if v == "v1" {
                s.chain.ours_by_kind.v1
            } else {
                s.chain.ours_by_kind.v2
            };
            fact_row(t, ver_dt(t, v), num(t, n(c), plural(c, "block")))
        };
        let last_win = if s.chain.ours.is_empty() {
            num(t, "–", "none yet")
        } else if let Some(lt) = s.chain.last_win_t {
            let (a, u) = duration(Some(s.now - lt));
            num(t, a, format!("{u} ago"))
        } else {
            num(t, n(*s.chain.ours.last().unwrap()), "block")
        };
        let rew_col = col(div()
            .flex()
            .flex_col()
            .child(head(t, "Rewards", None))
            .child(self.bigrow(t, big, "TXCH", None))
            .child(
                rewards
                    .child(won("v1"))
                    .child(won("v2"))
                    .child(fact_row(t, "Last win", last_win)),
            ));

        let mut row = div().flex().pt(px(40.)).pb(px(16.));
        row = if narrow {
            row.flex_col().gap(px(40.))
        } else {
            row.flex_row().gap(px(48.)).items_start()
        };
        row.child(farm_col).child(net_col).child(rew_col)
    }

    fn bigrow(
        &self,
        t: &Theme,
        big: impl Into<SharedString>,
        unit: impl Into<SharedString>,
        extra: Option<AnyElement>,
    ) -> Div {
        let mut r = div()
            .flex()
            .flex_row()
            .items_end()
            .gap(px(24.))
            .mb(px(20.))
            .min_w(px(0.))
            .child(
                div()
                    .flex()
                    .flex_row()
                    .items_baseline()
                    .flex_none()
                    .text_color(t.ink)
                    .child(
                        div()
                            .font_family(BIG)
                            .text_size(px(46.))
                            .line_height(px(46.))
                            .whitespace_nowrap()
                            .child(big.into()),
                    )
                    .child(
                        div()
                            .ml(px(6.))
                            .font_family(MED)
                            .text_size(px(20.))
                            .line_height(px(46.))
                            .text_color(t.ink2)
                            .child(unit.into()),
                    ),
            );
        if let Some(e) = extra {
            r = r.child(e);
        } else {
            r = r.child(div().flex_1());
        }
        r
    }

    fn plotting(&self, t: &Theme) -> Option<Div> {
        let p = self.snap.plotting.as_ref()?;
        let not_loaded = p.not_loaded.unwrap_or(0);
        if p.jobs.is_empty() && not_loaded == 0 {
            return None;
        }
        let now = self.snap.now;
        let mut jobs = div().flex().flex_col();
        for j in &p.jobs {
            let per = (j.done > 0).then(|| (now - j.started) / j.done as f64);
            let left = per.map_or("timing after the first plot".into(), |per| {
                format!(
                    "about {} left",
                    duration_str(Some(per * (j.total - j.done) as f64))
                )
            });
            let step = if j.done >= j.total {
                "finishing up".to_string()
            } else {
                format!(
                    "plot {}{}",
                    j.done + 1,
                    j.phase
                        .map_or(String::new(), |p| format!(", phase {p} of 4"))
                )
            };
            let c = t.ver(&j.kind);
            let segs = div()
                .flex()
                .flex_row()
                .gap(px(3.))
                .h(px(12.))
                .children((0..j.total).map(|i| {
                    let fill = if i < j.done {
                        c
                    } else if i == j.done {
                        with_alpha(c, 0.55)
                    } else {
                        t.faint
                    };
                    div().flex_1().h_full().rounded(px(2.)).bg(fill)
                }));
            jobs = jobs.child(
                div()
                    .flex()
                    .flex_row()
                    .items_center()
                    .py(px(6.))
                    .gap(px(24.))
                    .child(
                        div()
                            .w(px(124.))
                            .flex_none()
                            .flex()
                            .flex_row()
                            .items_center()
                            .text_color(t.ink3)
                            .child(swatch(t, &j.kind))
                            .child(format!(
                                "{}{}",
                                j.kind,
                                j.k.as_ref().map_or(String::new(), |k| format!(" k{k}"))
                            )),
                    )
                    .child(
                        div()
                            .flex_1()
                            .flex()
                            .flex_col()
                            .gap(px(6.))
                            .child(segs)
                            .child(div().text_size(px(14.)).text_color(t.ink2).child(format!(
                                "{} of {} done · {} · started {}{} · {}",
                                j.done,
                                j.total,
                                step,
                                clock(j.started),
                                per.map_or(String::new(), |p| format!(
                                    " · {} per plot",
                                    duration_str(Some(p))
                                )),
                                left
                            ))),
                    ),
            );
        }
        let mut d = div().pb(px(24.)).child(head(
            t,
            "Plotting",
            Some(div().child(format!(
                "{} v1 and {} v2 plots on disk",
                n(p.on_disk.v1),
                n(p.on_disk.v2)
            ))),
        ));
        d = d.child(if p.jobs.is_empty() {
            div()
                .text_color(t.ink3)
                .py(px(7.))
                .child("No plots being created right now.")
        } else {
            jobs
        });
        if not_loaded > 0 {
            d = d.child(div().font_family(SEMI).text_color(t.warn).mt(px(8.)).child(format!(
                "▲ {} finished plot{} waiting for the harvester to load {}. It rescans every couple of minutes.",
                not_loaded, if not_loaded == 1 { " is" } else { "s are" }, if not_loaded == 1 { "it" } else { "them" }
            )));
        }
        Some(d)
    }

    fn lower(&self, t: &Theme, colw: f32, narrow: bool) -> Div {
        let s = &*self.snap;
        let span2 = colw * 2.0 + 48.0;
        let key = div()
            .flex()
            .flex_row()
            .gap(px(14.))
            .child(
                div()
                    .flex()
                    .flex_row()
                    .child(div().text_color(t.v1).mr(px(5.)).child("◆"))
                    .child("v1 proof"),
            )
            .child(
                div()
                    .flex()
                    .flex_row()
                    .child(div().text_color(t.v2).mr(px(5.)).child("◆"))
                    .child("v2 quality"),
            );
        let mut left = div()
            .flex()
            .flex_col()
            .when(narrow, |d| d.w_full())
            .when(!narrow, |d| d.w(px(span2)).flex_none())
            .child(head(t, "Plots passing the filter", Some(key)));
        if s.sps.is_empty() {
            left = left.child(div().text_color(t.ink3).py(px(7.)).child("No signage points yet. They arrive about every 9 seconds once the farmer is connected."));
        } else {
            left = left.child(charts::sp_chart(self.snap.clone(), *t, self.tip.clone()));
            let tt = &s.sp_totals;
            let pass = if tt.count > 0 {
                tt.passed as f64 / tt.count as f64
            } else {
                0.0
            };
            let lookups: Vec<f64> = s.sps.iter().map(|p| p.s).collect();
            let cw = |frac: f32| {
                if narrow {
                    None
                } else {
                    Some(((span2 - 96.0) * frac / 5.3).max(0.0))
                }
            };
            let stat = |dt: Div, dd: Div, extra: Option<AnyElement>, w: Option<f32>| {
                let mut c = div().flex().flex_col().min_w(px(0.));
                c = match w {
                    Some(w) => c.w(px(w)).flex_none(),
                    None => c.flex_1(),
                };
                c = c.child(dt.text_size(px(13.5)).text_color(t.ink3)).child(dd);
                if let Some(e) = extra {
                    c = c.child(e);
                }
                c
            };
            let dd = |x: String| {
                div()
                    .font_family(STAT)
                    .text_size(px(20.))
                    .line_height(px(30.))
                    .text_color(t.ink)
                    .child(x)
            };
            let small = |x: &str| {
                div()
                    .font_family(MED)
                    .text_size(px(14.))
                    .text_color(t.ink2)
                    .ml(px(4.))
                    .child(x.to_string())
            };
            let pass_meter = s.farm.as_ref().and_then(|f| f.expected_pass).map(|e| {
                charts::pass_meter(pass, e, tt.count, *t, self.tip.clone()).into_any_element()
            });
            let mut stats = div()
                .flex()
                .gap(px(32.))
                .mt(px(16.))
                .pt(px(12.))
                .border_t_1()
                .border_color(t.rule);
            stats = if narrow {
                stats.flex_wrap()
            } else {
                stats.flex_row().items_start()
            };
            stats = stats
                .child(stat(
                    div().child("Passed filter"),
                    dd(format!("{:.1}%", pass * 100.0)),
                    pass_meter,
                    cw(1.3),
                ))
                .child(stat(
                    diamond_text(t, "v1").child("v1 proofs"),
                    dd(n(tt.v1.unwrap_or(tt.proofs))),
                    None,
                    cw(1.0),
                ))
                .child(stat(
                    diamond_text(t, "v2").child("v2 qualities"),
                    dd(n(tt.v2.unwrap_or(0))),
                    None,
                    cw(1.0),
                ))
                .child(stat(
                    div().child("Lookup time"),
                    div()
                        .flex()
                        .flex_row()
                        .items_baseline()
                        .child(dd(ms(tt.avg_s)))
                        .child(small("avg"))
                        .child(div().w(px(6.)))
                        .child(dd(ms(tt.max_s)))
                        .child(small("max")),
                    Some(
                        charts::lookup_strip(lookups, tt.avg_s, *t, self.tip.clone())
                            .into_any_element(),
                    ),
                    cw(2.0),
                ));
            left = left.child(stats);
        }

        let feed_rows: Vec<Div> = if s.events.is_empty() {
            vec![div().text_color(t.ink2).child("No proofs or warnings yet.")]
        } else {
            s.events
                .iter()
                .map(|e| {
                    let what = match e.kind.as_str() {
                        "win" => div()
                            .font_family(BOLD)
                            .text_color(t.yours)
                            .child(e.text.clone()),
                        "proof" => div().text_color(t.ink).child(format!("Found {}", e.text)),
                        "warning" => div()
                            .flex()
                            .flex_row()
                            .gap(px(4.))
                            .child(
                                div()
                                    .font_family(SEMI)
                                    .text_color(t.warn)
                                    .flex_none()
                                    .child("▲ Warning"),
                            )
                            .child(div().text_color(t.ink2).child(e.text.clone())),
                        _ => div()
                            .flex()
                            .flex_row()
                            .gap(px(4.))
                            .child(
                                div()
                                    .font_family(SEMI)
                                    .text_color(t.crit)
                                    .flex_none()
                                    .child("✕ Error"),
                            )
                            .child(div().text_color(t.ink2).child(e.text.clone())),
                    };
                    div()
                        .flex()
                        .flex_row()
                        .gap(px(12.))
                        .py(px(7.))
                        .border_t_1()
                        .border_color(t.rule)
                        .text_size(px(14.))
                        .child(
                            div()
                                .w(px(62.))
                                .flex_none()
                                .text_color(t.ink3)
                                .child(if e.t > 0.0 { clock(e.t) } else { String::new() }),
                        )
                        .child(div().flex_1().min_w(px(0.)).child(what))
                })
                .collect()
        };
        let feed = div()
            .id("feed")
            .size_full()
            .overflow_y_scroll()
            .children(feed_rows)
            .child(div().border_t_1().border_color(t.rule));
        let feed_box = if narrow {
            div().max_h(px(360.)).relative().child(
                div()
                    .id("feed-n")
                    .max_h(px(360.))
                    .overflow_y_scroll()
                    .children(Vec::<Div>::new())
                    .child(feed),
            )
        } else {
            div().flex_1().min_h(px(0.)).relative().child(
                div()
                    .absolute()
                    .top_0()
                    .left_0()
                    .right_0()
                    .bottom_0()
                    .child(feed),
            )
        };
        let side = div()
            .flex()
            .flex_col()
            .when(narrow, |d| d.w_full())
            .when(!narrow, |d| d.w(px(colw)).flex_none())
            .child(head(t, "Events", None))
            .child(feed_box);

        let mut row = div().flex().py(px(40.));
        row = if narrow {
            row.flex_col().gap(px(40.))
        } else {
            row.flex_row().gap(px(48.)).items_stretch()
        };
        row.child(left).child(side)
    }

    fn table(&self, t: &Theme, heads: &[&str], widths: &[f32], rows: Vec<Vec<Div>>) -> Div {
        let cell = |i: usize, c: Div, th: bool| {
            let last = i == widths.len() - 1;
            let mut d = div()
                .flex_basis(relative(0.))
                .flex_grow(widths[i])
                .min_w(px(0.))
                .py(px(7.))
                .pr(if last { px(0.) } else { px(12.) });
            if last {
                d = d.flex().justify_end();
            }
            if th {
                d.font_family(MED).text_color(t.ink3).child(c)
            } else {
                d.child(c)
            }
        };
        let mut tbl = div().flex().flex_col().text_size(px(14.)).w_full().child(
            div()
                .flex()
                .flex_row()
                .border_t_1()
                .border_color(t.rule)
                .children(
                    heads
                        .iter()
                        .enumerate()
                        .map(|(i, h)| cell(i, div().child(h.to_string()), true)),
                ),
        );
        for r in rows {
            tbl = tbl.child(
                div()
                    .flex()
                    .flex_row()
                    .border_t_1()
                    .border_color(t.rule)
                    .children(r.into_iter().enumerate().map(|(i, c)| cell(i, c, false))),
            );
        }
        tbl
    }

    fn strength(&self, t: &Theme) -> Option<Div> {
        let st = self.snap.strength.as_ref()?;
        let key = div().child(format!(
            "Counting since {} · max strength now {}",
            month_day_time(st.since),
            st.max_strength
        ));
        let sub = |x: String| div().text_color(t.ink3).child(x);
        let rows: Vec<Vec<Div>> =
            st.rows
                .iter()
                .map(|r| {
                    let label = if r.cls == "v1" {
                        div()
                            .flex()
                            .items_center()
                            .child(swatch(t, "v1"))
                            .child("v1")
                    } else {
                        div()
                            .flex()
                            .items_center()
                            .child(swatch(t, "v2"))
                            .child(format!("Strength {}", &r.cls[1..]))
                    };
                    let status = if r.cls == "v1" {
                        if r.state == "retired" {
                            sub(format!("Retired at {}", n(r.until)))
                        } else {
                            div().child(format!("Farming until {}", n(r.until)))
                        }
                    } else if r.state == "farming" {
                        div().child("Farming")
                    } else {
                        sub(format!(
                            "From block {}{}",
                            n(r.unlock),
                            if r.unlock == st.fork { " (fork)" } else { "" }
                        ))
                    };
                    let filter = r
                        .bits
                        .map_or("–".to_string(), |b| format!("1 in {}", n(1u64 << b)));
                    let ratio = (r.expected >= 1.0).then(|| r.passes as f64 / r.expected);
                    let off = ratio.is_some_and(|ra| {
                        r.expected >= 20.0 && (ra - 1.0).abs() > 3.0 / r.expected.sqrt()
                    });
                    let passes = if r.checks > 0 {
                        div()
                            .flex()
                            .flex_row()
                            .gap(px(4.))
                            .text_color(if off { t.warn } else { t.ink })
                            .child(n(r.passes))
                            .child(div().text_color(if off { t.warn } else { t.ink3 }).child(
                                format!(
                                    "of {:.1} expected{}",
                                    r.expected,
                                    ratio.map_or(String::new(), |ra| format!(
                                        " ({}%)",
                                        (ra * 100.0).round() as i64
                                    ))
                                ),
                            ))
                    } else {
                        sub("–".into())
                    };
                    let checks = div()
                        .flex()
                        .flex_row()
                        .gap(px(4.))
                        .child(n(r.checks))
                        .children(
                            (r.locked > 0)
                                .then(|| sub(format!("({} skipped while locked)", n(r.locked)))),
                        );
                    let per = if r.gb_sps > 0.0 {
                        format!("{:.2}", r.proofs as f64 / r.gb_sps * st.sps_per_day)
                    } else {
                        "–".into()
                    };
                    vec![
                        label,
                        div()
                            .flex()
                            .flex_row()
                            .gap(px(4.))
                            .child(n(r.plots))
                            .child(sub(bytes_str(r.gb * 1e9))),
                        status,
                        div().child(filter),
                        checks,
                        passes,
                        div().child(n(r.proofs)),
                        div().child(per),
                        div().child(n(r.wins)),
                    ]
                })
                .collect();
        let tbl = self.table(
            t,
            &[
                "Plots",
                "Count",
                "Status",
                "Filter now",
                "Plot checks",
                "Passed filter",
                "Proofs",
                "Proofs per GB-day",
                "Wins",
            ],
            &[118.0, 117.0, 195.0, 89.0, 109.0, 239.0, 71.0, 160.0, 98.0],
            rows,
        );
        let c = &st.check;
        let note = if c.mismatch_count > 0 {
            let m = c.mismatches.last();
            div().font_family(SEMI).text_color(t.warn).child(format!(
                "▲ Filter counts differ from the harvester's in {} of {} sub-slots{}. The pass counts above may be off.",
                n(c.mismatch_count), n(c.compared),
                m.map_or(String::new(), |m| format!(" (latest {}: {} here, {} from the harvester)", m.challenge, m.ours, m.harvester))
            ))
        } else if c.compared > 0 {
            div().flex().flex_row().text_color(t.ink2)
                .child(div().font_family(BOLD).text_color(t.v2).mr(px(6.)).child("✓"))
                .child(format!("Filter passes match the harvester's own count in all {} finished sub-slot{} checked.", n(c.compared), if c.compared == 1 { "" } else { "s" }))
        } else {
            div().text_color(t.ink2).child("Filter passes are checked against the harvester's count once a full sub-slot (about 10 minutes) has been seen.")
        };
        let title = hover_tip("strength-h", 100, &self.tip, "Plot strength",
            "Each step up in strength should halve how often a plot passes the filter and make each pass twice as likely to find a proof, so proofs and wins per GB-day should stay level across strengths. Plots above the maximum strength are skipped until their block.")
            .font_family(BOLD).text_size(px(15.)).line_height(px(24.)).text_color(t.ink).child("Plot strength");
        let header = div()
            .flex()
            .flex_row()
            .flex_wrap()
            .justify_between()
            .items_baseline()
            .min_h(px(24.))
            .mb(px(16.))
            .child(title)
            .child(
                div()
                    .text_size(px(13.5))
                    .line_height(px(24.))
                    .text_color(t.ink2)
                    .child(key),
            );
        Some(
            div()
                .pt(px(16.))
                .pb(px(40.))
                .child(header)
                .child(tbl)
                .child(
                    div()
                        .text_size(px(14.))
                        .mt(px(12.))
                        .max_w(px(760.))
                        .child(note),
                ),
        )
    }

    fn peers(&self, t: &Theme) -> Div {
        let s = &*self.snap;
        const ORDER: [&str; 6] = [
            "full node",
            "farmer",
            "wallet",
            "harvester",
            "timelord",
            "introducer",
        ];
        let rank = |k: &str| ORDER.iter().position(|o| *o == k).map_or(99, |p| p + 1);
        let mut peers: Vec<&Peer> = s.peers.iter().collect();
        peers.sort_by(|a, b| {
            rank(&a.kind)
                .cmp(&rank(&b.kind))
                .then(
                    b.height
                        .map_or(-1i64, |h| h as i64)
                        .cmp(&a.height.map_or(-1i64, |h| h as i64)),
                )
                .then(
                    a.since
                        .partial_cmp(&b.since)
                        .unwrap_or(std::cmp::Ordering::Equal),
                )
        });
        let mut counts: Vec<(String, u32)> = vec![];
        for p in &peers {
            match counts.iter_mut().find(|(k, _)| *k == p.kind) {
                Some(c) => c.1 += 1,
                None => counts.push((p.kind.clone(), 1)),
            }
        }
        let key = if peers.is_empty() {
            String::new()
        } else {
            format!(
                "{} connection{}: {}",
                n(peers.len() as u32),
                if peers.len() == 1 { "" } else { "s" },
                counts
                    .iter()
                    .map(|(k, c)| format!("{c} {k}{}", if *c == 1 { "" } else { "s" }))
                    .collect::<Vec<_>>()
                    .join(", ")
            )
        };
        let header = head(t, "Peers", Some(div().child(key)));
        if peers.is_empty() {
            return div().pt(px(16.)).pb(px(64.)).child(header).child(
                div()
                    .text_color(t.ink3)
                    .py(px(7.))
                    .child(if s.services.node {
                        "No connections yet."
                    } else {
                        "The full node isn't running."
                    }),
            );
        }
        let ours = s.node.as_ref().map(|n| n.height);
        let sub = |x: String| div().text_color(t.ink3).child(x);
        let rate = |r: Option<f64>| {
            r.map(|r| {
                sub(format!(
                    "· {}",
                    if r < 1.0 {
                        "idle".to_string()
                    } else {
                        format!("{}/s", bytes_str(r))
                    }
                ))
            })
        };
        let rows: Vec<Vec<Div>> = peers
            .iter()
            .map(|p| {
                let rates = self.rates(p);
                let lag = match (p.height, ours) {
                    (Some(h), Some(o)) => o as i64 - h as i64,
                    _ => 0,
                };
                let host = if p.host.contains(':') {
                    format!("[{}]", p.host)
                } else {
                    p.host.clone()
                };
                let height = match p.height {
                    None => sub("–".into()),
                    Some(h) => div()
                        .flex()
                        .flex_row()
                        .gap(px(4.))
                        .text_color(if lag > 3 { t.warn } else { t.ink })
                        .child(n(h))
                        .children((lag > 3).then(|| {
                            div()
                                .text_color(t.ink3)
                                .child(format!("({} behind)", n(lag)))
                        })),
                };
                vec![
                    div()
                        .flex()
                        .flex_row()
                        .child(host)
                        .children((p.port > 0).then(|| sub(format!(":{}", p.port)))),
                    div().child(p.kind.clone()),
                    height,
                    div()
                        .flex()
                        .flex_row()
                        .gap(px(4.))
                        .child(bytes_str(p.written as f64))
                        .children(rate(rates.map(|r| r.0))),
                    div()
                        .flex()
                        .flex_row()
                        .gap(px(4.))
                        .child(bytes_str(p.read as f64))
                        .children(rate(rates.map(|r| r.1))),
                    div().child(if s.now - p.since < 60.0 {
                        "just now".to_string()
                    } else {
                        duration_str(Some(s.now - p.since))
                    }),
                ]
            })
            .collect();
        let tbl = self.table(
            t,
            &[
                "IP",
                "Type",
                "Height",
                "Uploaded",
                "Downloaded",
                "Connected for",
            ],
            &[345.0, 137.0, 112.0, 157.0, 150.0, 235.0],
            rows,
        );
        div().pt(px(16.)).pb(px(64.)).child(header).child(tbl)
    }

    fn tab_title(&self) -> String {
        let s = &*self.snap;
        let now = now_secs();
        let short = |n: u32| {
            if n >= 1000 {
                format!("{:.*}k", if n >= 10000 { 0 } else { 1 }, n as f64 / 1000.0)
            } else {
                n.to_string()
            }
        };
        let dur = |sec: f64| {
            duration_str(Some(sec))
                .replace(" min", "m")
                .replace(" hours", "h")
                .replace(" days", "d")
        };
        if !s.ready {
            return "Testnet monitor".into();
        }
        let sv = &s.services;
        let f = s.farm.as_ref();
        let bad = if now - self.last_ok >= 8.0 {
            return format!(
                "Dashboard down {} · Testnet monitor",
                dur(now - self.last_ok)
            );
        } else if !sv.node || s.node.is_none() {
            Some("Offline")
        } else if !sv.farmer {
            Some("Farmer off")
        } else if f.is_some_and(|f| f.by_version.v2.plots > 0) && !sv.solver {
            Some("Solver off")
        } else {
            None
        };
        if let Some(b) = bad {
            return format!("{b} · Testnet monitor");
        }
        let node = s.node.as_ref().unwrap();
        if !node.synced {
            return format!(
                "{} · Testnet monitor",
                match (100 * node.height).checked_div(node.sync_tip) {
                    Some(pct) => format!("Syncing {pct}%"),
                    None => "Not synced".into(),
                }
            );
        }
        if !s.peers.iter().any(|p| p.kind == "full node") {
            return "No peers · Testnet monitor".into();
        }
        if f.is_none_or(|f| f.plots == 0) {
            return "No plots · Testnet monitor".into();
        }
        if let Some((h, at)) = self.won_at {
            if now - at < 300.0 {
                return format!("Won {} · Testnet monitor", n(h));
            }
        }
        let milestone = s.strength.as_ref().and_then(|st| {
            if node.height < st.fork {
                return Some(format!("fork in {}", short(st.fork - node.height)));
            }
            st.rows
                .iter()
                .filter(|r| r.cls != "v1" && r.unlock > node.height)
                .min_by_key(|r| r.unlock)
                .map(|r| format!("{} in {}", r.cls, short(r.unlock - node.height)))
        });
        match milestone {
            Some(m) => format!("Testnet monitor · {m}"),
            None => format!("Testnet monitor · {}", s.network),
        }
    }
}

fn wrap(pad: f32) -> Div {
    div().w_full().max_w(px(1200.)).mx_auto().px(px(pad))
}

fn n_(v: u32) -> String {
    n(v)
}

fn trim_float(x: f64, max_frac: usize) -> String {
    let s = format!("{x:.max_frac$}");
    let s = if s.contains('.') {
        s.trim_end_matches('0').trim_end_matches('.').to_string()
    } else {
        s
    };
    let (int, frac) = s.split_once('.').map_or((s.clone(), None), |(a, b)| {
        (a.to_string(), Some(b.to_string()))
    });
    let int = n(int.parse::<i64>().unwrap_or(0));
    match frac {
        Some(f) => format!("{int}.{f}"),
        None => int,
    }
}

impl Render for Monitor {
    fn render(&mut self, win: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let t = Theme::new(self.dark);
        let title = self.tab_title();
        if title != self.title {
            win.set_window_title(&title);
            self.title = title;
        }
        let vw: f32 = win.viewport_size().width.into();
        let pad = if vw <= 600.0 { 16.0 } else { 32.0 };
        let narrow = vw <= 860.0;
        let content = vw.min(1200.0) - pad * 2.0;
        let colw = (content - 96.0) / 3.0;

        let ready = self.snap.ready;
        let net = if ready {
            self.snap.network.clone()
        } else {
            "connecting…".into()
        };

        let toggle = {
            let on = self.confetti_on;
            let tip = self.tip.clone();
            hover_tip(
                "confetti-toggle",
                200,
                &tip,
                "Confetti on wins",
                if on {
                    "On. Click to turn off."
                } else {
                    "Off. Click to turn on."
                },
            )
            .ml_auto()
            .mr(px(-7.))
            .w(px(36.))
            .h(px(36.))
            .flex()
            .items_center()
            .justify_center()
            .rounded(px(8.))
            .cursor_pointer()
            .hover(|s| s.bg(t.band))
            .on_click(cx.listener(|this, _, _, cx| {
                this.confetti_on = !this.confetti_on;
                let _ = std::fs::write(pref_path(), if this.confetti_on { "on" } else { "off" });
                if this.confetti_on {
                    this.celebrate(0.35, cx);
                }
                cx.notify();
            }))
            .child(
                svg()
                    .path(if on {
                        "icons/party.svg"
                    } else {
                        "icons/party-off.svg"
                    })
                    .size(px(22.))
                    .text_color(if on { t.yours } else { t.ink3 }),
            )
        };

        let header = wrap(pad)
            .flex()
            .flex_row()
            .flex_wrap()
            .items_center()
            .gap_x(px(28.))
            .gap_y(px(10.))
            .pt(px(22.))
            .pb(px(18.))
            .child(
                div()
                    .flex()
                    .flex_row()
                    .items_center()
                    .gap(px(12.))
                    .font_family(BRAND)
                    .text_size(px(18.))
                    .text_color(t.ink)
                    .child(charts::logo(t))
                    .child("Testnet monitor")
                    .child(
                        div()
                            .ml(px(6.))
                            .px(px(9.))
                            .py(px(3.))
                            .rounded(px(6.))
                            .bg(t.band)
                            .text_color(t.ink2)
                            .font_family(MONO)
                            .text_size(px(13.))
                            .line_height(px(18.2))
                            .child(net),
                    ),
            )
            .child(if ready { self.health(&t) } else { div() })
            .child(toggle);

        let mut page = div().flex().flex_col().w_full().child(header);
        if ready {
            let mut hero = self.hero(&t);
            let _ = &mut hero;
            page = page.child(hero);
            let mut main = wrap(pad).child(self.figures(&t, colw, narrow));
            if let Some(p) = self.plotting(&t) {
                main = main.child(p);
            }
            main = main.child(self.lower(&t, colw, narrow));
            if let Some(s) = self.strength(&t) {
                main = main.child(s);
            }
            main = main.child(self.peers(&t));
            page = page.child(main);
        } else {
            page = page.child(wrap(pad).pt(px(34.)).child(
                div().text_color(t.ink2).text_size(px(17.)).child(format!(
                    "Waiting for the server at {}. Start server.py first.",
                    self.endpoint
                )),
            ));
        }

        div()
            .relative()
            .size_full()
            .bg(t.paper)
            .text_color(t.ink)
            .font_family(REG)
            .text_size(px(15.))
            .line_height(px(22.5))
            .child(div().id("page").size_full().overflow_y_scroll().child(page))
            .child(self.confetti.clone())
            .child(self.tip.clone())
    }
}
