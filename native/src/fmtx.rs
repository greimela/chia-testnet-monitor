//! Number and time formatting, matching the page's helpers (`fmt`, `bytes`, `duration`, ...).

use std::time::{SystemTime, UNIX_EPOCH};

pub fn now_secs() -> f64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs_f64())
        .unwrap_or(0.0)
}

/// 12345 -> "12,345"
pub fn n(v: impl Into<i128>) -> String {
    let v: i128 = v.into();
    let s = v.unsigned_abs().to_string();
    let mut out = String::with_capacity(s.len() + s.len() / 3 + 1);
    if v < 0 {
        out.push('-');
    }
    for (i, ch) in s.chars().enumerate() {
        if i > 0 && (s.len() - i) % 3 == 0 {
            out.push(',');
        }
        out.push(ch);
    }
    out
}

pub fn plural(count: u32, word: &str) -> String {
    if count == 1 {
        word.to_string()
    } else {
        format!("{word}s")
    }
}

/// (number, unit) like the page's `bytes()`.
pub fn bytes(b: Option<f64>) -> (String, &'static str) {
    let Some(mut b) = b else {
        return ("–".into(), "");
    };
    const U: [&str; 6] = ["B", "KiB", "MiB", "GiB", "TiB", "PiB"];
    let mut i = 0;
    while b >= 1024.0 && i < U.len() - 1 {
        b /= 1024.0;
        i += 1;
    }
    (
        if b >= 100.0 {
            format!("{b:.0}")
        } else {
            format!("{b:.1}")
        },
        U[i],
    )
}

pub fn bytes_str(b: f64) -> String {
    let (a, u) = bytes(Some(b));
    format!("{a} {u}")
}

/// (number, unit) like `duration()`: "11" "min", "2.1" "hours".
pub fn duration(sec: Option<f64>) -> (String, String) {
    let Some(sec) = sec.filter(|s| s.is_finite()) else {
        return ("–".into(), String::new());
    };
    let m = sec / 60.0;
    if m < 90.0 {
        return (format!("{}", m.round().max(1.0) as i64), "min".into());
    }
    let h = m / 60.0;
    if h < 48.0 {
        (format!("{h:.1}"), "hours".into())
    } else {
        (format!("{:.1}", h / 24.0), "days".into())
    }
}

pub fn duration_str(sec: Option<f64>) -> String {
    let (a, u) = duration(sec);
    if u.is_empty() { a } else { format!("{a} {u}") }
}

pub fn ms(sec: f64) -> String {
    if sec >= 1.0 {
        format!("{sec:.1} s")
    } else {
        format!("{:.*} ms", if sec < 0.01 { 1 } else { 0 }, sec * 1000.0)
    }
}

// ---- local time ----

use chrono::{DateTime, Datelike, Local as ChronoLocal, Timelike};

#[derive(Clone, Copy)]
pub struct Local {
    pub hour: i32,
    pub min: i32,
    pub mday: i32,
    /// 0-based month
    pub mon: i32,
    /// 0 = Sunday
    pub wday: i32,
    pub yday: i32,
    pub year: i32,
}

pub fn local(t: f64) -> Local {
    let dt: DateTime<ChronoLocal> = DateTime::from_timestamp(t as i64, 0)
        .unwrap_or_default()
        .with_timezone(&ChronoLocal);
    Local {
        hour: dt.hour() as i32,
        min: dt.minute() as i32,
        mday: dt.day() as i32,
        mon: dt.month0() as i32,
        wday: dt.weekday().num_days_from_sunday() as i32,
        yday: dt.ordinal0() as i32,
        year: dt.year(),
    }
}

/// "10:55 AM", as `toLocaleTimeString([], {hour:"2-digit", minute:"2-digit"})` prints in en-US.
pub fn clock(t: f64) -> String {
    let l = local(t);
    let h12 = if l.hour % 12 == 0 { 12 } else { l.hour % 12 };
    format!(
        "{:02}:{:02} {}",
        h12,
        l.min,
        if l.hour < 12 { "AM" } else { "PM" }
    )
}

