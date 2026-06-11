use gtk4::gdk;
use gtk4::prelude::*;
use std::cell::{Cell, RefCell};
use std::rc::Rc;

const MAX_RESULTS: usize = 50;

pub fn show(
    root: &str,
    parent: &gtk4::ApplicationWindow,
    notebook: &gtk4::Notebook,
    cfg: &Rc<RefCell<crate::config::Config>>,
    on_saved: Rc<dyn Fn(&str)>,
) {
    let win = gtk4::Window::new();
    win.set_transient_for(Some(parent));
    win.set_modal(true);
    win.set_decorated(false);
    win.set_resizable(false);
    win.set_default_size(540, 340);
    win.add_css_class("quickopen-window");

    let vbox = gtk4::Box::new(gtk4::Orientation::Vertical, 0);

    let entry = gtk4::Entry::new();
    entry.set_placeholder_text(Some("Search file names…"));
    entry.add_css_class("quickopen-entry");
    entry.set_margin_top(8);
    entry.set_margin_bottom(4);
    entry.set_margin_start(8);
    entry.set_margin_end(8);

    let list = gtk4::ListBox::new();
    list.set_selection_mode(gtk4::SelectionMode::Browse);
    list.add_css_class("quickopen-list");

    let scroll = gtk4::ScrolledWindow::new();
    scroll.set_child(Some(&list));
    scroll.set_hscrollbar_policy(gtk4::PolicyType::Never);
    scroll.set_vscrollbar_policy(gtk4::PolicyType::Automatic);
    scroll.set_vexpand(true);
    scroll.set_margin_bottom(8);
    scroll.set_margin_start(4);
    scroll.set_margin_end(4);

    vbox.append(&entry);
    vbox.append(&scroll);
    win.set_child(Some(&vbox));

    let (search_tx, search_rx) = async_channel::unbounded::<(u64, Vec<(String, String)>)>();
    let gen: Rc<Cell<u64>> = Rc::new(Cell::new(0));
    let debounce: Rc<Cell<Option<glib::SourceId>>> = Rc::new(Cell::new(None));

    // Search on every keystroke with 150ms debounce to avoid a thread-per-keypress
    {
        let root_s = root.to_string();
        let gen_c = Rc::clone(&gen);
        let tx = search_tx.clone();
        let debounce_c = Rc::clone(&debounce);
        entry.connect_changed(move |e| {
            let query = e.text().to_string();
            let root = root_s.clone();
            let g = gen_c.get() + 1;
            gen_c.set(g);
            let tx = tx.clone();

            if let Some(id) = debounce_c.take() {
                id.remove();
            }

            let debounce_inner = Rc::clone(&debounce_c);
            let id =
                glib::timeout_add_local_once(std::time::Duration::from_millis(150), move || {
                    debounce_inner.set(None);
                    std::thread::spawn(move || {
                        let results = search_files(&root, &query);
                        let _ = tx.send_blocking((g, results));
                    });
                });
            debounce_c.set(Some(id));
        });
    }

    // Receive results on the main thread
    {
        let list_c = list.clone();
        let gen_c = Rc::clone(&gen);
        glib::spawn_future_local(async move {
            while let Ok((g, results)) = search_rx.recv().await {
                if g == gen_c.get() {
                    populate(&list_c, &results);
                }
            }
        });
    }

    // Open file on row activation
    {
        let nb_c = notebook.clone();
        let cfg_c = Rc::clone(cfg);
        let on_saved_c = Rc::clone(&on_saved);
        let win_c = win.clone();
        list.connect_row_activated(move |_, row| {
            let path = row.widget_name().to_string();
            if !path.is_empty() {
                crate::editor::open_with_save_callback(
                    &path,
                    &nb_c,
                    &cfg_c.borrow(),
                    Some(Rc::clone(&on_saved_c)),
                );
                win_c.close();
            }
        });
    }

    // Keyboard: Escape closes, Down arrow moves focus to list
    {
        let win_c = win.clone();
        let list_c = list.clone();
        let key = gtk4::EventControllerKey::new();
        key.set_propagation_phase(gtk4::PropagationPhase::Capture);
        key.connect_key_pressed(move |_, keyval, _, _| match keyval {
            gdk::Key::Escape => {
                win_c.close();
                glib::Propagation::Stop
            }
            gdk::Key::Down => {
                if let Some(row) = list_c.row_at_index(0) {
                    list_c.select_row(Some(&row));
                    row.grab_focus();
                }
                glib::Propagation::Stop
            }
            gdk::Key::Return => {
                if let Some(row) = list_c.selected_row() {
                    row.activate();
                }
                glib::Propagation::Stop
            }
            _ => glib::Propagation::Proceed,
        });
        win.add_controller(key);
    }

    // Close when the window loses focus
    win.connect_is_active_notify(|w| {
        if !w.is_active() {
            w.close();
        }
    });

    win.present();
    entry.grab_focus();
}

