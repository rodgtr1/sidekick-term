use serde::Deserialize;
use std::fs;
use std::path::PathBuf;

#[derive(Deserialize, Default, Clone)]
#[serde(default)]
pub struct Config {
    pub theme: ThemeConfig,
    pub font: FontConfig,
    pub cursor: CursorConfig,
    pub window: WindowConfig,
    pub behavior: BehaviorConfig,
    pub editor: EditorConfig,
    pub tasks: Vec<crate::runpanel::Task>,
}

#[derive(Deserialize, Clone)]
#[serde(default)]
pub struct ThemeConfig {
    pub name: String,
    pub opacity: f32,
}

#[derive(Deserialize, Clone)]
#[serde(default)]
pub struct FontConfig {
    pub family: String,
    pub size: u32,
    pub bold_is_bright: bool,
}

#[derive(Deserialize, Clone)]
#[serde(default)]
pub struct CursorConfig {
    // block | ibeam | underline
    pub shape: String,
    pub blink: bool,
}

#[derive(Deserialize, Clone)]
#[serde(default)]
pub struct WindowConfig {
    pub padding: u32,
}

#[derive(Deserialize, Clone)]
#[serde(default)]
pub struct BehaviorConfig {
    pub scrollback_lines: i64,
    pub scroll_on_output: bool,
    pub scroll_on_keystroke: bool,
    pub allow_hyperlinks: bool,
    pub mouse_autohide: bool,
    pub audible_bell: bool,
    pub restore_session: bool,
}

#[derive(Deserialize, Clone)]
#[serde(default)]
pub struct EditorConfig {
    // builtin | nvim
    pub file_manager_open: String,
    pub word_wrap: bool,
}

impl Default for ThemeConfig {
    fn default() -> Self {
        Self {
            name: "catppuccin-mocha".to_string(),
            opacity: 0.9,
        }
    }
}

impl Default for FontConfig {
    fn default() -> Self {
        Self {
            family: "Monospace".to_string(),
            size: 15,
            bold_is_bright: true,
        }
    }
}

impl Default for CursorConfig {
    fn default() -> Self {
        Self {
            shape: "block".to_string(),
            blink: true,
        }
    }
}

impl Default for WindowConfig {
    fn default() -> Self {
        Self { padding: 8 }
    }
}

impl Default for BehaviorConfig {
    fn default() -> Self {
        Self {
            scrollback_lines: 10000,
            scroll_on_output: false,
            scroll_on_keystroke: true,
            allow_hyperlinks: true,
            mouse_autohide: true,
            audible_bell: false,
            restore_session: true,
        }
    }
}

impl Default for EditorConfig {
    fn default() -> Self {
        Self {
            file_manager_open: "builtin".to_string(),
            word_wrap: true,
        }
    }
}

const DEFAULT_CONFIG: &str = r#"[theme]
# Available themes: catppuccin-mocha
name = "catppuccin-mocha"
# Background opacity: 0.0 (fully transparent) to 1.0 (fully opaque)
opacity = 0.9

[font]
# Font family — use any monospace font installed on your system
family = "Monospace"
# Size in points
size = 15
# Bold text uses bright palette colors (like most terminals)
bold_is_bright = true

[cursor]
# shape: block | ibeam | underline
shape = "block"
blink = true

[window]
# Inner padding around the terminal content (pixels)
padding = 8

[behavior]
# Lines of scrollback (-1 for unlimited)
scrollback_lines = 10000
# Scroll to bottom when new output appears
scroll_on_output = false
# Scroll to bottom when you type
scroll_on_keystroke = true
# Clickable URLs
allow_hyperlinks = true
# Hide mouse cursor while typing
mouse_autohide = true
audible_bell = false
# Reopen tabs (and their split layout / directories) from the previous session
restore_session = true

[editor]
# File manager activation opens files in: builtin | nvim
file_manager_open = "builtin"
# Wrap long lines in the editor (true = word wrap, false = horizontal scroll)
word_wrap = true

# Global run-panel tasks (available in every project)
# [[tasks]]
# name = "My task"
# cmd  = "echo hello"
"#;

impl Config {
    /// Startup load: never fails, falls back to defaults on any error.
    pub fn load() -> Self {
        Self::load_checked().unwrap_or_default()
    }

    /// Load returning an error for malformed TOML, so reloads can keep the
    /// previous in-memory config instead of silently resetting to defaults.
    pub fn load_checked() -> Result<Self, String> {
        let path = config_path();

        if !path.exists() {
            if let Some(parent) = path.parent() {
                let _ = fs::create_dir_all(parent);
            }
            let _ = fs::write(&path, DEFAULT_CONFIG);
            return Ok(Self::default());
        }

        let content = fs::read_to_string(&path).map_err(|e| e.to_string())?;
        parse_config(&content)
    }
}

pub fn parse_config(content: &str) -> Result<Config, String> {
    toml::from_str(content).map_err(|e| e.to_string())
}

pub fn config_path() -> PathBuf {
    let config_dir = std::env::var("XDG_CONFIG_HOME")
        .map(PathBuf::from)
        .unwrap_or_else(|_| {
            let home = std::env::var("HOME").unwrap_or_else(|_| ".".to_string());
            PathBuf::from(home).join(".config")
        });
    config_dir.join("sidekick").join("config.toml")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_valid_config() {
        let cfg = parse_config("[font]\nsize = 20\n").expect("valid");
        assert_eq!(cfg.font.size, 20);
    }

    #[test]
    fn invalid_config_is_err() {
        assert!(parse_config("this is = = not toml").is_err());
    }
}
