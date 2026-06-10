use gtk4::prelude::*;
use sourceview5::prelude::*;
use std::cell::Cell;
use std::rc::Rc;

pub type SaveCallback = Rc<dyn Fn(&str)>;

pub fn open_with_save_callback(
    path: &str,
    notebook: &gtk4::Notebook,
    cfg: &crate::config::Config,
    on_saved: Option<SaveCallback>,
) {
    open_at_line(path, None, notebook, cfg, on_saved);
}

pub fn open_at_line(
    path: &str,
    line: Option<u32>,
    notebook: &gtk4::Notebook,
    cfg: &crate::config::Config,
    on_saved: Option<SaveCallback>,
) {
    let filename = crate::limits::display_name(path);

    let content =
        match crate::limits::read_text_file_limited(path, crate::limits::MAX_EDITOR_FILE_BYTES) {
            Ok(c) => c,
            Err(e) => {
                crate::diff::open_message(&filename, path, &e, notebook);
                return;
            }
        };

    // Language detection from file extension / content
    let lm = sourceview5::LanguageManager::default();
    let language = lm.guess_language(Some(path), None::<&str>);

    let buffer = sourceview5::Buffer::new(None::<&gtk4::TextTagTable>);
    buffer.set_language(language.as_ref());
    buffer.set_highlight_syntax(true);
    buffer.set_highlight_matching_brackets(true);

    // Apply a dark style scheme
    let sm = sourceview5::StyleSchemeManager::default();
    if let Some(scheme) = sm
        .scheme("oblivion")
        .or_else(|| sm.scheme("solarized-dark"))
    {
        buffer.set_style_scheme(Some(&scheme));
    }

    buffer.set_text(&content);
    // Clear undo history so the initial load isn't undoable
    buffer.set_modified(false);

    let view = sourceview5::View::with_buffer(&buffer);
    view.set_show_line_numbers(true);
    view.set_highlight_current_line(true);
    view.set_tab_width(4);
    view.set_indent_width(4);
    view.set_insert_spaces_instead_of_tabs(true);
    view.set_auto_indent(true);
    view.set_monospace(true);
    view.set_wrap_mode(if cfg.editor.word_wrap {
        gtk4::WrapMode::Word
    } else {
        gtk4::WrapMode::None
    });
    view.set_left_margin(12);
    view.set_right_margin(12);
    view.set_top_margin(8);
    view.set_bottom_margin(8);

    // Font is applied via CSS (.editor-view rule in build_css)

    view.add_css_class("editor-view");

    let scroll = gtk4::ScrolledWindow::new();
    scroll.set_child(Some(&view));
    scroll.set_vexpand(true);
    scroll.set_hexpand(true);

    // Tab label with dirty indicator
    let label = gtk4::Label::new(Some(&filename));
    let dirty: Rc<Cell<bool>> = Rc::new(Cell::new(false));

    {
        let dirty_c = Rc::clone(&dirty);
        let label_c = label.clone();
        let fname = filename.clone();
        buffer.connect_modified_changed(move |buf| {
            if buf.is_modified() {
                if !dirty_c.get() {
                    dirty_c.set(true);
                    label_c.set_markup(&format!(
                        "<span foreground=\"#f38ba8\">●</span> {}",
                        glib::markup_escape_text(&fname),
                    ));
                }
            } else {
                dirty_c.set(false);
                label_c.set_text(&fname);
            }
        });
    }

    // Ctrl+S → save
    let key_ctrl = gtk4::EventControllerKey::new();
    key_ctrl.set_propagation_phase(gtk4::PropagationPhase::Capture);
    {
        let buf = buffer.clone();
        let path_s = path.to_string();
        let on_saved = on_saved.clone();
        key_ctrl.connect_key_pressed(move |_, key, _, mods| {
            let ctrl = mods.contains(gtk4::gdk::ModifierType::CONTROL_MASK);
            if ctrl && (key == gtk4::gdk::Key::s || key == gtk4::gdk::Key::S) {
                let (start, end) = buf.bounds();
                let text = buf.text(&start, &end, false);
                if let Err(e) = std::fs::write(&path_s, text.as_bytes()) {
                    eprintln!("sidekick: save failed: {e}");
                } else {
                    buf.set_modified(false);
                    if let Some(on_saved) = &on_saved {
                        on_saved(&path_s);
                    }
                }
                return glib::Propagation::Stop;
            }
            glib::Propagation::Proceed
        });
    }
    view.add_controller(key_ctrl);

    let page_idx = notebook.n_pages();
    notebook.append_page(&scroll, Some(&label));
    notebook.set_current_page(Some(page_idx));
    view.grab_focus();

    if let Some(line) = line {
        // Scroll after the view has been laid out; scrolling immediately is a
        // no-op because line heights are not validated yet.
        let buf = buffer.clone();
        let v = view.clone();
        glib::idle_add_local_once(move || {
            let mut iter = buf.iter_at_line(line.saturating_sub(1) as i32);
            if let Some(iter) = iter.as_mut() {
                buf.place_cursor(iter);
                v.scroll_to_iter(iter, 0.0, true, 0.0, 0.3);
            }
        });
    }
}
