mod browser;
mod config;
mod diff;
mod editor;
mod filetree;
mod git;
mod gitpanel;
mod ipc;
mod limits;
mod pane;
mod quickopen;
mod runpanel;
mod searchpanel;
mod tab;
mod theme;

use gtk4::prelude::*;
use gtk4::{gdk, Application, ApplicationWindow, Notebook};
use std::cell::{Cell, RefCell};
use std::rc::Rc;
use std::sync::mpsc;
use vte4::prelude::*;
use webkit6::prelude::WebViewExt as _;

const APP_ID: &str = "com.travismedia.sidekick";

enum UiResult {
    Tree {
        shell_cwd: String,
        tree_root: String,
        entries: Vec<filetree::TreeEntry>,
        tasks: Vec<runpanel::Task>,
    },
    Subtree {
        path: String,
        entries: Vec<filetree::TreeEntry>,
    },
    Git {
        cwd: String,
        root: String,
        files: Vec<git::GitFile>,
        ahead: u32,
    },
    Diff {
        title: String,
        result: Result<String, String>,
    },
    Push {
        result: Result<(), String>,
    },
    SearchDone {
        gen: u64,
        results: Vec<searchpanel::FileMatches>,
    },
}

fn main() -> glib::ExitCode {
    let app = Application::builder().application_id(APP_ID).build();
    app.connect_activate(build_ui);
    app.run()
}

