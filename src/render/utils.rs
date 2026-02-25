use std::time::{SystemTime, UNIX_EPOCH};

use maud::PreEscaped;

pub const SEP: PreEscaped<&str> = PreEscaped("\u{00b7}");

pub fn fmt_num(n: u64) -> String {
    let s = n.to_string();
    let mut result = String::with_capacity(s.len() + s.len() / 3);
    for (i, c) in s.chars().enumerate() {
        if i > 0 && (s.len() - i) % 3 == 0 {
            result.push(',');
        }
        result.push(c);
    }
    result
}

// Duplicated in static/app.js for client-side refresh — keep both in sync.
const TIME_UNITS: &[(u64, &str)] = &[
    (31536000, "y"),
    (2592000, "mo"),
    (604800, "w"),
    (86400, "d"),
    (3600, "h"),
    (60, "m"),
];

pub fn format_time_ago(unix_time: u64) -> String {
    let now = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs();
    let elapsed = now.saturating_sub(unix_time);

    for &(secs, suffix) in TIME_UNITS {
        let count = elapsed / secs;
        if count > 0 {
            return format!("{count}{suffix}");
        }
    }
    "0m".to_string()
}
