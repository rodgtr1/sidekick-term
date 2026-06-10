use gtk4::prelude::*;
use sourceview5::prelude::*;
use std::cell::{Cell, RefCell};
use std::rc::Rc;
use std::time::{Duration, Instant, SystemTime};

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
    // Already open? Focus the existing tab instead of opening a duplicate.
    let page_name = format!("editor:{path}");
    for i in 0..notebook.n_pages() {
        if let Some(page) = notebook.nth_page(Some(i)) {
            if page.widget_name() == page_name {
                notebook.set_current_page(Some(i));
                if let Some(view) = page
                    .downcast_ref::<gtk4::ScrolledWindow>()
                    .and_then(|s| s.child())
                    .and_then(|c| c.downcast::<sourceview5::View>().ok())
                {
                    view.grab_focus();
                    if let Some(line) = line {
                        scroll_view_to_line(&view, line);
                    }
                }
                return;
            }
        }
    }

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
    scroll.set_widget_name(&page_name);

    // Tab label with dirty indicator
    let label = gtk4::Label::new(Some(&filename));
    let dirty: Rc<Cell<bool>> = Rc::new(Cell::new(false));

    // Disk state for external-change detection.
    let disk_mtime: Rc<Cell<Option<SystemTime>>> = Rc::new(Cell::new(file_mtime(path)));
    let conflicted: Rc<Cell<bool>> = Rc::new(Cell::new(false));
    let last_own_save: Rc<RefCell<Option<Instant>>> = Rc::new(RefCell::new(None));

    {
        let dirty_c = Rc::clone(&dirty);
        let label_c = label.clone();
        let fname = filename.clone();
        let conflicted_c = Rc::clone(&conflicted);
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
                if !conflicted_c.get() {
                    label_c.set_text(&fname);
                }
            }
        });
    }

    // The actual write: temp-file-and-rename so a crash never truncates the
    // target, refreshing our recorded mtime afterwards.
    let do_save: Rc<dyn Fn()> = Rc::new({
        let buf = buffer.clone();
        let path_s = path.to_string();
        let on_saved = on_saved.clone();
        let disk_mtime = Rc::clone(&disk_mtime);
        let conflicted = Rc::clone(&conflicted);
        let last_own_save = Rc::clone(&last_own_save);
        let label_c = label.clone();
        let fname = filename.clone();
        move || {
            let (start, end) = buf.bounds();
            let text = buf.text(&start, &end, false);
            *last_own_save.borrow_mut() = Some(Instant::now());
            match atomic_write(&path_s, text.as_bytes()) {
                Ok(()) => {
                    disk_mtime.set(file_mtime(&path_s));
                    conflicted.set(false);
                    buf.set_modified(false);
                    label_c.set_text(&fname);
                    if let Some(on_saved) = &on_saved {
                        on_saved(&path_s);
                    }
                }
                Err(e) => eprintln!("sidekick: save failed: {e}"),
            }
        }
    });

    // Ctrl+S → save, with a confirmation when the file changed on disk since
    // we loaded or last saved it (e.g. an agent edited it).
    let key_ctrl = gtk4::EventControllerKey::new();
    key_ctrl.set_propagation_phase(gtk4::PropagationPhase::Capture);
    {
        let path_s = path.to_string();
        let disk_mtime = Rc::clone(&disk_mtime);
        let conflicted = Rc::clone(&conflicted);
        let do_save = Rc::clone(&do_save);
        let view_c = view.clone();
        key_ctrl.connect_key_pressed(move |_, key, _, mods| {
            let ctrl = mods.contains(gtk4::gdk::ModifierType::CONTROL_MASK);
            if ctrl && (key == gtk4::gdk::Key::s || key == gtk4::gdk::Key::S) {
                let changed_on_disk = conflicted.get() || file_mtime(&path_s) != disk_mtime.get();
                if changed_on_disk {
                    let window = view_c
                        .root()
                        .and_then(|r| r.downcast::<gtk4::Window>().ok());
                    let do_save = Rc::clone(&do_save);
                    gtk4::AlertDialog::builder()
                        .message("File changed on disk")
                        .detail(format!(
                            "{path_s}\n\nThe file was modified outside this editor. Overwrite those changes?"
                        ))
                        .buttons(["Cancel", "Overwrite"])
                        .cancel_button(0)
                        .default_button(0)
                        .build()
                        .choose(window.as_ref(), None::<&gio::Cancellable>, move |choice| {
                            if choice == Ok(1) {
                                do_save();
                            }
                        });
                } else {
                    do_save();
                }
                return glib::Propagation::Stop;
            }
            glib::Propagation::Proceed
        });
    }
    view.add_controller(key_ctrl);

    // Watch the file: auto-reload while the buffer is clean, flag a conflict
    // when it is dirty. Our own saves are ignored via a short grace window.
    let monitor = gio::File::for_path(path)
        .monitor_file(gio::FileMonitorFlags::NONE, None::<&gio::Cancellable>)
        .ok();
    if let Some(monitor) = &monitor {
        let buf = buffer.clone();
        let view_c = view.clone();
        let path_s = path.to_string();
        let disk_mtime_c = Rc::clone(&disk_mtime);
        let conflicted_c = Rc::clone(&conflicted);
        let last_own_save_c = Rc::clone(&last_own_save);
        let label_c = label.clone();
        let fname = filename.clone();
        monitor.connect_changed(move |_, _, _, event| {
            if !matches!(
                event,
                gio::FileMonitorEvent::Changed
                    | gio::FileMonitorEvent::ChangesDoneHint
                    | gio::FileMonitorEvent::Created
                    | gio::FileMonitorEvent::Renamed
            ) {
                return;
            }
            // Ignore events triggered by our own atomic save.
            if last_own_save_c
                .borrow()
                .map(|t| t.elapsed() < Duration::from_secs(1))
                .unwrap_or(false)
            {
                return;
            }
            let new_mtime = file_mtime(&path_s);
            if new_mtime == disk_mtime_c.get() {
                return;
            }
            if buf.is_modified() {
                // Unsaved local edits + external change: flag it, Ctrl+S asks.
                conflicted_c.set(true);
                label_c.set_markup(&format!(
                    "<span foreground=\"#f9e2af\">⚠</span> {}",
                    glib::markup_escape_text(&fname),
                ));
                return;
            }
            // Clean buffer: follow the file on disk (agents edit freely).
            if let Ok(new_content) =
                crate::limits::read_text_file_limited(&path_s, crate::limits::MAX_EDITOR_FILE_BYTES)
            {
                let line = buf.iter_at_mark(&buf.get_insert()).line();
                buf.set_text(&new_content);
                buf.set_modified(false);
                disk_mtime_c.set(new_mtime);
                let target = line.min(buf.line_count().saturating_sub(1));
                if let Some(mut iter) = buf.iter_at_line(target) {
                    buf.place_cursor(&iter);
                    view_c.scroll_to_iter(&mut iter, 0.0, false, 0.0, 0.0);
                }
            }
        });
    }
    // Stop watching when the editor tab is closed.
    if let Some(monitor) = monitor {
        let scroll_w = scroll.clone();
        notebook.connect_page_removed(move |_, widget, _| {
            if widget == scroll_w.upcast_ref::<gtk4::Widget>() {
                monitor.cancel();
            }
        });
    }

    let page_idx = notebook.n_pages();
    notebook.append_page(&scroll, Some(&label));
    notebook.set_tab_reorderable(&scroll, true);
    notebook.set_current_page(Some(page_idx));
    view.grab_focus();

    if let Some(line) = line {
        scroll_view_to_line(&view, line);
    }
}

