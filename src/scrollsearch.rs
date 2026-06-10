use gtk4::gdk;
use gtk4::prelude::*;
use vte4::prelude::*;

// PCRE2 option bits VTE search regexes understand.
const PCRE2_CASELESS: u32 = 0x0000_0008;
const PCRE2_MULTILINE: u32 = 0x0000_0400;

/// Floating find-in-scrollback bar for `terminal`. Searches are literal and
/// case-insensitive. Enter finds the next match upward (into scrollback),
/// Shift+Enter searches downward, Escape closes and clears highlights.
pub fn show(parent: &gtk4::ApplicationWindow, terminal: &vte4::Terminal) {
    let win = gtk4::Window::new();
    win.set_transient_for(Some(parent));
    win.set_decorated(false);
    win.set_resizable(false);
    win.set_default_width(420);
    win.add_css_class("quickopen-window");

    let entry = gtk4::Entry::new();
    entry.set_placeholder_text(Some("Find in scrollback…"));
    entry.add_css_class("quickopen-entry");
    entry.set_hexpand(true);

    let up_btn = gtk4::Button::with_label("▲");
    let down_btn = gtk4::Button::with_label("▼");
    for b in [&up_btn, &down_btn] {
        b.add_css_class("run-btn");
        b.set_tooltip_text(Some("Enter: search up · Shift+Enter: search down"));
    }

    let hbox = gtk4::Box::new(gtk4::Orientation::Horizontal, 4);
    hbox.set_margin_top(6);
    hbox.set_margin_bottom(6);
    hbox.set_margin_start(8);
    hbox.set_margin_end(8);
    hbox.append(&entry);
    hbox.append(&up_btn);
    hbox.append(&down_btn);
    win.set_child(Some(&hbox));

    terminal.search_set_wrap_around(true);

    // Update the search regex on every keystroke and jump to the nearest
    // match above the current view.
    {
        let term = terminal.clone();
        entry.connect_changed(move |e| {
            let text = e.text().to_string();
            if text.is_empty() {
                term.search_set_regex(None, 0);
                return;
            }
            let pattern = regex_escape(&text);
            match vte4::Regex::for_search(&pattern, PCRE2_CASELESS | PCRE2_MULTILINE) {
                Ok(re) => {
                    term.search_set_regex(Some(&re), 0);
                    term.search_find_previous();
                }
                Err(_) => term.search_set_regex(None, 0),
            }
        });
    }

    {
        let term = terminal.clone();
        up_btn.connect_clicked(move |_| {
            term.search_find_previous();
        });
    }
    {
        let term = terminal.clone();
        down_btn.connect_clicked(move |_| {
            term.search_find_next();
        });
    }

    {
        let term = terminal.clone();
        let win_c = win.clone();
        let key = gtk4::EventControllerKey::new();
        key.set_propagation_phase(gtk4::PropagationPhase::Capture);
        key.connect_key_pressed(move |_, keyval, _, mods| {
            let shift = mods.contains(gdk::ModifierType::SHIFT_MASK);
            match keyval {
                gdk::Key::Escape => {
                    win_c.close();
                    glib::Propagation::Stop
                }
                gdk::Key::Return | gdk::Key::KP_Enter => {
                    if shift {
                        term.search_find_next();
                    } else {
                        term.search_find_previous();
                    }
                    glib::Propagation::Stop
                }
                _ => glib::Propagation::Proceed,
            }
        });
        win.add_controller(key);
    }

    // Closing clears highlights and hands focus back to the terminal.
    {
        let term = terminal.clone();
        win.connect_destroy(move |_| {
            term.search_set_regex(None, 0);
            term.grab_focus();
        });
    }
    win.connect_is_active_notify(|w| {
        if !w.is_active() {
            w.close();
        }
    });

    win.present();
    entry.grab_focus();
}

/// Escape PCRE2 metacharacters so the query is matched literally.
fn regex_escape(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    for c in s.chars() {
        if "\\^$.[]|()?*+{}-".contains(c) {
            out.push('\\');
        }
        out.push(c);
    }
    out
}
