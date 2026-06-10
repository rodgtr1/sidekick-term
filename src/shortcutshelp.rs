use gtk4::gdk;
use gtk4::prelude::*;

const SECTIONS: &[(&str, &[(&str, &str)])] = &[
    (
        "Tabs",
        &[
            ("Ctrl+Shift+T", "New terminal tab"),
            ("Ctrl+Shift+W", "Close pane / editor tab / diff tab"),
            ("Ctrl+Tab", "Next tab"),
            ("Ctrl+Shift+Tab", "Previous tab"),
        ],
    ),
    (
        "Splits",
        &[
            ("Ctrl+Shift+D", "Split terminal right"),
            ("Ctrl+Shift+X", "Split terminal down"),
            ("Alt+Left", "Focus previous terminal pane"),
            ("Alt+Right", "Focus next terminal pane"),
        ],
    ),
    (
        "Terminal",
        &[
            ("Ctrl+Shift+C / Ctrl+Insert", "Copy selected text"),
            ("Ctrl+Shift+V / Shift+Insert", "Paste text"),
            ("Ctrl+V", "Paste clipboard image as temp PNG path"),
            ("Ctrl+Shift+H", "Find in scrollback"),
            ("Ctrl+= / Ctrl+- / Ctrl+0", "Font zoom in / out / reset"),
        ],
    ),
    (
        "Panels",
        &[
            ("Ctrl+Shift+E", "File explorer panel"),
            ("Ctrl+Shift+G", "Git panel"),
            ("Ctrl+Shift+F", "Search-in-files panel"),
            ("Ctrl+Shift+R", "Run panel"),
            ("Ctrl+Shift+A", "Agents dashboard panel"),
            ("Ctrl+Shift+B", "Toggle sidebar"),
            ("Ctrl+Shift+O", "Toggle embedded browser"),
        ],
    ),
    (
        "Files",
        &[
            ("Ctrl+F", "Quick open: search file names"),
            ("Ctrl+S", "Save the current editor tab"),
        ],
    ),
    (
        "App",
        &[
            ("Ctrl+Shift+P", "Command palette"),
            ("Ctrl+Shift+?", "Keyboard shortcuts help"),
            ("Ctrl+,", "Open sidekick config in nvim"),
        ],
    ),
];

/// Show a modal window listing every keyboard shortcut, grouped by section.
pub fn show(parent: &gtk4::ApplicationWindow) {
    let win = gtk4::Window::new();
    win.set_transient_for(Some(parent));
    win.set_modal(true);
    win.set_decorated(false);
    win.set_resizable(false);
    win.set_default_size(520, 560);
    win.add_css_class("quickopen-window");

    let vbox = gtk4::Box::new(gtk4::Orientation::Vertical, 0);
    vbox.set_margin_top(12);
    vbox.set_margin_bottom(12);
    vbox.set_margin_start(16);
    vbox.set_margin_end(16);

    let title = gtk4::Label::new(Some("Keyboard Shortcuts"));
    title.add_css_class("shortcuts-title");
    title.set_halign(gtk4::Align::Start);
    title.set_margin_bottom(8);
    vbox.append(&title);

    let grid = gtk4::Grid::new();
    grid.set_row_spacing(2);
    grid.set_column_spacing(24);

    let mut row = 0;
    for (section, entries) in SECTIONS {
        let header = gtk4::Label::new(Some(section));
        header.add_css_class("shortcuts-section");
        header.set_halign(gtk4::Align::Start);
        header.set_margin_top(if row == 0 { 0 } else { 10 });
        grid.attach(&header, 0, row, 2, 1);
        row += 1;

        for (keys, action) in *entries {
            let key_label = gtk4::Label::new(Some(keys));
            key_label.add_css_class("shortcuts-keys");
            key_label.set_halign(gtk4::Align::Start);
            let action_label = gtk4::Label::new(Some(action));
            action_label.add_css_class("shortcuts-action");
            action_label.set_halign(gtk4::Align::Start);
            grid.attach(&key_label, 0, row, 1, 1);
            grid.attach(&action_label, 1, row, 1, 1);
            row += 1;
        }
    }

    let scroll = gtk4::ScrolledWindow::new();
    scroll.set_child(Some(&grid));
    scroll.set_hscrollbar_policy(gtk4::PolicyType::Never);
    scroll.set_vscrollbar_policy(gtk4::PolicyType::Automatic);
    scroll.set_vexpand(true);
    vbox.append(&scroll);

    win.set_child(Some(&vbox));

    // Escape or any panel shortcut closes the window.
    {
        let win_c = win.clone();
        let key = gtk4::EventControllerKey::new();
        key.connect_key_pressed(move |_, keyval, _, mods| {
            let ctrl = mods.contains(gdk::ModifierType::CONTROL_MASK);
            let shift = mods.contains(gdk::ModifierType::SHIFT_MASK);
            if keyval == gdk::Key::Escape
                || (ctrl && shift && matches!(keyval, gdk::Key::question | gdk::Key::slash))
            {
                win_c.close();
                return glib::Propagation::Stop;
            }
            glib::Propagation::Proceed
        });
        win.add_controller(key);
    }

    win.present();
}