/// Scroll a sourceview to a 1-based line after layout has settled.
fn scroll_view_to_line(view: &sourceview5::View, line: u32) {
    let Some(buffer) = view.buffer().downcast::<sourceview5::Buffer>().ok() else {
        return;
    };
    let v = view.clone();
    glib::idle_add_local_once(move || {
        let mut iter = buffer.iter_at_line(line.saturating_sub(1) as i32);
        if let Some(iter) = iter.as_mut() {
            buffer.place_cursor(iter);
            v.scroll_to_iter(iter, 0.0, true, 0.0, 0.3);
        }
    });
}

fn file_mtime(path: &str) -> Option<SystemTime> {
    std::fs::metadata(path).and_then(|m| m.modified()).ok()
}

/// Write via a temp file in the same directory plus rename, preserving the
/// original permissions, so readers never observe a half-written file.
fn atomic_write(path: &str, bytes: &[u8]) -> Result<(), String> {
    let target = std::path::Path::new(path);
    let dir = target
        .parent()
        .ok_or_else(|| "File has no parent directory.".to_string())?;
    let name = target
        .file_name()
        .map(|n| n.to_string_lossy().to_string())
        .unwrap_or_else(|| "file".to_string());
    let tmp = dir.join(format!(".{name}.sidekick-tmp"));

    std::fs::write(&tmp, bytes).map_err(|e| e.to_string())?;
    if let Ok(meta) = std::fs::metadata(target) {
        let _ = std::fs::set_permissions(&tmp, meta.permissions());
    }
    std::fs::rename(&tmp, target).map_err(|e| {
        let _ = std::fs::remove_file(&tmp);
        e.to_string()
    })
}
