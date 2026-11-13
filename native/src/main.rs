mod charts;
mod fmtx;
mod model;
mod paint;
mod theme;
mod view;
mod widgets;

use std::borrow::Cow;

use gpui_kit::*;

struct Assets;

impl AssetSource for Assets {
    fn load(&self, path: &str) -> anyhow::Result<Option<Cow<'static, [u8]>>> {
        Ok(match path {
            "icons/party.svg" => Some(Cow::Borrowed(
                include_bytes!("../assets/icons/party.svg").as_slice(),
            )),
            "icons/party-off.svg" => Some(Cow::Borrowed(
                include_bytes!("../assets/icons/party-off.svg").as_slice(),
            )),
            _ => None,
        })
    }
    fn list(&self, _path: &str) -> anyhow::Result<Vec<SharedString>> {
        Ok(vec![])
    }
}

/// Where to find server.py: `--url`, then $CHIA_MONITOR_URL, then the first line of
/// `~/.chia-monitor-native-url` (the way to set it for a .app launched from Finder), then the default.
fn server_url() -> String {
    let flag = std::env::args().skip_while(|a| a != "--url").nth(1);
    let env = std::env::var("CHIA_MONITOR_URL").ok();
    let file = view::home_file(".chia-monitor-native-url")
        .and_then(|p| std::fs::read_to_string(p).ok())
        .and_then(|s| s.lines().next().map(|l| l.trim().to_string()));
    flag.or(env)
        .or(file)
        .filter(|u| !u.is_empty())
        .unwrap_or_else(|| "http://127.0.0.1:8899".into())
}

fn main() {
    let url = server_url();
    gpui_kit::application().with_assets(Assets).run(move |cx| {
        let fonts = theme::FONT_FILES
            .iter()
            .map(|f| Cow::Borrowed(*f))
            .collect();
        cx.text_system().add_fonts(fonts).expect("fonts");
        cx.on_window_closed(|cx, _| {
            if cx.windows().is_empty() {
                cx.quit();
            }
        })
        .detach();
        let (w, h) = std::env::args()
            .skip_while(|a| a != "--size")
            .nth(1)
            .and_then(|s| {
                s.split_once('x')
                    .and_then(|(a, b)| Some((a.parse().ok()?, b.parse().ok()?)))
            })
            .unwrap_or((1200.0f32, 1000.0f32));
        let opts = WindowOptions {
            window_bounds: Some(WindowBounds::Windowed(Bounds::centered(
                None,
                size(px(w), px(h)),
                cx,
            ))),
            titlebar: Some(TitlebarOptions {
                title: Some("Testnet monitor".into()),
                ..Default::default()
            }),
            ..Default::default()
        };
        cx.open_window(opts, move |win, cx| {
            cx.new(|cx| view::Monitor::new(url.clone(), win, cx))
        })
        .expect("window");
        cx.activate(true);
    });
}
