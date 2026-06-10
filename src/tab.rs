use crate::config::Config;
use gtk4::{gdk, prelude::*};
use std::collections::HashMap;
use std::sync::{Mutex, OnceLock};
use std::time::{Duration, Instant};
use vte4::prelude::*;
use vte4::Terminal;

type BranchCache = HashMap<String, (Instant, Option<String>)>;

static BRANCH_CACHE: OnceLock<Mutex<BranchCache>> = OnceLock::new();
const BRANCH_CACHE_TTL: Duration = Duration::from_secs(5);

/// App-wide font zoom factor (runtime-only), stored as f64 bits so terminals
/// created later pick up the current zoom.
static FONT_ZOOM_BITS: std::sync::atomic::AtomicU64 =
    std::sync::atomic::AtomicU64::new(0x3FF0_0000_0000_0000); // 1.0f64

pub const ZOOM_MIN: f64 = 0.4;
pub const ZOOM_MAX: f64 = 4.0;
pub const ZOOM_STEP: f64 = 1.1;

pub fn font_zoom() -> f64 {
    f64::from_bits(FONT_ZOOM_BITS.load(std::sync::atomic::Ordering::Relaxed))
}

pub fn set_font_zoom(zoom: f64) -> f64 {
    let zoom = zoom.clamp(ZOOM_MIN, ZOOM_MAX);
    FONT_ZOOM_BITS.store(zoom.to_bits(), std::sync::atomic::Ordering::Relaxed);
    zoom
}

/// Creates and configures a terminal widget. Does not spawn the shell —
/// caller is responsible for spawning so it can capture the child PID.
pub fn build(cfg: &Config) -> Terminal {
    let terminal = Terminal::new();

    apply_config(&terminal, cfg);
    terminal.set_font_scale(font_zoom());

    // Register a URL regex so plain https?:// links get a pointer cursor on hover.
    if let Ok(re) = vte4::Regex::for_match("https?://[^\\s\\])'\">\x01-\x1f]+", 0) {
        let tag = terminal.match_add_regex(&re, 0);
        terminal.match_set_cursor_name(tag, "pointer");
    }

    // Ctrl+click opens hyperlinks (OSC 8) and regex-matched URLs via xdg-open.
    {
        let term_c = terminal.clone();
        let gesture = gtk4::GestureClick::new();
        gesture.connect_pressed(move |gesture, _n_press, x, y| {
            let mods = gesture.current_event_state();
            if !mods.contains(gdk::ModifierType::CONTROL_MASK) {
                return;
            }
            let uri = term_c
                .check_hyperlink_at(x, y)
                .or_else(|| term_c.check_match_at(x, y).0);
            if let Some(uri) = uri {
                let _ = std::process::Command::new("xdg-open")
                    .arg(uri.as_str())
                    .spawn();
                gesture.set_state(gtk4::EventSequenceState::Claimed);
            }
        });
        terminal.add_controller(gesture);
    }

    terminal
}

pub fn apply_config(terminal: &Terminal, cfg: &Config) {
    crate::theme::apply(terminal, &cfg.theme.name, cfg.theme.opacity);

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
}

/// Builds the tab title and detail text from the shell's PID.
/// Shows basename of cwd, plus branch and compact cwd details.
pub fn tab_title_parts(pid: i32) -> (String, String) {
    let home = std::env::var("HOME").unwrap_or_default();

    let cwd = match std::fs::read_link(format!("/proc/{}/cwd", pid)) {
        Ok(p) => p,
        Err(_) => return ("~".to_string(), "~".to_string()),
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

    let compact_cwd = if cwd_str == home {
        "~".to_string()
    } else if !home.is_empty() && cwd_str.starts_with(&format!("{home}/")) {
        cwd_str.replacen(&home, "~", 1)
    } else {
        cwd_str.to_string()
    };

    let detail = match branch {
        Some(b) => format!("{} - {}", b, compact_cwd),
        None => compact_cwd,
    };

    (short, detail)
}

/// Returns the cached branch immediately (possibly stale or None) and, when
/// the cache entry is expired, refreshes it on a background thread. This is
/// called from the 500ms UI tick, so it must never run git synchronously.
fn branch_for_cwd(cwd: &str) -> Option<String> {
    let cache = BRANCH_CACHE.get_or_init(|| Mutex::new(HashMap::new()));
    let now = Instant::now();

    let (cached, needs_refresh) = match cache.lock() {
        Ok(cache) => match cache.get(cwd) {
            Some((checked_at, branch)) => (
                branch.clone(),
                now.duration_since(*checked_at) >= BRANCH_CACHE_TTL,
            ),
            None => (None, true),
        },
        Err(_) => return None,
    };

    if needs_refresh {
        // Stamp the entry now so concurrent ticks don't spawn duplicate threads.
        if let Ok(mut cache) = cache.lock() {
            cache.insert(cwd.to_string(), (now, cached.clone()));
        }
        let cwd = cwd.to_string();
        std::thread::spawn(move || {
            let branch = std::process::Command::new("git")
                .args(["-C", &cwd, "branch", "--show-current"])
                .output()
                .ok()
                .filter(|o| o.status.success())
                .and_then(|o| String::from_utf8(o.stdout).ok())
                .map(|s| s.trim().to_string())
                .filter(|s| !s.is_empty());
            let cache = BRANCH_CACHE.get_or_init(|| Mutex::new(HashMap::new()));
            if let Ok(mut cache) = cache.lock() {
                cache.insert(cwd, (Instant::now(), branch));
            }
        });
    }

    cached
}
