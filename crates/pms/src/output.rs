use clap::ValueEnum;
use comfy_table::{presets::NOTHING, Table};
use serde::Serialize;

#[derive(Debug, Clone, Copy, PartialEq, Eq, ValueEnum)]
pub enum Format {
    Table,
    Json,
}

pub fn print_json<T: Serialize + ?Sized>(value: &T) {
    // Serializing our own structs cannot fail.
    println!(
        "{}",
        serde_json::to_string_pretty(value).expect("serialize")
    );
}

pub fn print_table(headers: &[&str], rows: Vec<Vec<String>>) {
    let mut table = Table::new();
    table.load_style(NOTHING);
    table.set_header(headers.iter().map(|h| h.to_uppercase()));
    for row in rows {
        table.add_row(row);
    }
    println!("{table}");
}

pub fn print_kv(rows: Vec<(&str, String)>) {
    let mut table = Table::new();
    table.load_style(NOTHING);
    for (k, v) in rows {
        table.add_row(vec![format!("{k}:"), v]);
    }
    println!("{table}");
}

pub fn opt(value: &Option<String>) -> String {
    value.clone().unwrap_or_else(|| "-".into())
}

/// `2026-10-10T12:34:56.789+00:00` becomes `2026-10-10 12:34:56`.
pub fn ts(value: &str) -> String {
    value.chars().take(19).collect::<String>().replace('T', " ")
}

pub fn ts_opt(value: &Option<String>) -> String {
    value.as_deref().map(ts).unwrap_or_else(|| "-".into())
}

pub fn status_text(status: &str, message: &Option<String>) -> String {
    match message.as_deref() {
        Some(m) if !m.is_empty() => format!("{status} ({m})"),
        _ => status.to_owned(),
    }
}

/// Secrets show one time. The API does not return them again, so say it where the user looks.
pub fn warn_one_time(what: &str) {
    eprintln!("Save the {what} now. It is shown one time and cannot be read again.");
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn timestamps_are_short() {
        assert_eq!(ts("2026-10-10T12:34:56.789+00:00"), "2026-10-10 12:34:56");
        assert_eq!(ts("short"), "short");
    }

    #[test]
    fn status_shows_message() {
        assert_eq!(status_text("failed", &Some("boom".into())), "failed (boom)");
        assert_eq!(status_text("running", &None), "running");
    }
}
