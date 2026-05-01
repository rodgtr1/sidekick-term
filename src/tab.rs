use crate::config::Config;
use std::collections::HashMap;
use std::sync::{Mutex, OnceLock};
use std::time::{Duration, Instant};
use vte4::prelude::*;
use vte4::Terminal;

type BranchCache = HashMap<String, (Instant, Option<String>)>;

static BRANCH_CACHE: OnceLock<Mutex<BranchCache>> = OnceLock::new();
const BRANCH_CACHE_TTL: Duration = Duration::from_secs(5);

/// Creates and configures a terminal widget. Does not spawn the shell —
/// caller is responsible for spawning so it can capture the child PID.
pub fn build(cfg: &Config) -> Terminal {
    let terminal = Terminal::new();

    crate::theme::apply(&terminal, &cfg.theme.name, cfg.theme.opacity);

    let font_str = format!("{} {}", cfg.font.family, cfg.font.size);
    terminal.set_font(Some(&pango::FontDescription::from_string(&font_str)));
    terminal.set_bold_is_bright(cfg.font.bold_is_bright);

    terminal.set_cursor_shape(match cfg.cursor.shape.as_str() {
        "ibeam" => vte4::CursorShape::Ibeam,
        "underline" => vte4::CursorShape::Underline,
        _ => vte4::CursorShape::Block,
    });
    terminal.set_cursor_blink_mode(if cfg.cursor.blink {
        vte4::CursorBlinkMode::On
    } else {
        vte4::CursorBlinkMode::Off
    });

    terminal.set_scrollback_lines(cfg.behavior.scrollback_lines);
    terminal.set_scroll_on_output(cfg.behavior.scroll_on_output);
    terminal.set_scroll_on_keystroke(cfg.behavior.scroll_on_keystroke);
    terminal.set_allow_hyperlink(cfg.behavior.allow_hyperlinks);
    terminal.set_mouse_autohide(cfg.behavior.mouse_autohide);
    terminal.set_audible_bell(cfg.behavior.audible_bell);

    terminal
}

/// Builds the tab label string from the shell's PID.
/// Shows basename of cwd + git branch if inside a repo.
pub fn tab_title(pid: i32) -> String {
    let home = std::env::var("HOME").unwrap_or_default();

    let cwd = match std::fs::read_link(format!("/proc/{}/cwd", pid)) {
        Ok(p) => p,
        Err(_) => return "~".to_string(),
    };

    let branch = branch_for_cwd(cwd.to_str().unwrap_or("."));

    let cwd_str = cwd.to_string_lossy();
    let short = if cwd_str == home {
        "~".to_string()
    } else {
        cwd.file_name()
            .map(|n| n.to_string_lossy().to_string())
            .unwrap_or_else(|| "~".to_string())
    };

    match branch {
        Some(b) => format!("  {} [{}]  ", short, b),
        None => format!("  {}  ", short),
    }
}

fn branch_for_cwd(cwd: &str) -> Option<String> {
    let cache = BRANCH_CACHE.get_or_init(|| Mutex::new(HashMap::new()));
    let now = Instant::now();
    if let Ok(cache) = cache.lock() {
        if let Some((checked_at, branch)) = cache.get(cwd) {
            if now.duration_since(*checked_at) < BRANCH_CACHE_TTL {
                return branch.clone();
            }
        }
    }

    let branch = std::process::Command::new("git")
        .args(["-C", cwd, "branch", "--show-current"])
        .output()
        .ok()
        .filter(|o| o.status.success())
        .and_then(|o| String::from_utf8(o.stdout).ok())
        .map(|s| s.trim().to_string())
        .filter(|s| !s.is_empty());

    if let Ok(mut cache) = cache.lock() {
        cache.insert(cwd.to_string(), (now, branch.clone()));
    }
    branch
}