/// "7:01 AM"
fn clock_short(t: f64) -> String {
    let l = local(t);
    let h12 = if l.hour % 12 == 0 { 12 } else { l.hour % 12 };
    format!(
        "{}:{:02} {}",
        h12,
        l.min,
        if l.hour < 12 { "AM" } else { "PM" }
    )
}

const WEEKDAYS: [&str; 7] = [
    "Sunday",
    "Monday",
    "Tuesday",
    "Wednesday",
    "Thursday",
    "Friday",
    "Saturday",
];
const MONTHS: [&str; 12] = [
    "Jan", "Feb", "Mar", "Apr", "May", "Jun", "Jul", "Aug", "Sep", "Oct", "Nov", "Dec",
];

/// "around 7:01 AM on Sunday"
pub fn when_phrase(t: f64, now: f64) -> String {
    let d = local(t);
    let a = local(now);
    let time = clock_short(t);
    let day_index = |l: Local| l.year * 400 + l.yday;
    let days = day_index(d) - day_index(a);
    if days == 0 {
        format!(
            "around {time} {}",
            if d.hour >= 18 { "tonight" } else { "today" }
        )
    } else if days == 1 {
        format!("around {time} tomorrow")
    } else {
        format!("around {time} on {}", WEEKDAYS[d.wday as usize % 7])
    }
}

/// "Nov 13, 7:35 AM"
pub fn month_day_time(t: f64) -> String {
    let l = local(t);
    format!(
        "{} {}, {}",
        MONTHS[l.mon as usize % 12],
        l.mday,
        clock_short(t)
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn groups_thousands() {
        assert_eq!(n(0u32), "0");
        assert_eq!(n(999u32), "999");
        assert_eq!(n(1_234_567u32), "1,234,567");
        assert_eq!(n(-12_345i64), "-12,345");
    }

    #[test]
    fn bytes_match_the_page() {
        assert_eq!(bytes(None), ("–".to_string(), ""));
        assert_eq!(bytes(Some(512.0)), ("512".to_string(), "B"));
        assert_eq!(bytes(Some(1536.0)), ("1.5".to_string(), "KiB"));
        assert_eq!(bytes_str(73_836_799_637.0), "68.8 GiB");
        assert_eq!(bytes_str(150.0 * 1024.0 * 1024.0), "150 MiB");
    }

    #[test]
    fn durations_switch_units() {
        assert_eq!(duration_str(Some(30.0)), "1 min");
        assert_eq!(duration_str(Some(45.0 * 60.0)), "45 min");
        assert_eq!(duration_str(Some(2.0 * 3600.0)), "2.0 hours");
        assert_eq!(duration_str(Some(3.0 * 86400.0)), "3.0 days");
        assert_eq!(duration_str(None), "–");
        assert_eq!(duration_str(Some(f64::INFINITY)), "–");
    }

    #[test]
    fn lookup_times() {
        assert_eq!(ms(0.00213), "2.1 ms");
        assert_eq!(ms(0.142), "142 ms");
        assert_eq!(ms(1.25), "1.2 s");
    }

    #[test]
    fn plurals() {
        assert_eq!(plural(1, "plot"), "plot");
        assert_eq!(plural(0, "plot"), "plots");
    }

    #[test]
    fn clock_is_twelve_hour() {
        // Whatever the local zone, the shape is "hh:mm AM|PM".
        let c = clock(1_794_588_777.0);
        assert_eq!(c.len(), 8);
        assert!(c.ends_with("AM") || c.ends_with("PM"));
    }

    #[test]
    fn when_phrase_days() {
        let now = 1_794_588_777.0;
        assert!(when_phrase(now + 60.0, now).starts_with("around "));
        assert!(when_phrase(now + 86_400.0, now).ends_with("tomorrow"));
        assert!(when_phrase(now + 3.0 * 86_400.0, now).contains(" on "));
    }
}