fn build_ui(app: &Application) {
    let cfg = Rc::new(config::Config::load());

    let ipc_rx = ipc::start();

    let notebook = Notebook::new();
    notebook.set_scrollable(true);
    notebook.set_show_border(false);

    // File tree
    let (tree_header, tree_view, tree_store, tree_scroll) = filetree::build();
    let tree_store = Rc::new(tree_store);

    // Git panel
    let (git_panel, git_list_header, git_list, push_btn) = gitpanel::build();
    let git_files: Rc<RefCell<Vec<git::GitFile>>> = Rc::new(RefCell::new(Vec::new()));

    // Search panel
    let (search_panel, search_entry, search_list) = searchpanel::build();
    let search_gen: Rc<Cell<u64>> = Rc::new(Cell::new(0));

    // Run panel
    let (run_panel, run_list) = runpanel::build();

    // File tree page (header + scroll stacked vertically)
    let files_page = gtk4::Box::new(gtk4::Orientation::Vertical, 0);
    files_page.append(&tree_header);
    files_page.append(&tree_scroll);

    // Panel stack switches between panels
    let panel_stack = gtk4::Stack::new();
    panel_stack.set_transition_type(gtk4::StackTransitionType::None);
    panel_stack.set_hexpand(false);
    panel_stack.add_named(&files_page, Some("files"));
    panel_stack.add_named(&git_panel, Some("git"));
    panel_stack.add_named(&search_panel, Some("search"));
    panel_stack.add_named(&run_panel, Some("run"));

    // Activity bar: narrow icon strip on the far left
    let activity_bar = gtk4::Box::new(gtk4::Orientation::Vertical, 0);
    activity_bar.add_css_class("activity-bar");
    activity_bar.set_width_request(40);

    let btn_files = gtk4::Button::new();
    btn_files.add_css_class("activity-btn");
    btn_files.add_css_class("active");
    let img_files = gtk4::Image::from_icon_name("folder-symbolic");
    img_files.set_pixel_size(20);
    btn_files.set_child(Some(&img_files));
    btn_files.set_tooltip_text(Some("Explorer (Ctrl+Shift+E)"));

    let btn_git = gtk4::Button::new();
    btn_git.add_css_class("activity-btn");
    let img_git = gtk4::Image::from_icon_name("emblem-synchronizing-symbolic");
    img_git.set_pixel_size(20);
    btn_git.set_child(Some(&img_git));
    btn_git.set_tooltip_text(Some("Source Control (Ctrl+Shift+G)"));

    let btn_search = gtk4::Button::new();
    btn_search.add_css_class("activity-btn");
    let img_search = gtk4::Image::from_icon_name("edit-find-symbolic");
    img_search.set_pixel_size(20);
    btn_search.set_child(Some(&img_search));
    btn_search.set_tooltip_text(Some("Search in files (Ctrl+Shift+F)"));

    let btn_run = gtk4::Button::new();
    btn_run.add_css_class("activity-btn");
    let img_run = gtk4::Image::from_icon_name("media-playback-start-symbolic");
    img_run.set_pixel_size(20);
    btn_run.set_child(Some(&img_run));
    btn_run.set_tooltip_text(Some("Run Tasks (Ctrl+Shift+R)"));

    activity_bar.append(&btn_files);
    activity_bar.append(&btn_git);
    activity_bar.append(&btn_search);
    activity_bar.append(&btn_run);

    // Panel stack is the togglable sidebar content (no activity bar inside)
    let tree_sidebar = panel_stack.clone();
    tree_sidebar.set_width_request(220);
    tree_sidebar.add_css_class("sidebar");

    // Browser panel (hidden by default)
    let browser_panel = browser::build();
    browser_panel.widget.set_visible(false);
    browser_panel.widget.set_hexpand(true);

    // Content paned: notebook | browser
    let content_paned = gtk4::Paned::new(gtk4::Orientation::Horizontal);
    content_paned.set_start_child(Some(&notebook));
    content_paned.set_end_child(Some(&browser_panel.widget));
    content_paned.set_shrink_start_child(false);
    content_paned.set_shrink_end_child(false);
    content_paned.set_resize_start_child(true);
    content_paned.set_resize_end_child(true);

    content_paned.set_hexpand(true);

    // Root layout: activity bar (always visible) | sidebar | content
    let root_box = gtk4::Box::new(gtk4::Orientation::Horizontal, 0);
    root_box.append(&activity_bar);
    root_box.append(&tree_sidebar);
    root_box.append(&content_paned);

    // Sidebar visible by default
    let sidebar_visible: Rc<Cell<bool>> = Rc::new(Cell::new(true));
    let browser_visible: Rc<Cell<bool>> = Rc::new(Cell::new(false));

    add_tab(&notebook, &cfg, None);

    let css = gtk4::CssProvider::new();
    css.load_from_string(&build_css(&cfg));
    gtk4::style_context_add_provider_for_display(
        &gdk::Display::default().unwrap(),
        &css,
        gtk4::STYLE_PROVIDER_PRIORITY_APPLICATION,
    );

    let window = ApplicationWindow::builder()
        .application(app)
        .title("sidekick")
        .default_width(1400)
        .default_height(800)
        .child(&root_box)
        .build();

    // Track last known cwd to avoid redundant tree reloads
    let last_cwd: Rc<RefCell<String>> = Rc::new(RefCell::new(String::new()));
    let (ui_tx, ui_rx) = mpsc::channel::<UiResult>();
    let ui_rx = Rc::new(RefCell::new(ui_rx));
    let tree_busy = Rc::new(Cell::new(false));
    let git_busy = Rc::new(Cell::new(false));

    // Callback that triggers a background git status refresh (used by the context menu).
    let refresh_git: Rc<dyn Fn()> = {
        let last_cwd = Rc::clone(&last_cwd);
        let tx = ui_tx.clone();
        Rc::new(move || {
            let cwd = last_cwd.borrow().clone();
            if cwd.is_empty() { return; }
            let tx = tx.clone();
            std::thread::spawn(move || {
                let root = git::repo_root(&cwd).unwrap_or_else(|| cwd.clone());
                let files = git::changed_files(&cwd);
                let ahead = git::ahead_count(&cwd);
                let _ = tx.send(UiResult::Git { cwd, root, files, ahead });
            });
        })
    };

    // Apply filesystem/git work that completed off the GTK thread.
    {
        let rx = Rc::clone(&ui_rx);
        let store = Rc::clone(&tree_store);
        let tv = tree_view.clone();
        let last = Rc::clone(&last_cwd);
        let header_c = tree_header.clone();
        let git_list_c = git_list.clone();
        let git_hdr_c = git_list_header.clone();
        let git_files_c = Rc::clone(&git_files);
        let tree_busy_c = Rc::clone(&tree_busy);
        let git_busy_c = Rc::clone(&git_busy);
        let nb_c = notebook.clone();
        let push_btn_c = push_btn.clone();
        let win_c = window.clone();
        let search_list_c = search_list.clone();
        let search_gen_c = Rc::clone(&search_gen);
        let run_list_c = run_list.clone();
        let nb_run_c = notebook.clone();
        let win_run_c = window.clone();
        let cfg_c = Rc::clone(&cfg);
        let refresh_git_c = Rc::clone(&refresh_git);
        glib::timeout_add_local(std::time::Duration::from_millis(200), move || {
            while let Ok(result) = rx.borrow_mut().try_recv() {
                match result {
                    UiResult::Tree { shell_cwd, tree_root, entries, tasks } => {
                        tree_busy_c.set(false);
                        if *last.borrow() == shell_cwd {
                            let name = std::path::Path::new(&tree_root)
                                .file_name()
                                .map(|n| n.to_string_lossy().to_string())
                                .unwrap_or_else(|| tree_root.clone());
                            header_c.set_text(&name);
                            filetree::apply_root(&store, &tv, &entries);
                            let win_r = win_run_c.clone();
                            let nb_r = nb_run_c.clone();
                            runpanel::populate(&run_list_c, &cfg_c.tasks, &tasks, move |cmd, run| {
                                if let Some(term) = focused_terminal(&win_r, &nb_r) {
                                    let mut bytes = cmd.as_bytes().to_vec();
                                    if run { bytes.push(b'\n'); }
                                    term.feed_child(&bytes);
                                    term.grab_focus();
                                }
                            });
                        }
                    }
                    UiResult::Subtree { path, entries } => {
                        if let Some(iter) = filetree::find_iter_by_file_path(&store, &path) {
                            filetree::apply_subtree(&store, &iter, &entries);
                            // clear_children briefly leaves 0 children, collapsing the row.
                            // Re-expand it now that real entries are in place.
                            #[allow(deprecated)]
                            tv.expand_row(&store.path(&iter), false);
                        }
                    }
                    UiResult::Git { cwd, root, files, ahead } => {
                        git_busy_c.set(false);
                        if *last.borrow() == cwd {
                            let count = files.len();
                            git_hdr_c.set_text(
                                if count > 0 {
                                    format!("GIT CHANGES ({})", count)
                                } else {
                                    "GIT CHANGES".to_string()
                                }
                                .as_str(),
                            );
                            gitpanel::populate(&git_list_c, &files, &root, &refresh_git_c);
                            *git_files_c.borrow_mut() = files;
                            gitpanel::update_push_button(&push_btn_c, ahead);
                        }
                    }
                    UiResult::Diff { title, result } => match result {
                        Ok(diff_text) => diff::open_readonly(&title, &diff_text, &nb_c),
                        Err(message) => {
                            diff::open_message("diff unavailable", &title, &message, &nb_c)
                        }
                    },
                    UiResult::Push { result } => match result {
                        Ok(()) => {
                            push_btn_c.set_visible(false);
                        }
                        Err(msg) => {
                            push_btn_c.set_sensitive(true);
                            gtk4::AlertDialog::builder()
                                .message("Push failed")
                                .detail(&msg)
                                .build()
                                .show(Some(&win_c));
                        }
                    },
                    UiResult::SearchDone { gen, results } => {
                        if gen == search_gen_c.get() {
                            searchpanel::populate(&search_list_c, &results);
                        }
                    }
                }
            }
            glib::ControlFlow::Continue
        });
    }

    // Poll active terminal's cwd; refresh file tree when it changes.
    {
        let nb = notebook.clone();
        let last = Rc::clone(&last_cwd);
        let win = window.clone();
        let tx = ui_tx.clone();
        let tree_busy_c = Rc::clone(&tree_busy);
        glib::timeout_add_seconds_local(1, move || {
            if let Some(cwd) = focused_terminal_cwd(&win, &nb) {
                let mut prev = last.borrow_mut();
                if *prev != cwd {
                    *prev = cwd.clone();
                    tree_busy_c.set(true);
                    let tx = tx.clone();
                    let shell_cwd = cwd.clone();
                    std::thread::spawn(move || {
                        let tree_root = git::repo_root(&shell_cwd)
                            .unwrap_or_else(|| shell_cwd.clone());
                        let entries = filetree::scan_root(&tree_root);
                        let tasks = runpanel::load_tasks(&tree_root);
                        let _ = tx.send(UiResult::Tree {
                            shell_cwd,
                            tree_root,
                            entries,
                            tasks,
                        });
                    });
                }
            }
            glib::ControlFlow::Continue
        });
    }

    // Refresh git status off the GTK thread on a slower cadence.
    {
        let last = Rc::clone(&last_cwd);
        let tx = ui_tx.clone();
        let git_busy_c = Rc::clone(&git_busy);
        glib::timeout_add_seconds_local(5, move || {
            let cwd = last.borrow().clone();
            if !cwd.is_empty() && !git_busy_c.get() {
                git_busy_c.set(true);
                let tx = tx.clone();
                std::thread::spawn(move || {
                    let root = git::repo_root(&cwd).unwrap_or_else(|| cwd.clone());
                    let files = git::changed_files(&cwd);
                    let ahead = git::ahead_count(&cwd);
                    let _ = tx.send(UiResult::Git { cwd, root, files, ahead });
                });
            }
            glib::ControlFlow::Continue
        });
    }

    // Open diff tab when a changed file is clicked in the git panel
    {
        let files_ref = Rc::clone(&git_files);
        let last_cwd_c = Rc::clone(&last_cwd);
        let tx = ui_tx.clone();
        git_list.connect_row_activated(move |_, row| {
            let name = row.widget_name().to_string();
            let (staged, rel_path) = if let Some(r) = name.strip_prefix("s:") {
                (true, r.to_string())
            } else if let Some(r) = name.strip_prefix("u:") {
                (false, r.to_string())
            } else {
                (false, name.clone())
            };
            let files = files_ref.borrow();
            if let Some(file) = files
                .iter()
                .find(|f| f.rel_path == rel_path && f.staged == staged)
            {
                let cwd = last_cwd_c.borrow().clone();
                let file = file.clone();
                let title = file.rel_path.clone();
                let tx = tx.clone();
                std::thread::spawn(move || {
                    let result = git::repo_root(&cwd)
                        .ok_or_else(|| "Not inside a git repository.".to_string())
                        .and_then(|root| git::file_diff(&root, &file));
                    let _ = tx.send(UiResult::Diff { title, result });
                });
            }
        });
    }

    // Open file in editor when a search result is clicked
    {
        let nb_c = notebook.clone();
        let cfg_c = Rc::clone(&cfg);
        search_list.connect_row_activated(move |_, row| {
            let name = row.widget_name().to_string();
            // widget name is "abs_path:line"
            if let Some(path) = name.splitn(2, ':').next() {
                editor::open(path, &nb_c, &cfg_c);
            }
        });
    }

    // Shared panel-switching logic for both activity bar buttons and keyboard shortcuts.
    let all_btns = [btn_files.clone(), btn_git.clone(), btn_search.clone(), btn_run.clone()];
    let switch_panel: Rc<dyn Fn(&'static str, usize)> = Rc::new({
        let stack = panel_stack.clone();
        let sidebar = tree_sidebar.clone();
        let vis = Rc::clone(&sidebar_visible);
        let btns = all_btns.clone();
        move |page: &'static str, btn_idx: usize| {
            let already =
                stack.visible_child_name().as_deref() == Some(page) && sidebar.is_visible();
            if already {
                vis.set(false);
                sidebar.set_visible(false);
            } else {
                vis.set(true);
                sidebar.set_visible(true);
                stack.set_visible_child_name(page);
                for (i, b) in btns.iter().enumerate() {
                    if i == btn_idx {
                        b.add_css_class("active");
                    } else {
                        b.remove_css_class("active");
                    }
                }
            }
        }
    });

    let key_ctrl = gtk4::EventControllerKey::new();
    key_ctrl.set_propagation_phase(gtk4::PropagationPhase::Capture);
    {
        let nb = notebook.clone();
        let cfg = Rc::clone(&cfg);
        let win = window.clone();
        let sidebar_vis = Rc::clone(&sidebar_visible);
        let scroll = tree_sidebar.clone();
        let browser_vis = Rc::clone(&browser_visible);
        let browser_wgt = browser_panel.widget.clone();
        let browser_wv = browser_panel.webview.clone();
        let switch_panel = Rc::clone(&switch_panel);
        let search_entry_k = search_entry.clone();
        let cpaned = content_paned.clone();
        let last_cwd_qo = Rc::clone(&last_cwd);
        let cfg_qo = Rc::clone(&cfg);
        key_ctrl.connect_key_pressed(move |_, key, _, mods| {
            // Let WebView handle all key events itself — its IME/input breaks under Capture
            if gtk4::prelude::GtkWindowExt::focus(&win).map(|f| f.is::<webkit6::WebView>()).unwrap_or(false) {
                return glib::Propagation::Proceed;
            }
            let ctrl = mods.contains(gdk::ModifierType::CONTROL_MASK);
            let shift = mods.contains(gdk::ModifierType::SHIFT_MASK);
            let alt = mods.contains(gdk::ModifierType::ALT_MASK);

            match (ctrl, shift, alt, key) {
                // New tab
                (true, true, false, gdk::Key::t | gdk::Key::T) => {
                    let cwd = focused_terminal_cwd(&win, &nb);
                    add_tab(&nb, &cfg, cwd.as_deref());
                    glib::Propagation::Stop
                }
                // Close pane / tab (also closes editor tabs)
                (true, true, false, gdk::Key::w | gdk::Key::W) => {
                    if gtk4::prelude::GtkWindowExt::focus(&win)
                        .and_then(|w| w.downcast::<vte4::Terminal>().ok())
                        .is_some()
                    {
                        pane::close(&win, &nb);
                    } else {
                        // Editor tab or other — just remove current page
                        if let Some(idx) = nb.current_page() {
                            nb.remove_page(Some(idx));
                            if nb.n_pages() == 0 {
                                std::process::exit(0);
                            }
                        }
                    }
                    glib::Propagation::Stop
                }
                // Split right (side by side)
                (true, true, false, gdk::Key::d | gdk::Key::D) => {
                    pane::split(&win, &nb, &cfg, gtk4::Orientation::Horizontal);
                    glib::Propagation::Stop
                }
                // Split down (top / bottom)
                (true, true, false, gdk::Key::x | gdk::Key::X) => {
                    pane::split(&win, &nb, &cfg, gtk4::Orientation::Vertical);
                    glib::Propagation::Stop
                }
                // Panel shortcuts
                (true, true, false, gdk::Key::e | gdk::Key::E) => {
                    switch_panel("files", 0);
                    glib::Propagation::Stop
                }
                (true, true, false, gdk::Key::g | gdk::Key::G) => {
                    switch_panel("git", 1);
                    glib::Propagation::Stop
                }
                (true, true, false, gdk::Key::f | gdk::Key::F) => {
                    switch_panel("search", 2);
                    let e = search_entry_k.clone();
                    glib::idle_add_local_once(move || { e.grab_focus(); });
                    glib::Propagation::Stop
                }
                // Quick open: file name search
                (true, false, false, gdk::Key::f | gdk::Key::F) => {
                    let cwd = last_cwd_qo.borrow().clone();
                    if !cwd.is_empty() {
                        let root = git::repo_root(&cwd).unwrap_or(cwd);
                        quickopen::show(&root, &win, &nb, &cfg_qo);
                    }
                    glib::Propagation::Stop
                }
                (true, true, false, gdk::Key::r | gdk::Key::R) => {
                    switch_panel("run", 3);
                    glib::Propagation::Stop
                }
                // Navigate panes
                (false, false, true, gdk::Key::Left) => {
                    pane::navigate(&win, &nb, false);
                    glib::Propagation::Stop
                }
                (false, false, true, gdk::Key::Right) => {
                    pane::navigate(&win, &nb, true);
                    glib::Propagation::Stop
                }
                // Next / prev tab
                (true, false, false, gdk::Key::Tab) => {
                    let n = nb.n_pages();
                    if n > 1 {
                        let next = (nb.current_page().unwrap_or(0) + 1) % n;
                        nb.set_current_page(Some(next));
                    }
                    glib::Propagation::Stop
                }
                (true, true, false, gdk::Key::Tab | gdk::Key::ISO_Left_Tab) => {
                    let n = nb.n_pages();
                    if n > 1 {
                        let cur = nb.current_page().unwrap_or(0);
                        let prev = if cur == 0 { n - 1 } else { cur - 1 };
                        nb.set_current_page(Some(prev));
                    }
                    glib::Propagation::Stop
                }
                // Toggle sidebar
                (true, true, false, gdk::Key::b | gdk::Key::B) => {
                    let visible = !sidebar_vis.get();
                    sidebar_vis.set(visible);
                    scroll.set_visible(visible);
                    glib::Propagation::Stop
                }
                // Toggle browser panel
                (true, true, false, gdk::Key::o | gdk::Key::O) => {
                    let visible = !browser_vis.get();
                    browser_vis.set(visible);
                    browser_wgt.set_visible(visible);
                    if visible {
                        let total = cpaned.width();
                        cpaned.set_position(if total > 0 { total / 2 } else { 600 });
                        if browser_wv.uri().map(|u| u == "about:blank").unwrap_or(true) {
                            // leave blank — user types their URL
                        }
                        browser_wgt.grab_focus();
                    }
                    glib::Propagation::Stop
                }
                _ => glib::Propagation::Proceed,
            }
        });
    }

    window.add_controller(key_ctrl);

    // Open file in editor tab when tree row is activated (M10)
    {
        let store = Rc::clone(&tree_store);
        let nb_editor = notebook.clone();
        let cfg_editor = Rc::clone(&cfg);
        #[allow(deprecated)]
        tree_view.connect_row_activated(move |_tv, path, _col| {
            if let Some(iter) = filetree::iter_for_path(&store, path) {
                let (file_path, is_dir) = filetree::row_info(&store, &iter);
                if !is_dir {
                    editor::open(&file_path, &nb_editor, &cfg_editor);
                }
            }
        });
    }

    // Lazy-load directory children when a row with a placeholder is expanded.
    // We replace the placeholder with a "Loading…" row immediately so the
    // parent stays open while the background scan runs.
    {
        let store = Rc::clone(&tree_store);
        let tx = ui_tx.clone();
        #[allow(deprecated)]
        tree_view.connect_row_expanded(move |_tv, iter, _path| {
            if filetree::has_placeholder(&store, iter) {
                filetree::set_loading(&store, iter);
                let (file_path, _) = filetree::row_info(&store, iter);
                let tx = tx.clone();
                std::thread::spawn(move || {
                    let entries = filetree::scan_subtree(&file_path);
                    let _ = tx.send(UiResult::Subtree { path: file_path, entries });
                });
            }
        });
    }

    // Push button: run git push in background thread
    {
        let last_cwd_c = Rc::clone(&last_cwd);
        let tx = ui_tx.clone();
        push_btn.connect_clicked(move |btn| {
            btn.set_sensitive(false);
            btn.set_label("pushing…");
            let cwd = last_cwd_c.borrow().clone();
            let tx = tx.clone();
            std::thread::spawn(move || {
                let result = git::push(&cwd);
                let _ = tx.send(UiResult::Push { result });
            });
        });
    }

    for (idx, (btn, page)) in [
        (btn_files.clone(), "files"),
        (btn_git.clone(),   "git"),
        (btn_search.clone(),"search"),
        (btn_run.clone(),   "run"),
    ].into_iter().enumerate() {
        let sp = Rc::clone(&switch_panel);
        let entry_c = (page == "search").then(|| search_entry.clone());
        btn.connect_clicked(move |_| {
            sp(page, idx);
            if let Some(e) = &entry_c {
                let e = e.clone();
                glib::idle_add_local_once(move || { e.grab_focus(); });
            }
        });
    }

    // Search: run on Enter, cancel stale results with a generation counter
    {
        let last_cwd_c = Rc::clone(&last_cwd);
        let tx = ui_tx.clone();
        let gen_c = Rc::clone(&search_gen);
        search_entry.connect_activate(move |entry| {
            let query = entry.text().to_string();
            let root = {
                let cwd = last_cwd_c.borrow().clone();
                git::repo_root(&cwd).unwrap_or(cwd)
            };
            let gen = gen_c.get() + 1;
            gen_c.set(gen);
            let tx = tx.clone();
            std::thread::spawn(move || {
                let results = searchpanel::run_search(&root, &query);
                let _ = tx.send(UiResult::SearchDone { gen, results });
            });
        });
    }

    // Dispatch IPC commands on the GTK main thread via an async task
    {
        let nb = notebook.clone();
        let cfg = Rc::clone(&cfg);
        glib::MainContext::default().spawn_local(async move {
            while let Ok(req) = ipc_rx.recv().await {
                let resp = match req.command {
                    ipc::Command::Ping => ipc::Response {
                        ok: true,
                        error: None,
                        accepted: None,
                    },
                    ipc::Command::NewTab => {
                        add_tab(&nb, &cfg, None);
                        ipc::Response {
                            ok: true,
                            error: None,
                            accepted: None,
                        }
                    }
                    ipc::Command::ShowDiff {
                        path,
                        old,
                        new_content,
                    } => {
                        let (tx, rx) = async_channel::bounded::<bool>(1);
                        diff::open(&path, &old, &new_content, &nb, tx);
                        let accepted = rx.recv().await.unwrap_or(false);
                        ipc::Response {
                            ok: true,
                            error: None,
                            accepted: Some(accepted),
                        }
                    }
                };
                let _ = req.reply.send(resp);
            }
        });
    }

    window.present();
}

