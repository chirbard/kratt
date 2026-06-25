use std::io::Write;
use serde::{Deserialize, Serialize};
use crate::graph::DATA_DIR;

const CHAT_LOG: &str = "kratt-data/chat.jsonl";

#[derive(Serialize, Deserialize)]
pub struct Message {
    pub role: String,
    pub content: String,
    pub timestamp: String,
}

fn now_iso() -> String {
    use std::time::{SystemTime, UNIX_EPOCH};
    let total_secs = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs();
    let secs_since_midnight = total_secs % 86400;
    let hour = secs_since_midnight / 3600;
    let minute = (secs_since_midnight % 3600) / 60;
    let second = secs_since_midnight % 60;
    let days = (total_secs / 86400) as i64;
    let z = days + 719468;
    let civil_era = (if z >= 0 { z } else { z - 146096 }) / 146097;
    let day_of_era = z - civil_era * 146097;
    let year_of_era =
        (day_of_era - day_of_era / 1460 + day_of_era / 36524 - day_of_era / 146096) / 365;
    let y = year_of_era + civil_era * 400;
    let day_of_year = day_of_era - (365 * year_of_era + year_of_era / 4 - year_of_era / 100);
    let month_prime = (5 * day_of_year + 2) / 153;
    let day = day_of_year - (153 * month_prime + 2) / 5 + 1;
    let month = if month_prime < 10 { month_prime + 3 } else { month_prime - 9 };
    let y = if month <= 2 { y + 1 } else { y };
    format!("{y:04}-{month:02}-{day:02}T{hour:02}:{minute:02}:{second:02}Z")
}

pub fn append(role: &str, content: &str) {
    let msg = Message { role: role.to_string(), content: content.to_string(), timestamp: now_iso() };
    let line = match serde_json::to_string(&msg) {
        Ok(json) => json,
        Err(_) => return,
    };
    let _ = std::fs::create_dir_all(DATA_DIR);
    if let Ok(mut file) = std::fs::OpenOptions::new().create(true).append(true).open(CHAT_LOG) {
        let _ = writeln!(file, "{line}");
    }
}

pub fn load_recent(count: usize) -> Vec<Message> {
    let text = match std::fs::read_to_string(CHAT_LOG) {
        Ok(text) => text,
        Err(_) => return Vec::new(),
    };
    let mut messages: Vec<Message> = Vec::new();
    for line in text.lines() {
        if let Ok(msg) = serde_json::from_str(line) {
            messages.push(msg);
        }
    }
    if messages.len() > count {
        messages.drain(..messages.len() - count);
    }
    messages
}

pub fn format_context(messages: &[Message]) -> String {
    if messages.is_empty() {
        return String::new();
    }
    let mut out = String::from("[Recent conversation]\n");
    for msg in messages {
        let role = if msg.role == "user" { "User" } else { "Assistant" };
        out.push_str(role);
        out.push_str(": ");
        out.push_str(&msg.content);
        out.push('\n');
    }
    out
}
