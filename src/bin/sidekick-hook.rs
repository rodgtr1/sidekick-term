/// Claude Code PreToolUse hook.
/// Install: copy to ~/.claude/hooks/PreToolUse/sidekick-hook (chmod +x).
/// Reads JSON from stdin, sends diff to sidekick, exits 0 (accept) or 2 (reject).
use serde::Deserialize;
use serde_json::Value;
use std::io::{BufRead, BufReader, Write};
use std::os::unix::net::UnixStream;

const MAX_HOOK_TEXT_BYTES: u64 = 4 * 1024 * 1024;

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
            if new.len() > MAX_HOOK_TEXT_BYTES as usize {
                eprintln!("sidekick-hook: edit too large to preview");
                std::process::exit(2);
            }
            let old = read_existing_text_limited(&path).unwrap_or_default();
            vec![(path, old, new)]
        }
        "Edit" => {
            let path = str_field(&input.tool_input, "file_path");
            let old_str = str_field(&input.tool_input, "old_string");
            let new_str = str_field(&input.tool_input, "new_string");
            if path.is_empty() {
                return vec![];
            }
            let file_content = match read_existing_text_limited(&path) {
                Some(content) => content,
                None => return vec![],
            };
            // If old_string is not present, the tool call itself will fail —
            // skip the preview rather than showing a misleading no-op diff.
            if !file_content.contains(&old_str) {
                return vec![];
            }
            let replace_all = input.tool_input["replace_all"].as_bool().unwrap_or(false);
            let new_content = if replace_all {
                file_content.replace(&old_str, &new_str)
            } else {
                file_content.replacen(&old_str, &new_str, 1)
            };
            if new_content.len() > MAX_HOOK_TEXT_BYTES as usize {
                eprintln!("sidekick-hook: edit too large to preview");
                std::process::exit(2);
            }
            vec![(path, file_content, new_content)]
        }
        "MultiEdit" => {
            let path = str_field(&input.tool_input, "file_path");
            if path.is_empty() {
                return vec![];
            }
            let mut current = match read_existing_text_limited(&path) {
                Some(content) => content,
                None => return vec![],
            };
            let original = current.clone();
            if let Some(edits) = input.tool_input["edits"].as_array() {
                for edit in edits {
                    let old_str = edit["old_string"].as_str().unwrap_or("").to_string();
                    let new_str = edit["new_string"].as_str().unwrap_or("").to_string();
                    // Any edit that won't apply makes the simulated result
                    // diverge from what the tool will do — skip the preview.
                    if !current.contains(&old_str) {
                        return vec![];
                    }
                    let replace_all = edit["replace_all"].as_bool().unwrap_or(false);
                    current = if replace_all {
                        current.replace(&old_str, &new_str)
                    } else {
                        current.replacen(&old_str, &new_str, 1)
                    };
                    if current.len() > MAX_HOOK_TEXT_BYTES as usize {
                        eprintln!("sidekick-hook: edit too large to preview");
                        std::process::exit(2);
                    }
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

fn read_existing_text_limited(path: &str) -> Option<String> {
    let meta = std::fs::metadata(path).ok()?;
    if !meta.is_file() || meta.len() > MAX_HOOK_TEXT_BYTES {
        return None;
    }
    let content = std::fs::read(path).ok()?;
    if content.iter().take(8192).any(|b| *b == 0) {
        return None;
    }
    String::from_utf8(content).ok()
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
