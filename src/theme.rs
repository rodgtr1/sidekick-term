use gtk4::gdk::RGBA;
use vte4::prelude::*;
use vte4::Terminal;

pub fn apply(terminal: &Terminal, name: &str, opacity: f32) {
    match name {
        "catppuccin-mocha" => catppuccin_mocha(terminal, opacity),
        _ => catppuccin_mocha(terminal, opacity),
    }
}

fn rgba(hex: &str, alpha: f32) -> RGBA {
    let r = u8::from_str_radix(&hex[1..3], 16).unwrap() as f32 / 255.0;
    let g = u8::from_str_radix(&hex[3..5], 16).unwrap() as f32 / 255.0;
    let b = u8::from_str_radix(&hex[5..7], 16).unwrap() as f32 / 255.0;
    RGBA::new(r, g, b, alpha)
}

fn c(hex: &str) -> RGBA {
    rgba(hex, 1.0)
}

fn catppuccin_mocha(terminal: &Terminal, opacity: f32) {
    let fg = c("#cdd6f4");
    let bg = rgba("#1e1e2e", opacity);

    let owned = [
        c("#45475a"), // 0  black        (Surface1)
        c("#f38ba8"), // 1  red
        c("#a6e3a1"), // 2  green
        c("#f9e2af"), // 3  yellow
        c("#89b4fa"), // 4  blue
        c("#cba6f7"), // 5  magenta      (Mauve)
        c("#94e2d5"), // 6  cyan         (Teal)
        c("#bac2de"), // 7  white        (Subtext1)
        c("#585b70"), // 8  bright black (Surface2)
        c("#f38ba8"), // 9  bright red
        c("#a6e3a1"), // 10 bright green
        c("#f9e2af"), // 11 bright yellow
        c("#89b4fa"), // 12 bright blue
        c("#cba6f7"), // 13 bright magenta
        c("#94e2d5"), // 14 bright cyan
        c("#a6adc8"), // 15 bright white  (Subtext0)
    ];
    let palette: Vec<&RGBA> = owned.iter().collect();

    terminal.set_colors(Some(&fg), Some(&bg), &palette);
    terminal.set_color_cursor(Some(&c("#f5e0dc")));
    terminal.set_color_cursor_foreground(Some(&c("#1e1e2e")));
    terminal.set_color_highlight(Some(&c("#585b70")));
    terminal.set_color_highlight_foreground(Some(&c("#cdd6f4")));
}