fn add_tab(notebook: &Notebook, cfg: &config::Config, cwd: Option<&str>) {
    let terminal = tab::build(cfg);
    let page_idx = notebook.n_pages();

    let label = gtk4::Label::new(Some("  ~  "));
    notebook.append_page(&terminal, Some(&label));
    notebook.set_current_page(Some(page_idx));
    let t = terminal.clone();
    glib::idle_add_local(move || {
        t.grab_focus();
        glib::ControlFlow::Break
    });

    let shell = std::env::var("SHELL").unwrap_or_else(|_| "/bin/bash".to_string());
    let pid_cell: Rc<Cell<i32>> = Rc::new(Cell::new(0));
    let pid_for_spawn = Rc::clone(&pid_cell);

    terminal.spawn_async(
        vte4::PtyFlags::DEFAULT,
        cwd,
        &[shell.as_str()],
        &["PROMPT_SP=", "PROMPT_CR="],
        glib::SpawnFlags::DEFAULT,
        || {},
        -1,
        None::<&gio::Cancellable>,
        move |result| {
            if let Ok(pid) = result {
                pid_for_spawn.set(pid.0);
            }
        },
    );

    // Notification ring: mark tab dirty when shell returns to prompt in a background tab.
    // Requires shell integration to emit OSC 133;A (see shell-integration.zsh).
    let dirty: Rc<Cell<bool>> = Rc::new(Cell::new(false));
    {
        let dirty_c = Rc::clone(&dirty);
        let nb_c = notebook.clone();
        let term_c = terminal.clone();
        terminal.connect_termprop_changed(Some("vte.shell.precmd"), move |_, _| {
            if dirty_c.get() {
                return;
            }
            let tw: gtk4::Widget = term_c.clone().upcast();
            if let Some(page) = notebook_page_of(&tw, &nb_c) {
                if nb_c.page_num(&page) != nb_c.current_page() {
                    dirty_c.set(true);
                }
            }
        });
    }

    // Poll cwd + git branch every second; show ring indicator when dirty
    {
        let label_ref = label.clone();
        let pid_ref = Rc::clone(&pid_cell);
        let dirty_ref = Rc::clone(&dirty);
        let nb_ref = notebook.clone();
        let term_ref = terminal.clone();
        glib::timeout_add_seconds_local(1, move || {
            let pid = pid_ref.get();
            if pid <= 0 {
                return glib::ControlFlow::Continue;
            }
            if !std::path::Path::new(&format!("/proc/{}", pid)).exists() {
                return glib::ControlFlow::Break;
            }

            let tw: gtk4::Widget = term_ref.clone().upcast();
            if let Some(page) = notebook_page_of(&tw, &nb_ref) {
                if nb_ref.page_num(&page) == nb_ref.current_page() {
                    dirty_ref.set(false);
                }
            }

            let title = tab::tab_title(pid);
            if dirty_ref.get() {
                label_ref.set_markup(&format!(
                    "<span foreground=\"#f38ba8\">●</span>{}",
                    glib::markup_escape_text(title.trim_start()),
                ));
            } else {
                label_ref.set_text(&title);
            }
            glib::ControlFlow::Continue
        });
    }

    let nb = notebook.clone();
    let weak = terminal.downgrade();
    terminal.connect_child_exited(move |_, _| {
        if let Some(t) = weak.upgrade() {
            pane::close_terminal(&t, &nb);
        }
    });
}

