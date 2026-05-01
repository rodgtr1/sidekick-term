/// Claude Code PreToolUse hook.
/// Install: copy to ~/.claude/hooks/PreToolUse/sidekick-hook (chmod +x).
/// Reads JSON from stdin, sends diff to sidekick, exits 0 (accept) or 2 (reject).
use serde::Deserialize;
use serde_json::Value;
use std::io::{BufRead, BufReader, Write};
use std::os::unix::net::UnixStream;

#[derive(Deserialize)]
struct HookInput {
    tool_name: String,
    tool_input: Value,
}

fn socket_path() -> std::path::PathBuf {
    let home = std::env::var("HOME").unwrap_or_else(|_| "/tmp".to_string());
    std::path::PathBuf::from(home).join(".local/run/sidekick.sock")
}

fn main() {
    let stdin = std::io::stdin();
    let mut raw = String::new();
    for line in stdin.lock().lines() {
        match line {
            Ok(l) => {
                raw.push_str(&l);
                raw.push('\n');
            }
            Err(_) => break,
        }
    }

    let input: HookInput = match serde_json::from_str(&raw) {
        Ok(v) => v,
        Err(_) => std::process::exit(0), // not our problem, let Claude proceed
    };

    let edits = extract_edits(&input);
    if edits.is_empty() {
        std::process::exit(0);
    }

    // Show each edit one at a time; if any is rejected, exit 2 immediately
    for (path, old, new_content) in edits {
        match show_diff(&path, &old, &new_content) {
            Some(true) => {} // accepted, continue
            Some(false) => {
                eprintln!("sidekick-hook: edit rejected by user");
                std::process::exit(2);
            }
            None => {
                // sidekick not running — fall through and let Claude proceed
            }
        }
    }

    std::process::exit(0);
}

fn extract_edits(input: &HookInput) -> Vec<(String, String, String)> {
    match input.tool_name.as_str() {
        "Write" => {
            let path = str_field(&input.tool_input, "file_path");
            let new = str_field(&input.tool_input, "content");
            if path.is_empty() {
                return vec![];
            }
            let old = std::fs::read_to_string(&path).unwrap_or_default();
            vec![(path, old, new)]
        }
        "Edit" => {
            let path = str_field(&input.tool_input, "file_path");
            let old_str = str_field(&input.tool_input, "old_string");
            let new_str = str_field(&input.tool_input, "new_string");
            if path.is_empty() {
                return vec![];
            }
            let file_content = std::fs::read_to_string(&path).unwrap_or_default();
            let new_content = file_content.replacen(&old_str, &new_str, 1);
            vec![(path, file_content, new_content)]
        }
        "MultiEdit" => {
            let path = str_field(&input.tool_input, "file_path");
            if path.is_empty() {
                return vec![];
            }
            let mut current = std::fs::read_to_string(&path).unwrap_or_default();
            let original = current.clone();
            if let Some(edits) = input.tool_input["edits"].as_array() {
                for edit in edits {
                    let old_str = edit["old_string"].as_str().unwrap_or("").to_string();
                    let new_str = edit["new_string"].as_str().unwrap_or("").to_string();
                    current = current.replacen(&old_str, &new_str, 1);
                }
            }
            vec![(path, original, current)]
        }
        _ => vec![],
    }
}

fn str_field(v: &Value, key: &str) -> String {
    v[key].as_str().unwrap_or("").to_string()
}

fn show_diff(path: &str, old: &str, new_content: &str) -> Option<bool> {
    let mut stream = UnixStream::connect(socket_path()).ok()?;

    let payload = serde_json::json!({
        "action": "show_diff",
        "path":   path,
        "old":    old,
        "new":    new_content,
    });
    writeln!(stream, "{}", payload).ok()?;

    let mut reader = BufReader::new(stream);
    let mut line = String::new();
    reader.read_line(&mut line).ok()?;

    let resp: Value = serde_json::from_str(line.trim()).ok()?;
    if resp["ok"].as_bool() != Some(true) {
        return None;
    }
    resp["accepted"].as_bool()
}