fn populate(list: &gtk4::ListBox, results: &[(String, String)]) {
    while let Some(row) = list.row_at_index(0) {
        list.remove(&row);
    }
    for (abs_path, rel_path) in results {
        let row = gtk4::ListBoxRow::new();
        row.set_widget_name(abs_path);

        let vbox = gtk4::Box::new(gtk4::Orientation::Vertical, 1);
        vbox.set_margin_start(10);
        vbox.set_margin_end(10);
        vbox.set_margin_top(4);
        vbox.set_margin_bottom(4);

        let filename = std::path::Path::new(rel_path)
            .file_name()
            .map(|n| n.to_string_lossy().to_string())
            .unwrap_or_else(|| rel_path.clone());

        let name_label = gtk4::Label::new(Some(&filename));
        name_label.set_xalign(0.0);
        name_label.add_css_class("quickopen-name");

        let path_label = gtk4::Label::new(Some(rel_path.as_str()));
        path_label.set_xalign(0.0);
        path_label.set_ellipsize(pango::EllipsizeMode::Start);
        path_label.add_css_class("quickopen-path");

        vbox.append(&name_label);
        vbox.append(&path_label);
        row.set_child(Some(&vbox));
        list.insert(&row, -1);
    }
}

fn search_files(root: &str, query: &str) -> Vec<(String, String)> {
    if query.trim().is_empty() {
        return vec![];
    }
    let max = MAX_RESULTS.to_string();
    // Truncating cap: with `find` on a huge tree we keep the first chunk of
    // results instead of buffering the entire listing in memory.
    const MAX_OUTPUT_BYTES: usize = 1024 * 1024;

    // Try fd first
    let out = crate::limits::command_stdout_limited(
        std::process::Command::new("fd")
            .args([
                "--type",
                "f",
                "--fixed-strings",
                "--max-results",
                &max,
                "--color",
                "never",
                "--",
                query,
            ])
            .current_dir(root),
        MAX_OUTPUT_BYTES,
        &[1],
        crate::limits::CapMode::Truncate,
    );

    let raw = match out {
        Ok(o) => o,
        Err(_) => match crate::limits::command_stdout_limited(
            std::process::Command::new("find")
                .args([
                    ".",
                    "-name",
                    ".git",
                    "-prune",
                    "-o",
                    "-type",
                    "f",
                    "-iname",
                    &format!("*{}*", query),
                    "-print",
                ])
                .current_dir(root),
            MAX_OUTPUT_BYTES,
            &[1],
            crate::limits::CapMode::Truncate,
        ) {
            Ok(o) => o,
            Err(_) => return vec![],
        },
    };

    String::from_utf8_lossy(&raw)
        .lines()
        .take(MAX_RESULTS)
        .map(|line| {
            let rel = line.trim_start_matches("./").to_string();
            let abs = format!("{}/{}", root, rel);
            (abs, rel)
        })
        .collect()
}