fn focused_terminal(window: &ApplicationWindow, notebook: &Notebook) -> Option<vte4::Terminal> {
    if let Some(term) = gtk4::prelude::GtkWindowExt::focus(window)
        .and_then(|w| w.downcast::<vte4::Terminal>().ok())
    {
        return Some(term);
    }
    let page = notebook.nth_page(Some(notebook.current_page()?))?;
    pane::collect_terminals_pub(&page).into_iter().next()
}

fn focused_terminal_cwd(window: &ApplicationWindow, notebook: &Notebook) -> Option<String> {
    focused_terminal(window, notebook).and_then(|t| terminal_cwd(&t))
}

/// Get the cwd of the foreground process running in a terminal via the PTY.
fn terminal_cwd(terminal: &vte4::Terminal) -> Option<String> {
    use std::os::unix::io::AsRawFd;
    let pty = terminal.pty()?;
    let raw_fd = pty.fd().as_raw_fd();
    let pgid = unsafe { libc::tcgetpgrp(raw_fd) };
    if pgid <= 0 {
        return None;
    }
    std::fs::read_link(format!("/proc/{}/cwd", pgid))
        .ok()
        .map(|p| p.to_string_lossy().to_string())
}

/// Walk up the widget tree to find the notebook page that contains `widget`.
fn notebook_page_of(widget: &gtk4::Widget, notebook: &Notebook) -> Option<gtk4::Widget> {
    let mut w = widget.clone();
    loop {
        if notebook.page_num(&w).is_some() {
            return Some(w);
        }
        w = w.parent()?;
    }
}

fn build_css(cfg: &config::Config) -> String {
    let font = &cfg.font.family;
    let fsize = cfg.font.size;
    let sidebar_pt = (fsize - 2).max(10);
    let run_task_pt = (fsize - 4).max(9);
    format!(
        "
        window {{ background: transparent; }}

        vte-terminal {{ padding: {p}px; }}

        notebook header {{
            background-color: #181825;
            border-bottom: 1px solid #313244;
            padding: 0;
        }}
        notebook header tab {{
            color: #6c7086;
            padding: 6px 16px;
            border-radius: 0;
            border: none;
            box-shadow: none;
        }}
        notebook header tab:checked {{
            color: #cdd6f4;
            background-color: #1e1e2e;
            border-bottom: 2px solid #cba6f7;
        }}
        notebook header tab:hover:not(:checked) {{
            color: #bac2de;
            background-color: #1e1e2e;
        }}
        notebook > stack {{ background-color: transparent; }}

        .sidebar {{
            background-color: #181825;
            border-right: 1px solid #313244;
        }}
        .sidebar-header {{
            background-color: #181825;
            color: #a6adc8;
            font-size: 11px;
            font-weight: bold;
            padding: 10px 12px 6px 12px;
            letter-spacing: 1px;
        }}
        .editor-view,
        .editor-view text {{
            font-family: {font};
            font-size: {fsize}pt;
            background-color: #1e1e2e;
            color: #cdd6f4;
        }}
        .file-tree {{
            background-color: #181825;
            color: #cdd6f4;
            font-family: {font};
            font-size: {sidebar_pt}pt;
        }}
        .file-tree:selected {{
            background-color: #313244;
            color: #cdd6f4;
        }}
        .git-section-header {{
            color: #6c7086;
            font-size: 10px;
            font-weight: bold;
            letter-spacing: 1px;
        }}

        .browser-panel {{
            background-color: #1e1e2e;
            border-left: 1px solid #313244;
        }}
        .browser-nav {{
            background-color: #181825;
            border-bottom: 1px solid #313244;
        }}
        .browser-nav-btn {{
            color: #cdd6f4;
            background-color: #313244;
            border: none;
            border-radius: 4px;
            padding: 2px 6px;
            min-width: 0;
            min-height: 0;
        }}
        .browser-nav-btn:hover {{
            background-color: #45475a;
        }}
        .browser-nav entry {{
            background-color: #313244;
            color: #cdd6f4;
            border: 1px solid #45475a;
            border-radius: 4px;
            padding: 2px 8px;
        }}
        .browser-nav entry:focus {{
            border-color: #cba6f7;
        }}

        .activity-bar {{
            background-color: #11111b;
            border-right: 1px solid #313244;
            padding: 4px 0;
        }}
        .activity-btn {{
            background: transparent;
            border: none;
            border-radius: 0;
            box-shadow: none;
            padding: 10px;
            min-width: 40px;
            min-height: 40px;
            color: #6c7086;
        }}
        .activity-btn:hover {{
            color: #cdd6f4;
            background-color: rgba(255,255,255,0.05);
        }}
        .activity-btn.active {{
            color: #cdd6f4;
            border-left: 2px solid #cba6f7;
        }}

        .push-btn {{
            background-color: #313244;
            color: #a6e3a1;
            border: none;
            border-radius: 4px;
            padding: 6px 12px;
            margin: 6px 8px 8px 8px;
            font-size: {sidebar_pt}pt;
            font-weight: bold;
        }}
        .push-btn:hover {{
            background-color: #45475a;
        }}
        .push-btn:disabled {{
            color: #6c7086;
        }}

        .context-menu {{
            background: #1e1e2e;
            border: 1px solid #313244;
            border-radius: 6px;
            padding: 4px;
        }}
        .context-menu-item {{
            background: transparent;
            border: none;
            border-radius: 4px;
            color: #cdd6f4;
            font-family: {font};
            font-size: {sidebar_pt}pt;
            padding: 4px 12px;
            min-height: 0;
        }}
        .context-menu-item:hover {{
            background: #313244;
        }}

        .search-result-file {{
            color: #89b4fa;
            font-family: {font};
            font-size: {sidebar_pt}pt;
            font-weight: bold;
        }}
        .search-result-line {{
            color: #6c7086;
            font-family: {font};
            font-size: {sidebar_pt}pt;
        }}
        .search-result-text {{
            color: #cdd6f4;
            font-family: {font};
            font-size: {sidebar_pt}pt;
        }}

        .run-task-name {{
            color: #cdd6f4;
            font-family: {font};
            font-size: {run_task_pt}pt;
        }}
        .run-btn {{
            background-color: transparent;
            border: none;
            border-radius: 4px;
            color: #6c7086;
            padding: 0 4px;
            min-width: 0;
            min-height: 0;
            font-size: {sidebar_pt}pt;
        }}
        .run-btn:hover {{
            background-color: #313244;
            color: #cdd6f4;
        }}

        .quickopen-window {{
            background: #1e1e2e;
            border: 1px solid #45475a;
            border-radius: 8px;
        }}
        .quickopen-entry {{
            background: #313244;
            color: #cdd6f4;
            border: none;
            border-radius: 6px;
            font-family: {font};
            font-size: {sidebar_pt}pt;
        }}
        .quickopen-list {{
            background: transparent;
        }}
        .quickopen-list row {{
            border-radius: 4px;
        }}
        .quickopen-list row:hover, .quickopen-list row:selected {{
            background: #313244;
        }}
        .quickopen-name {{
            color: #cdd6f4;
            font-family: {font};
            font-size: {sidebar_pt}pt;
        }}
        .quickopen-path {{
            color: #6c7086;
            font-family: {font};
            font-size: {sidebar_pt}pt;
        }}
        ",
        p = cfg.window.padding,
    )
}
