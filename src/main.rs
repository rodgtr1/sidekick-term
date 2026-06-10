mod agentpanel;
mod browser;
mod config;
mod diff;
mod editor;
mod filetree;
mod git;
mod gitpanel;
mod hostspanel;
mod ipc;
mod limits;
mod palette;
mod pane;
mod quickopen;
mod runpanel;
mod scrollsearch;
mod searchpanel;
mod session;
mod shortcutshelp;
mod tab;
mod theme;

use gio::prelude::*;
use gtk4::prelude::*;
use gtk4::{gdk, Application, ApplicationWindow, Notebook};
use std::cell::{Cell, RefCell};
use std::collections::HashMap;
use std::ffi::CString;
use std::rc::Rc;
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::{Duration, Instant};
use vte4::prelude::*;
use webkit6::prelude::WebViewExt as _;

const APP_ID: &str = "com.travismedia.sidekick";
const AGENT_STATUS_TERMPROP: &str = "vte.ext.sidekick.agent";
const CMD_EXIT_TERMPROP: &str = "vte.ext.sidekick.exit";
/// Commands that ran at least this long trigger a desktop notification on
/// finish when the window is unfocused.
const LONG_COMMAND_NOTIFY_SECS: u64 = 15;
const SIDE_RAIL_WIDTH: i32 = 220;
const TOOL_PANEL_WIDTH: i32 = SIDE_RAIL_WIDTH - 1;
const NOTEBOOK_TAB_WIDTH: i32 = SIDE_RAIL_WIDTH - 3;
const SESSION_TAB_WIDTH: i32 = NOTEBOOK_TAB_WIDTH - 22;

#[derive(Clone, Copy, PartialEq)]
enum AgentState {
    Idle,
    AutoBusy,
    Busy,
    Ready,
    Done,
}

impl AgentState {
    fn label(self) -> &'static str {
        match self {
            AgentState::Idle => "IDLE",
            AgentState::AutoBusy | AgentState::Busy => "RUN",
            AgentState::Ready => "WAIT",
            AgentState::Done => "DONE",
        }
    }

    /// The tab-dot color for this state.
    fn color(self) -> &'static str {
        match self {
            AgentState::Idle => "#6c7086",
            AgentState::AutoBusy | AgentState::Busy => "#f9e2af",
            AgentState::Ready => "#a6e3a1",
            AgentState::Done => "#89b4fa",
        }
    }
}

type AgentCell = Rc<Cell<AgentState>>;
/// Terminal widget pointer -> (tab id, shared agent state). Terminals split
/// from the same tab share one AgentCell so any pane drives the tab dot.
type AgentMap = Rc<RefCell<HashMap<usize, (u64, AgentCell)>>>;
type SaveCallback = Rc<dyn Fn(&str)>;
type TaskPopulator = Rc<dyn Fn(&[runpanel::Task])>;

/// Tab ids are exported to each shell as SIDEKICK_TAB_ID so out-of-band
/// status updates (sidekick-ctl) can address the terminal they ran in.
static NEXT_TAB_ID: AtomicU64 = AtomicU64::new(1);

enum UiResult {
    Tree {
        shell_cwd: String,
        tree_root: String,
        entries: Vec<filetree::TreeEntry>,
        tasks: Vec<runpanel::Task>,
    },
    /// The cwd changed but the repo root did not — skip the tree rebuild and
    /// only refresh the (cheap) task list.
    TreeUnchanged {
        shell_cwd: String,
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
        branch: Option<String>,
    },
    Diff {
        title: String,
        result: Result<String, String>,
    },
    Push {
        result: Result<(), String>,
    },
    Pull {
        result: Result<(), String>,
    },
    Commit {
        result: Result<(), String>,
    },
    SearchDone {
        gen: u64,
        results: Vec<searchpanel::FileMatches>,
    },
}

fn main() -> glib::ExitCode {
    let raw: Vec<String> = std::env::args().collect();
    let mut initial_dir: Option<String> = None;
    let mut gtk_args: Vec<String> = vec![raw[0].clone()];
    let mut i = 1;
    while i < raw.len() {
        if raw[i] == "--dir" || raw[i] == "-d" {
            i += 1;
            if i < raw.len() {
                initial_dir = Some(raw[i].clone());
            }
        } else if let Some(val) = raw[i].strip_prefix("--dir=") {
            initial_dir = Some(val.to_string());
        } else {
            gtk_args.push(raw[i].clone());
        }
        i += 1;
    }

    let app = Application::builder().application_id(APP_ID).build();
    app.connect_activate(move |app| build_ui(app, initial_dir.as_deref()));
    app.run_with_args(&gtk_args)
}

fn build_ui(app: &Application, initial_dir: Option<&str>) {
    install_agent_status_termprop();

    let cfg = Rc::new(RefCell::new(config::Config::load()));

    let ipc_rx = ipc::start();

    let notebook = Notebook::new();
    notebook.set_scrollable(true);
    notebook.set_show_border(false);
    notebook.set_tab_pos(gtk4::PositionType::Left);
    notebook.add_css_class("terminal-notebook");

    // File tree
    let (tree_header, tree_view, tree_store, tree_scroll) = filetree::build();
    let tree_store = Rc::new(tree_store);

    // Git panel
    let git_panel = gitpanel::build();
    let git_list_header = git_panel.header.clone();
    let git_branch_label = git_panel.branch_label.clone();
    let stage_all_btn = git_panel.stage_all_btn.clone();
    let unstage_all_btn = git_panel.unstage_all_btn.clone();
    let git_list = git_panel.list.clone();
    let push_btn = git_panel.push_btn.clone();
    let pull_btn = git_panel.pull_btn.clone();
    let commit_btn = git_panel.commit_btn.clone();
    let commit_view = git_panel.commit_view.clone();
    let git_files: Rc<RefCell<Vec<git::GitFile>>> = Rc::new(RefCell::new(Vec::new()));

    // Per-terminal agent state: key = terminal widget pointer as usize
    let agent_map: AgentMap = Rc::new(RefCell::new(HashMap::new()));

    // Search panel
    let (search_panel, search_entry, search_list) = searchpanel::build();
    let search_gen: Rc<Cell<u64>> = Rc::new(Cell::new(0));

    // Run panel
    let (run_panel, run_list) = runpanel::build();

    // Agents dashboard panel
    let agent_panel = Rc::new(agentpanel::build());

    // Hosts panel (ssh config + teleport)
    let hosts_panel = hostspanel::build();

    // File tree page (header + scroll stacked vertically)
    let files_page = gtk4::Box::new(gtk4::Orientation::Vertical, 0);
    files_page.append(&tree_header);
    files_page.append(&tree_scroll);

    // Panel stack switches between panels
    let panel_stack = gtk4::Stack::new();
    panel_stack.set_transition_type(gtk4::StackTransitionType::None);
    panel_stack.set_hexpand(false);
    panel_stack.add_named(&files_page, Some("files"));
    panel_stack.add_named(&git_panel.widget, Some("git"));
    panel_stack.add_named(&search_panel, Some("search"));
    panel_stack.add_named(&run_panel, Some("run"));
    panel_stack.add_named(&agent_panel.widget, Some("agents"));
    panel_stack.add_named(&hosts_panel.widget, Some("hosts"));

    // Activity bar: narrow icon strip on the far left
    let activity_bar = gtk4::Box::new(gtk4::Orientation::Vertical, 0);
    activity_bar.add_css_class("activity-bar");
    activity_bar.set_width_request(40);

    let btn_files = gtk4::Button::new();
    btn_files.add_css_class("activity-btn");
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

    let btn_agents = gtk4::Button::new();
    btn_agents.add_css_class("activity-btn");
    let img_agents = gtk4::Image::from_icon_name("system-users-symbolic");
    img_agents.set_pixel_size(20);
    btn_agents.set_child(Some(&img_agents));
    btn_agents.set_tooltip_text(Some("Agents (Ctrl+Shift+A)"));

    let btn_hosts = gtk4::Button::new();
    btn_hosts.add_css_class("activity-btn");
    let img_hosts = gtk4::Image::from_icon_name("network-server-symbolic");
    img_hosts.set_pixel_size(20);
    btn_hosts.set_child(Some(&img_hosts));
    btn_hosts.set_tooltip_text(Some("Hosts"));

    activity_bar.append(&btn_files);
    activity_bar.append(&btn_git);
    activity_bar.append(&btn_search);
    activity_bar.append(&btn_run);
    activity_bar.append(&btn_agents);
    activity_bar.append(&btn_hosts);

    // Badge at the bottom of the activity bar: count of agents waiting for input.
    let badge_spacer = gtk4::Box::new(gtk4::Orientation::Vertical, 0);
    badge_spacer.set_vexpand(true);
    let agent_badge = gtk4::Label::new(None);
    agent_badge.add_css_class("agent-badge");
    agent_badge.set_visible(false);
    activity_bar.append(&badge_spacer);
    activity_bar.append(&agent_badge);
    {
        let agent_map_b = Rc::clone(&agent_map);
        let badge = agent_badge.clone();
        glib::timeout_add_seconds_local(1, move || {
            // Split panes share one state cell per tab id — count unique tabs.
            let mut waiting: std::collections::HashSet<u64> = std::collections::HashSet::new();
            for (tab_id, state) in agent_map_b.borrow().values() {
                if matches!(state.get(), AgentState::Ready) {
                    waiting.insert(*tab_id);
                }
            }
            let n = waiting.len();
            if n > 0 {
                badge.set_text(&n.to_string());
                badge.set_tooltip_text(Some(&format!(
                    "{n} agent{} waiting for input",
                    if n == 1 { "" } else { "s" }
                )));
            }
            badge.set_visible(n > 0);
            glib::ControlFlow::Continue
        });
    }

    // Panel stack is the togglable sidebar content (no activity bar inside)
    let tree_sidebar = panel_stack.clone();
    tree_sidebar.set_width_request(TOOL_PANEL_WIDTH);
    tree_sidebar.add_css_class("sidebar");
    tree_sidebar.set_visible(false);

    // Browser panel (hidden by default)
    let browser_panel = browser::build();
    browser_panel.widget.set_visible(false);
    browser_panel.widget.set_hexpand(true);

    // Content paned: notebook | browser
    let content_paned = gtk4::Paned::new(gtk4::Orientation::Horizontal);
    content_paned.add_css_class("content-paned");
    content_paned.set_start_child(Some(&notebook));
    content_paned.set_end_child(Some(&browser_panel.widget));
    content_paned.set_shrink_start_child(false);
    content_paned.set_shrink_end_child(false);
    content_paned.set_resize_start_child(true);
    content_paned.set_resize_end_child(true);

    content_paned.set_hexpand(true);

    // Root layout starts minimized: activity bar (always visible) | content.
    // The sidebar is inserted between them only when opened.
    let root_box = gtk4::Box::new(gtk4::Orientation::Horizontal, 0);
    root_box.append(&activity_bar);
    root_box.append(&content_paned);

    // Sidebar content starts minimized; the activity bar remains visible.
    let sidebar_visible: Rc<Cell<bool>> = Rc::new(Cell::new(false));
    let browser_visible: Rc<Cell<bool>> = Rc::new(Cell::new(false));

    // Restore the previous session's tabs unless the user asked for a
    // specific directory or disabled restore.
    let restored = initial_dir.is_none()
        && cfg.borrow().behavior.restore_session
        && restore_session(&notebook, &cfg.borrow(), &agent_map);
    if !restored {
        add_tab(&notebook, &cfg.borrow(), initial_dir, &agent_map);
    }

    let css = gtk4::CssProvider::new();
    css.load_from_string(&build_css(&cfg.borrow()));
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

    // Persist the session on close and periodically (crash safety).
    {
        let nb = notebook.clone();
        window.connect_close_request(move |_| {
            session::save(&snapshot_session(&nb));
            glib::Propagation::Proceed
        });
    }
    {
        let nb = notebook.clone();
        glib::timeout_add_seconds_local(60, move || {
            session::save(&snapshot_session(&nb));
            glib::ControlFlow::Continue
        });
    }

    // Track last known cwd to avoid redundant tree reloads
    let last_cwd: Rc<RefCell<String>> = Rc::new(RefCell::new(String::new()));
    // Track the last applied tree root so cd-ing around inside one repo does
    // not trigger full rescans of an identical tree.
    let last_tree_root: Rc<RefCell<String>> = Rc::new(RefCell::new(String::new()));
    let (ui_tx, ui_rx) = async_channel::unbounded::<UiResult>();
    let tree_busy = Rc::new(Cell::new(false));
    let git_busy = Rc::new(Cell::new(false));

    // Open the browser panel (if hidden) and load a URL — used by Ctrl+Shift+O
    // and by run-panel tasks with open_browser set.
    let show_browser: Rc<dyn Fn(&str)> = Rc::new({
        let vis = Rc::clone(&browser_visible);
        let wgt = browser_panel.widget.clone();
        let wv = browser_panel.webview.clone();
        let cpaned = content_paned.clone();
        move |url: &str| {
            if !vis.get() {
                vis.set(true);
                wgt.set_visible(true);
                let total = cpaned.width();
                cpaned.set_position(if total > 0 { total / 2 } else { 600 });
            }
            wv.load_uri(url);
        }
    });

    // Shared run-panel population for both tree refreshes and config reloads.
    // Paste types the command into the focused terminal; Run executes it in a
    // dedicated split below so agent prompts stay clean.
    let populate_tasks: TaskPopulator = Rc::new({
        let run_list = run_list.clone();
        let cfg = Rc::clone(&cfg);
        let win = window.clone();
        let nb = notebook.clone();
        let agent_map_t = Rc::clone(&agent_map);
        let show_browser = Rc::clone(&show_browser);
        move |local_tasks| {
            let win_r = win.clone();
            let nb_r = nb.clone();
            let cfg_r = Rc::clone(&cfg);
            let agent_map_r = Rc::clone(&agent_map_t);
            let show_browser_r = Rc::clone(&show_browser);
            runpanel::populate(
                &run_list,
                &cfg.borrow().tasks,
                local_tasks,
                move |task, action, status| match action {
                    runpanel::TaskAction::Paste => {
                        if let Some(term) = focused_terminal(&win_r, &nb_r) {
                            term.feed_child(task.cmd.as_bytes());
                            term.grab_focus();
                        }
                    }
                    runpanel::TaskAction::Run => {
                        let Some(focused) = focused_terminal(&win_r, &nb_r) else {
                            return;
                        };
                        let cwd = terminal_cwd(&focused);
                        let pid_cell: Rc<Cell<i32>> = Rc::new(Cell::new(0));
                        let new_term = split_terminal(
                            &nb_r,
                            &cfg_r.borrow(),
                            &agent_map_r,
                            &focused,
                            gtk4::Orientation::Vertical,
                            cwd.as_deref(),
                            Some(task.cmd.clone()),
                            Some(Rc::clone(&pid_cell)),
                        );
                        track_task_status(&new_term, pid_cell, status);
                        if let Some(url) = &task.open_browser {
                            show_browser_r(url);
                        }
                    }
                },
            );
        }
    });

    // Callback that triggers a background git status refresh (used by the context menu).
    let refresh_git: Rc<dyn Fn()> = {
        let last_cwd = Rc::clone(&last_cwd);
        let tx = ui_tx.clone();
        Rc::new(move || {
            let cwd = last_cwd.borrow().clone();
            if cwd.is_empty() {
                return;
            }
            let tx = tx.clone();
            std::thread::spawn(move || {
                let root = git::repo_root(&cwd).unwrap_or_else(|| cwd.clone());
                let files = git::changed_files(&cwd);
                let ahead = git::ahead_count(&cwd);
                let branch = git::current_branch(&root);
                let _ = tx.send_blocking(UiResult::Git {
                    cwd,
                    root,
                    files,
                    ahead,
                    branch,
                });
            });
        })
    };

    // Apply filesystem/git work that completed off the GTK thread.
    {
        let rx = ui_rx.clone();
        let store = Rc::clone(&tree_store);
        let tv = tree_view.clone();
        let last = Rc::clone(&last_cwd);
        let last_root_c = Rc::clone(&last_tree_root);
        let header_c = tree_header.clone();
        let git_list_c = git_list.clone();
        let git_hdr_c = git_list_header.clone();
        let git_branch_c = git_branch_label.clone();
        let git_files_c = Rc::clone(&git_files);
        let tree_busy_c = Rc::clone(&tree_busy);
        let git_busy_c = Rc::clone(&git_busy);
        let nb_c = notebook.clone();
        let push_btn_c = push_btn.clone();
        let pull_btn_c = pull_btn.clone();
        let commit_btn_c = commit_btn.clone();
        let commit_view_c = commit_view.clone();
        let win_c = window.clone();
        let search_list_c = search_list.clone();
        let search_gen_c = Rc::clone(&search_gen);
        let populate_tasks_c = Rc::clone(&populate_tasks);
        let refresh_git_c = Rc::clone(&refresh_git);
        glib::MainContext::default().spawn_local(async move {
            while let Ok(result) = rx.recv().await {
                match result {
                    UiResult::Tree {
                        shell_cwd,
                        tree_root,
                        entries,
                        tasks,
                    } => {
                        tree_busy_c.set(false);
                        if *last.borrow() == shell_cwd {
                            let name = std::path::Path::new(&tree_root)
                                .file_name()
                                .map(|n| n.to_string_lossy().to_string())
                                .unwrap_or_else(|| tree_root.clone());
                            header_c.set_text(&name);
                            filetree::apply_root(&store, &tv, &entries);
                            *last_root_c.borrow_mut() = tree_root;
                            populate_tasks_c(&tasks);
                        }
                    }
                    UiResult::TreeUnchanged { shell_cwd, tasks } => {
                        tree_busy_c.set(false);
                        if *last.borrow() == shell_cwd {
                            populate_tasks_c(&tasks);
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
                    UiResult::Git {
                        cwd,
                        root,
                        files,
                        ahead,
                        branch,
                    } => {
                        git_busy_c.set(false);
                        if *last.borrow() == cwd {
                            match &branch {
                                Some(b) => git_branch_c.set_text(&format!("⎇ {b}")),
                                None => git_branch_c.set_text(""),
                            }
                            let count = files.len();
                            git_hdr_c.set_text(
                                if count > 0 {
                                    format!("GIT CHANGES ({})", count)
                                } else {
                                    "GIT CHANGES".to_string()
                                }
                                .as_str(),
                            );
                            let staged_count =
                                gitpanel::populate(&git_list_c, &files, &root, &refresh_git_c);
                            *git_files_c.borrow_mut() = files;
                            gitpanel::update_push_button(&push_btn_c, ahead);
                            gitpanel::update_commit_button(&commit_btn_c, staged_count);
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
                            push_btn_c.set_label("↑  push");
                            push_btn_c.set_sensitive(true);
                        }
                        Err(msg) => {
                            push_btn_c.set_sensitive(true);
                            push_btn_c.set_label("↑  push");
                            gtk4::AlertDialog::builder()
                                .message("Push failed")
                                .detail(&msg)
                                .build()
                                .show(Some(&win_c));
                        }
                    },
                    UiResult::Pull { result } => match result {
                        Ok(()) => {
                            pull_btn_c.set_sensitive(true);
                            pull_btn_c.set_label("↓  pull");
                            refresh_git_c();
                        }
                        Err(msg) => {
                            pull_btn_c.set_sensitive(true);
                            pull_btn_c.set_label("↓  pull");
                            gtk4::AlertDialog::builder()
                                .message("Pull failed")
                                .detail(&msg)
                                .build()
                                .show(Some(&win_c));
                        }
                    },
                    UiResult::Commit { result } => match result {
                        Ok(()) => {
                            commit_btn_c.set_label("Commit staged");
                            commit_btn_c.set_sensitive(false);
                            commit_view_c.buffer().set_text("");
                            refresh_git_c();
                        }
                        Err(msg) => {
                            commit_btn_c.set_label("Commit staged");
                            commit_btn_c.set_sensitive(true);
                            gtk4::AlertDialog::builder()
                                .message("Commit failed")
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
        });
    }

    // Poll active terminal's cwd; refresh file tree when it changes.
    {
        let nb = notebook.clone();
        let last = Rc::clone(&last_cwd);
        let last_root = Rc::clone(&last_tree_root);
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
                    let prev_root = last_root.borrow().clone();
                    std::thread::spawn(move || {
                        let tree_root =
                            git::repo_root(&shell_cwd).unwrap_or_else(|| shell_cwd.clone());
                        let tasks = runpanel::load_tasks(&tree_root);
                        // cd within the same repo: the tree is identical, so
                        // skip the expensive rescan and ignored-set rebuild.
                        if !prev_root.is_empty() && tree_root == prev_root {
                            let _ = tx.send_blocking(UiResult::TreeUnchanged { shell_cwd, tasks });
                            return;
                        }
                        let entries = filetree::scan_root(&tree_root);
                        let _ = tx.send_blocking(UiResult::Tree {
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
                    let branch = git::current_branch(&root);
                    let _ = tx.send_blocking(UiResult::Git {
                        cwd,
                        root,
                        files,
                        ahead,
                        branch,
                    });
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
                    let _ = tx.send_blocking(UiResult::Diff { title, result });
                });
            }
        });
    }

    // Open file in editor when a search result is clicked
    let on_editor_saved: Rc<RefCell<Option<SaveCallback>>> = Rc::new(RefCell::new(None));
    {
        let nb_c = notebook.clone();
        let cfg_c = Rc::clone(&cfg);
        let on_saved_ref = Rc::clone(&on_editor_saved);
        search_list.connect_row_activated(move |_, row| {
            // Row names encode "line\npath" (see searchpanel::row_name).
            let name = row.widget_name().to_string();
            let (line, path) = match name.split_once(searchpanel::ROW_NAME_SEP) {
                Some((l, p)) => (l.parse::<u32>().ok().filter(|n| *n > 0), p.to_string()),
                None => (None, name),
            };
            if !path.is_empty() {
                editor::open_at_line(
                    &path,
                    line,
                    &nb_c,
                    &cfg_c.borrow(),
                    on_saved_ref.borrow().as_ref().map(Rc::clone),
                );
            }
        });
    }

    let reload_config: Rc<dyn Fn()> = Rc::new({
        let cfg = Rc::clone(&cfg);
        let css = css.clone();
        let root_widget: gtk4::Widget = root_box.clone().upcast();
        let last_cwd = Rc::clone(&last_cwd);
        let populate_tasks = Rc::clone(&populate_tasks);
        move || {
            let next = config::Config::load();
            *cfg.borrow_mut() = next;

            {
                let cfg_ref = cfg.borrow();
                css.load_from_string(&build_css(&cfg_ref));
                apply_config_to_open_widgets(&root_widget, &cfg_ref);
            }

            let cwd = last_cwd.borrow().clone();
            if !cwd.is_empty() {
                let root = git::repo_root(&cwd).unwrap_or(cwd);
                let local_tasks = runpanel::load_tasks(&root);
                populate_tasks(&local_tasks);
            }
        }
    });

    {
        let config_path = config::config_path();
        let reload_config = Rc::clone(&reload_config);
        *on_editor_saved.borrow_mut() = Some(Rc::new(move |path| {
            if std::path::Path::new(path) == config_path.as_path() {
                reload_config();
            }
        }));
    }
    {
        let config_file = gio::File::for_path(config::config_path());
        match config_file.monitor_file(gio::FileMonitorFlags::NONE, None::<&gio::Cancellable>) {
            Ok(config_monitor) => {
                let reload_config = Rc::clone(&reload_config);
                let debounce: Rc<RefCell<Option<glib::SourceId>>> = Rc::new(RefCell::new(None));
                config_monitor.connect_changed(move |_, _, _, _| {
                    if let Some(id) = debounce.borrow_mut().take() {
                        id.remove();
                    }

                    let reload_config = Rc::clone(&reload_config);
                    let debounce_c = Rc::clone(&debounce);
                    let id = glib::timeout_add_local_once(
                        std::time::Duration::from_millis(150),
                        move || {
                            *debounce_c.borrow_mut() = None;
                            reload_config();
                        },
                    );
                    *debounce.borrow_mut() = Some(id);
                });

                let monitor_keepalive = config_monitor.clone();
                window.connect_destroy(move |_| {
                    monitor_keepalive.cancel();
                });
            }
            Err(e) => eprintln!("sidekick: config monitor failed: {e}"),
        }
    }

    // Shared panel-switching logic for both activity bar buttons and keyboard shortcuts.
    let all_btns = [
        btn_files.clone(),
        btn_git.clone(),
        btn_search.clone(),
        btn_run.clone(),
        btn_agents.clone(),
        btn_hosts.clone(),
    ];
    let switch_panel: Rc<dyn Fn(&'static str, usize)> = Rc::new({
        let stack = panel_stack.clone();
        let sidebar = tree_sidebar.clone();
        let root = root_box.clone();
        let activity = activity_bar.clone();
        let vis = Rc::clone(&sidebar_visible);
        let btns = all_btns.clone();
        move |page: &'static str, btn_idx: usize| {
            let already = stack.visible_child_name().as_deref() == Some(page) && vis.get();
            if already {
                vis.set(false);
                sidebar.set_visible(false);
                if sidebar.parent().is_some() {
                    root.remove(&sidebar);
                }
                for b in btns.iter() {
                    b.remove_css_class("active");
                }
            } else {
                vis.set(true);
                if sidebar.parent().is_none() {
                    root.insert_child_after(&sidebar, Some(&activity));
                }
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

    // Sidebar / browser toggles shared by the key handler and command palette.
    let toggle_sidebar: Rc<dyn Fn()> = Rc::new({
        let vis = Rc::clone(&sidebar_visible);
        let sidebar = tree_sidebar.clone();
        let root = root_box.clone();
        let activity = activity_bar.clone();
        move || {
            let visible = !vis.get();
            vis.set(visible);
            if visible && sidebar.parent().is_none() {
                root.insert_child_after(&sidebar, Some(&activity));
            } else if !visible && sidebar.parent().is_some() {
                root.remove(&sidebar);
            }
            sidebar.set_visible(visible);
        }
    });
    let toggle_browser: Rc<dyn Fn()> = Rc::new({
        let vis = Rc::clone(&browser_visible);
        let wgt = browser_panel.widget.clone();
        let cpaned = content_paned.clone();
        move || {
            let visible = !vis.get();
            vis.set(visible);
            wgt.set_visible(visible);
            if visible {
                let total = cpaned.width();
                cpaned.set_position(if total > 0 { total / 2 } else { 600 });
                wgt.grab_focus();
            }
        }
    });

    let palette_actions = build_palette_actions(PaletteContext {
        window: &window,
        notebook: &notebook,
        cfg: &cfg,
        agent_map: &agent_map,
        last_cwd: &last_cwd,
        on_editor_saved: &on_editor_saved,
        switch_panel: &switch_panel,
        search_entry: &search_entry,
        toggle_sidebar: &toggle_sidebar,
        toggle_browser: &toggle_browser,
    });

    // Agents dashboard: refresh rows every second, jump to a tab on click.
    {
        let nb = notebook.clone();
        let agent_map_d = Rc::clone(&agent_map);
        let panel = Rc::clone(&agent_panel);
        let stack = panel_stack.clone();
        let vis = Rc::clone(&sidebar_visible);
        // tab id -> (state label, when that state started). Updated every
        // tick so elapsed times are right even while the panel is hidden.
        let since: Rc<RefCell<HashMap<u64, (&'static str, Instant)>>> =
            Rc::new(RefCell::new(HashMap::new()));
        glib::timeout_add_seconds_local(1, move || {
            let now = Instant::now();
            let mut rows: Vec<agentpanel::Row> = Vec::new();
            let mut live_tabs: std::collections::HashSet<u64> = std::collections::HashSet::new();
            for i in 0..nb.n_pages() {
                let Some(page) = nb.nth_page(Some(i)) else {
                    continue;
                };
                let Some(term) = pane::collect_terminals_pub(&page).into_iter().next() else {
                    continue;
                };
                let key = term.as_ptr() as usize;
                let Some((tab_id, state)) = agent_map_d
                    .borrow()
                    .get(&key)
                    .map(|(id, cell)| (*id, cell.get()))
                else {
                    continue;
                };
                live_tabs.insert(tab_id);
                let label = state.label();
                let elapsed_secs = {
                    let mut since_map = since.borrow_mut();
                    let entry = since_map.entry(tab_id).or_insert((label, now));
                    if entry.0 != label {
                        *entry = (label, now);
                    }
                    entry.1.elapsed().as_secs()
                };
                rows.push(agentpanel::Row {
                    page_index: i,
                    title: tab_label_title(&nb, &page)
                        .unwrap_or_else(|| format!("tab {}", i + 1)),
                    state_label: label,
                    color: state.color(),
                    elapsed_secs,
                });
            }
            since.borrow_mut().retain(|id, _| live_tabs.contains(id));
            if vis.get() && stack.visible_child_name().as_deref() == Some("agents") {
                panel.populate(&rows);
            }
            glib::ControlFlow::Continue
        });
    }
    // Hosts panel: open a new tab running the row's ssh / tsh command.
    {
        let nb = notebook.clone();
        let cfg_h = Rc::clone(&cfg);
        let agent_map_h = Rc::clone(&agent_map);
        hosts_panel.list.connect_row_activated(move |_, row| {
            let command = row.widget_name().to_string();
            if !command.is_empty() {
                add_tab_with_command(&nb, &cfg_h.borrow(), None, &agent_map_h, Some(command));
            }
        });
    }
    {
        let nb = notebook.clone();
        agent_panel.list.connect_row_activated(move |_, row| {
            let Ok(idx) = row.widget_name().parse::<u32>() else {
                return;
            };
            nb.set_current_page(Some(idx));
            if let Some(page) = nb.nth_page(Some(idx)) {
                if let Some(term) = pane::collect_terminals_pub(&page).into_iter().next() {
                    term.grab_focus();
                }
            }
        });
    }

    let key_ctrl = gtk4::EventControllerKey::new();
    key_ctrl.set_propagation_phase(gtk4::PropagationPhase::Capture);
    {
        let nb = notebook.clone();
        let cfg = Rc::clone(&cfg);
        let win = window.clone();
        let switch_panel = Rc::clone(&switch_panel);
        let search_entry_k = search_entry.clone();
        let last_cwd_qo = Rc::clone(&last_cwd);
        let cfg_qo = Rc::clone(&cfg);
        let agent_map_kb = Rc::clone(&agent_map);
        let on_saved_k = Rc::clone(&on_editor_saved);
        let toggle_sidebar_k = Rc::clone(&toggle_sidebar);
        let toggle_browser_k = Rc::clone(&toggle_browser);
        let palette_actions_k = Rc::clone(&palette_actions);
        key_ctrl.connect_key_pressed(move |_, key, _, mods| {
            let ctrl = mods.contains(gdk::ModifierType::CONTROL_MASK);
            let shift = mods.contains(gdk::ModifierType::SHIFT_MASK);
            let alt = mods.contains(gdk::ModifierType::ALT_MASK);

            // Plain Ctrl+W deliberately reaches the shell (delete-word) and
            // the browser — closing the app is Ctrl+Shift+W on the last tab.

            // Let WebView handle all key events itself — its IME/input breaks under Capture
            if gtk4::prelude::GtkWindowExt::focus(&win)
                .map(|f| f.is::<webkit6::WebView>())
                .unwrap_or(false)
            {
                return glib::Propagation::Proceed;
            }

            if let Some(term) = focused_window_terminal_on_current_page(&win, &nb) {
                if matches!(
                    (ctrl, shift, alt, key),
                    (true, true, false, gdk::Key::c | gdk::Key::C)
                        | (true, false, false, gdk::Key::Insert)
                ) {
                    term.copy_clipboard_format(vte4::Format::Text);
                    return glib::Propagation::Stop;
                }
                if matches!(
                    (ctrl, shift, alt, key),
                    (true, false, false, gdk::Key::v | gdk::Key::V)
                ) {
                    // Only claim Ctrl+V when the clipboard actually holds an
                    // image; otherwise let ^V reach the shell (verbatim
                    // insert). Text paste is Ctrl+Shift+V.
                    if clipboard_has_image(&term) {
                        paste_clipboard_image(&term);
                        return glib::Propagation::Stop;
                    }
                    return glib::Propagation::Proceed;
                }
                if matches!(
                    (ctrl, shift, alt, key),
                    (true, true, false, gdk::Key::v | gdk::Key::V)
                        | (false, true, false, gdk::Key::Insert)
                ) {
                    term.paste_clipboard();
                    return glib::Propagation::Stop;
                }
            }

            match (ctrl, shift, alt, key) {
                // New tab
                (true, true, false, gdk::Key::t | gdk::Key::T) => {
                    let cwd = focused_terminal_cwd(&win, &nb);
                    add_tab(&nb, &cfg.borrow(), cwd.as_deref(), &agent_map_kb);
                    glib::Propagation::Stop
                }
                // Close pane / tab (also closes editor tabs)
                (true, true, false, gdk::Key::w | gdk::Key::W) => {
                    if focused_terminal(&win, &nb).is_some() {
                        if !current_page_is_final_terminal_tab(&nb) {
                            pane::close(&win, &nb);
                        }
                    } else {
                        // Editor tab or other — remove current page unless it is the last one.
                        if let Some(idx) = nb.current_page() {
                            if nb.n_pages() > 1 {
                                nb.remove_page(Some(idx));
                            }
                        }
                    }
                    glib::Propagation::Stop
                }
                // Split right (side by side)
                (true, true, false, gdk::Key::d | gdk::Key::D) => {
                    split_focused(
                        &win,
                        &nb,
                        &cfg.borrow(),
                        &agent_map_kb,
                        gtk4::Orientation::Horizontal,
                    );
                    glib::Propagation::Stop
                }
                // Split down (top / bottom)
                (true, true, false, gdk::Key::x | gdk::Key::X) => {
                    split_focused(
                        &win,
                        &nb,
                        &cfg.borrow(),
                        &agent_map_kb,
                        gtk4::Orientation::Vertical,
                    );
                    glib::Propagation::Stop
                }
                // Find in scrollback
                (true, true, false, gdk::Key::h | gdk::Key::H) => {
                    if let Some(term) = focused_terminal(&win, &nb) {
                        scrollsearch::show(&win, &term);
                    }
                    glib::Propagation::Stop
                }
                // Font zoom (applies to every terminal, current and future)
                (true, false, false, gdk::Key::equal | gdk::Key::plus | gdk::Key::KP_Add)
                | (true, true, false, gdk::Key::plus | gdk::Key::equal | gdk::Key::KP_Add) => {
                    let zoom = tab::set_font_zoom(tab::font_zoom() * tab::ZOOM_STEP);
                    apply_font_zoom_all(&win, zoom);
                    glib::Propagation::Stop
                }
                (true, false, false, gdk::Key::minus | gdk::Key::KP_Subtract) => {
                    let zoom = tab::set_font_zoom(tab::font_zoom() / tab::ZOOM_STEP);
                    apply_font_zoom_all(&win, zoom);
                    glib::Propagation::Stop
                }
                (true, false, false, gdk::Key::_0 | gdk::Key::KP_0) => {
                    let zoom = tab::set_font_zoom(1.0);
                    apply_font_zoom_all(&win, zoom);
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
                    glib::idle_add_local_once(move || {
                        e.grab_focus();
                    });
                    glib::Propagation::Stop
                }
                // Quick open: file name search
                (true, false, false, gdk::Key::f | gdk::Key::F) => {
                    let cwd = last_cwd_qo.borrow().clone();
                    if !cwd.is_empty() {
                        let repo = git::repo_root(&cwd);
                        let home = std::env::var("HOME").unwrap_or_default();
                        // Outside a repo, refuse to index the entire home dir.
                        if repo.is_some() || cwd != home {
                            let root = repo.unwrap_or(cwd);
                            if let Some(on_saved) = on_saved_k.borrow().as_ref() {
                                quickopen::show(&root, &win, &nb, &cfg_qo, Rc::clone(on_saved));
                            }
                        }
                    }
                    glib::Propagation::Stop
                }
                // Open sidekick config
                (true, false, false, gdk::Key::comma) => {
                    open_config_in_nvim(&nb, &cfg.borrow(), &agent_map_kb);
                    glib::Propagation::Stop
                }
                (true, true, false, gdk::Key::r | gdk::Key::R) => {
                    switch_panel("run", 3);
                    glib::Propagation::Stop
                }
                (true, true, false, gdk::Key::a | gdk::Key::A) => {
                    switch_panel("agents", 4);
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
                    toggle_sidebar_k();
                    glib::Propagation::Stop
                }
                // Keyboard shortcuts help
                (true, true, false, gdk::Key::question | gdk::Key::slash) => {
                    shortcutshelp::show(&win);
                    glib::Propagation::Stop
                }
                // Command palette
                (true, true, false, gdk::Key::p | gdk::Key::P) => {
                    palette::show(&win, Rc::clone(&palette_actions_k));
                    glib::Propagation::Stop
                }
                // Toggle browser panel
                (true, true, false, gdk::Key::o | gdk::Key::O) => {
                    toggle_browser_k();
                    glib::Propagation::Stop
                }
                _ => glib::Propagation::Proceed,
            }
        });
    }

    window.add_controller(key_ctrl);

    // Open files when tree rows are activated (M10)
    {
        let store = Rc::clone(&tree_store);
        let nb_editor = notebook.clone();
        let cfg_editor = Rc::clone(&cfg);
        let agent_map_editor = Rc::clone(&agent_map);
        let on_saved_ref = Rc::clone(&on_editor_saved);
        #[allow(deprecated)]
        tree_view.connect_row_activated(move |_tv, path, _col| {
            if let Some(iter) = filetree::iter_for_path(&store, path) {
                let (file_path, is_dir) = filetree::row_info(&store, &iter);
                if !is_dir {
                    open_file_from_file_manager(
                        &file_path,
                        &nb_editor,
                        &cfg_editor.borrow(),
                        &agent_map_editor,
                        on_saved_ref.borrow().as_ref().map(Rc::clone),
                    );
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
                    let _ = tx.send_blocking(UiResult::Subtree {
                        path: file_path,
                        entries,
                    });
                });
            }
        });
    }

    // File tree context menu.
    {
        let store = Rc::clone(&tree_store);
        let tree = tree_view.clone();
        let refresh_git_c = Rc::clone(&refresh_git);
        let gesture = gtk4::GestureClick::new();
        gesture.set_button(3);
        gesture.connect_pressed(move |gesture, _n_press, x, y| {
            #[allow(deprecated)]
            let Some((Some(path), _, _, _)) = tree.path_at_pos(x as i32, y as i32) else {
                return;
            };
            let Some(iter) = filetree::iter_for_path(&store, &path) else {
                return;
            };
            let (file_path, is_dir) = filetree::row_info(&store, &iter);
            if file_path.is_empty()
                || file_path == filetree::PLACEHOLDER_PATH
                || file_path == filetree::LOADING_PATH
            {
                return;
            }

            #[allow(deprecated)]
            tree.selection().select_path(&path);
            let Some(widget) = gesture.widget() else {
                return;
            };
            show_filetree_context_menu(
                &widget,
                &store,
                x,
                y,
                file_path,
                is_dir,
                Rc::clone(&refresh_git_c),
            );
            gesture.set_state(gtk4::EventSequenceState::Claimed);
        });
        tree_view.add_controller(gesture);
    }

    // Stage all / Unstage all
    for (btn, stage) in [(stage_all_btn.clone(), true), (unstage_all_btn.clone(), false)] {
        let last_cwd_c = Rc::clone(&last_cwd);
        let refresh = Rc::clone(&refresh_git);
        btn.connect_clicked(move |btn| {
            let cwd = last_cwd_c.borrow().clone();
            if cwd.is_empty() {
                return;
            }
            let Some(root) = git::repo_root(&cwd) else {
                return;
            };
            let result = if stage {
                git::stage_all(&root)
            } else {
                git::unstage_all(&root)
            };
            match result {
                Ok(()) => refresh(),
                Err(e) => {
                    let window = btn
                        .root()
                        .and_then(|r| r.downcast::<gtk4::Window>().ok());
                    gtk4::AlertDialog::builder()
                        .message("Git operation failed")
                        .detail(&e)
                        .build()
                        .show(window.as_ref());
                }
            }
        });
    }

    // Push button
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
                let _ = tx.send_blocking(UiResult::Push { result });
            });
        });
    }

    // Pull button
    {
        let last_cwd_c = Rc::clone(&last_cwd);
        let tx = ui_tx.clone();
        pull_btn.connect_clicked(move |btn| {
            btn.set_sensitive(false);
            btn.set_label("pulling…");
            let cwd = last_cwd_c.borrow().clone();
            let tx = tx.clone();
            std::thread::spawn(move || {
                let result = git::pull(&cwd);
                let _ = tx.send_blocking(UiResult::Pull { result });
            });
        });
    }

    // Commit button
    {
        let last_cwd_c = Rc::clone(&last_cwd);
        let tx = ui_tx.clone();
        let commit_view_cb = commit_view.clone();
        commit_btn.connect_clicked(move |btn| {
            let buf = commit_view_cb.buffer();
            let message = buf
                .text(&buf.start_iter(), &buf.end_iter(), false)
                .to_string();
            if message.trim().is_empty() {
                return;
            }
            btn.set_sensitive(false);
            btn.set_label("committing…");
            let cwd = last_cwd_c.borrow().clone();
            let tx = tx.clone();
            std::thread::spawn(move || {
                let result = git::repo_root(&cwd)
                    .ok_or_else(|| "Not a git repository.".to_string())
                    .and_then(|root| git::commit(&root, &message));
                let _ = tx.send_blocking(UiResult::Commit { result });
            });
        });
    }

    for (idx, (btn, page)) in [
        (btn_files.clone(), "files"),
        (btn_git.clone(), "git"),
        (btn_search.clone(), "search"),
        (btn_run.clone(), "run"),
        (btn_agents.clone(), "agents"),
        (btn_hosts.clone(), "hosts"),
    ]
    .into_iter()
    .enumerate()
    {
        let sp = Rc::clone(&switch_panel);
        let entry_c = (page == "search").then(|| search_entry.clone());
        btn.connect_clicked(move |_| {
            sp(page, idx);
            if let Some(e) = &entry_c {
                let e = e.clone();
                glib::idle_add_local_once(move || {
                    e.grab_focus();
                });
            }
        });
    }

    // Search: run on Enter, cancel stale results with a generation counter
    {
        let last_cwd_c = Rc::clone(&last_cwd);
        let tx = ui_tx.clone();
        let gen_c = Rc::clone(&search_gen);
        let search_list_g = search_list.clone();
        search_entry.connect_activate(move |entry| {
            let query = entry.text().to_string();
            let cwd = last_cwd_c.borrow().clone();
            let repo = git::repo_root(&cwd);
            let home = std::env::var("HOME").unwrap_or_default();
            if repo.is_none() && cwd == home {
                searchpanel::show_message(
                    &search_list_g,
                    "Not in a git repository — searching your entire home directory is disabled. cd into a project first.",
                );
                return;
            }
            let root = repo.unwrap_or(cwd);
            let gen = gen_c.get() + 1;
            gen_c.set(gen);
            let tx = tx.clone();
            std::thread::spawn(move || {
                let results = searchpanel::run_search(&root, &query);
                let _ = tx.send_blocking(UiResult::SearchDone { gen, results });
            });
        });
    }

    // Dispatch IPC commands on the GTK main thread via an async task
    {
        let nb = notebook.clone();
        let cfg = Rc::clone(&cfg);
        let win_ipc = window.clone();
        let agent_map_ipc = Rc::clone(&agent_map);
        glib::MainContext::default().spawn_local(async move {
            while let Ok(req) = ipc_rx.recv().await {
                let ipc::Request { command, reply } = req;
                let resp = match command {
                    ipc::Command::Ping => ipc::Response {
                        ok: true,
                        error: None,
                        accepted: None,
                    },
                    ipc::Command::NewTab => {
                        add_tab(&nb, &cfg.borrow(), None, &agent_map_ipc);
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
                        // Reply from a detached task so other IPC traffic
                        // (agent status from other tabs) keeps flowing while
                        // the diff waits for the user's decision.
                        glib::MainContext::default().spawn_local(async move {
                            let accepted = rx.recv().await.unwrap_or(false);
                            let _ = reply.send(ipc::Response {
                                ok: true,
                                error: None,
                                accepted: Some(accepted),
                            });
                        });
                        continue;
                    }
                    ipc::Command::AgentBusy { tab } => {
                        set_agent_state(&win_ipc, &nb, &agent_map_ipc, tab, AgentState::Busy);
                        ipc::Response {
                            ok: true,
                            error: None,
                            accepted: None,
                        }
                    }
                    ipc::Command::AgentReady { tab } => {
                        set_agent_state(&win_ipc, &nb, &agent_map_ipc, tab, AgentState::Ready);
                        ipc::Response {
                            ok: true,
                            error: None,
                            accepted: None,
                        }
                    }
                    ipc::Command::AgentDone { tab } => {
                        set_agent_state(&win_ipc, &nb, &agent_map_ipc, tab, AgentState::Done);
                        ipc::Response {
                            ok: true,
                            error: None,
                            accepted: None,
                        }
                    }
                    ipc::Command::AgentIdle { tab } => {
                        set_agent_state(&win_ipc, &nb, &agent_map_ipc, tab, AgentState::Idle);
                        ipc::Response {
                            ok: true,
                            error: None,
                            accepted: None,
                        }
                    }
                };
                let _ = reply.send(resp);
            }
        });
    }

    window.present();
    {
        let sidebar = tree_sidebar.clone();
        let root = root_box.clone();
        let vis = Rc::clone(&sidebar_visible);
        let btns = all_btns.clone();
        glib::idle_add_local_once(move || {
            vis.set(false);
            sidebar.set_visible(false);
            if sidebar.parent().is_some() {
                root.remove(&sidebar);
            }
            for btn in btns {
                btn.remove_css_class("active");
            }
        });
    }
    {
        let window = window.clone();
        glib::idle_add_local_once(move || {
            allow_window_transparency(&window);
        });
    }
}

/// Everything the command palette actions need to capture; passed by
/// reference and cloned per-action inside build_palette_actions.
struct PaletteContext<'a> {
    window: &'a ApplicationWindow,
    notebook: &'a Notebook,
    cfg: &'a Rc<RefCell<config::Config>>,
    agent_map: &'a AgentMap,
    last_cwd: &'a Rc<RefCell<String>>,
    on_editor_saved: &'a Rc<RefCell<Option<SaveCallback>>>,
    switch_panel: &'a Rc<dyn Fn(&'static str, usize)>,
    search_entry: &'a gtk4::Entry,
    toggle_sidebar: &'a Rc<dyn Fn()>,
    toggle_browser: &'a Rc<dyn Fn()>,
}

fn build_palette_actions(ctx: PaletteContext<'_>) -> Rc<Vec<palette::Action>> {
    let mut actions: Vec<palette::Action> = Vec::new();

    let mut push = |title: &'static str, shortcut: Option<&'static str>, run: Rc<dyn Fn()>| {
        actions.push(palette::Action {
            title,
            shortcut,
            run,
        });
    };

    {
        let win = ctx.window.clone();
        let nb = ctx.notebook.clone();
        let cfg = Rc::clone(ctx.cfg);
        let agent_map = Rc::clone(ctx.agent_map);
        push(
            "New Tab",
            Some("Ctrl+Shift+T"),
            Rc::new(move || {
                let cwd = focused_terminal_cwd(&win, &nb);
                add_tab(&nb, &cfg.borrow(), cwd.as_deref(), &agent_map);
            }),
        );
    }
    for (title, shortcut, orientation) in [
        (
            "Split Terminal Right",
            "Ctrl+Shift+D",
            gtk4::Orientation::Horizontal,
        ),
        (
            "Split Terminal Down",
            "Ctrl+Shift+X",
            gtk4::Orientation::Vertical,
        ),
    ] {
        let win = ctx.window.clone();
        let nb = ctx.notebook.clone();
        let cfg = Rc::clone(ctx.cfg);
        let agent_map = Rc::clone(ctx.agent_map);
        push(
            title,
            Some(shortcut),
            Rc::new(move || {
                split_focused(&win, &nb, &cfg.borrow(), &agent_map, orientation);
            }),
        );
    }
    {
        let win = ctx.window.clone();
        let nb = ctx.notebook.clone();
        push(
            "Find in Scrollback",
            Some("Ctrl+Shift+H"),
            Rc::new(move || {
                if let Some(term) = focused_terminal(&win, &nb) {
                    scrollsearch::show(&win, &term);
                }
            }),
        );
    }
    {
        let win = ctx.window.clone();
        let nb = ctx.notebook.clone();
        let cfg = Rc::clone(ctx.cfg);
        let last_cwd = Rc::clone(ctx.last_cwd);
        let on_saved = Rc::clone(ctx.on_editor_saved);
        push(
            "Quick Open File",
            Some("Ctrl+F"),
            Rc::new(move || {
                let cwd = last_cwd.borrow().clone();
                if cwd.is_empty() {
                    return;
                }
                let repo = git::repo_root(&cwd);
                let home = std::env::var("HOME").unwrap_or_default();
                if repo.is_some() || cwd != home {
                    let root = repo.unwrap_or(cwd);
                    if let Some(on_saved) = on_saved.borrow().as_ref() {
                        quickopen::show(&root, &win, &nb, &cfg, Rc::clone(on_saved));
                    }
                }
            }),
        );
    }
    for (title, shortcut, page, idx) in [
        ("Show Files Panel", "Ctrl+Shift+E", "files", 0_usize),
        ("Show Git Panel", "Ctrl+Shift+G", "git", 1),
        ("Show Search Panel", "Ctrl+Shift+F", "search", 2),
        ("Show Run Panel", "Ctrl+Shift+R", "run", 3),
        ("Show Agents Panel", "Ctrl+Shift+A", "agents", 4),
    ] {
        let sp = Rc::clone(ctx.switch_panel);
        let entry = (page == "search").then(|| ctx.search_entry.clone());
        push(
            title,
            Some(shortcut),
            Rc::new(move || {
                sp(page, idx);
                if let Some(e) = &entry {
                    let e = e.clone();
                    glib::idle_add_local_once(move || {
                        e.grab_focus();
                    });
                }
            }),
        );
    }
    {
        let sp = Rc::clone(ctx.switch_panel);
        push(
            "Show Hosts Panel",
            None,
            Rc::new(move || {
                sp("hosts", 5);
            }),
        );
    }
    {
        let toggle = Rc::clone(ctx.toggle_sidebar);
        push(
            "Toggle Sidebar",
            Some("Ctrl+Shift+B"),
            Rc::new(move || toggle()),
        );
    }
    {
        let toggle = Rc::clone(ctx.toggle_browser);
        push(
            "Toggle Browser Panel",
            Some("Ctrl+Shift+O"),
            Rc::new(move || toggle()),
        );
    }
    for (title, shortcut, factor) in [
        ("Zoom In", "Ctrl+=", Some(tab::ZOOM_STEP)),
        ("Zoom Out", "Ctrl+-", Some(1.0 / tab::ZOOM_STEP)),
        ("Zoom Reset", "Ctrl+0", None),
    ] {
        let win = ctx.window.clone();
        push(
            title,
            Some(shortcut),
            Rc::new(move || {
                let zoom = match factor {
                    Some(f) => tab::set_font_zoom(tab::font_zoom() * f),
                    None => tab::set_font_zoom(1.0),
                };
                apply_font_zoom_all(&win, zoom);
            }),
        );
    }
    {
        let nb = ctx.notebook.clone();
        let cfg = Rc::clone(ctx.cfg);
        let agent_map = Rc::clone(ctx.agent_map);
        push(
            "Open Config File",
            Some("Ctrl+,"),
            Rc::new(move || {
                open_config_in_nvim(&nb, &cfg.borrow(), &agent_map);
            }),
        );
    }
    {
        let win = ctx.window.clone();
        push(
            "Keyboard Shortcuts Help",
            Some("Ctrl+Shift+?"),
            Rc::new(move || {
                shortcutshelp::show(&win);
            }),
        );
    }

    Rc::new(actions)
}

#[allow(deprecated)]
fn show_filetree_context_menu(
    parent: &gtk4::Widget,
    store: &Rc<gtk4::TreeStore>,
    x: f64,
    y: f64,
    path: String,
    is_dir: bool,
    on_refresh_git: Rc<dyn Fn()>,
) {
    let popover = gtk4::Popover::new();
    popover.set_has_arrow(false);
    popover.set_pointing_to(Some(&gtk4::gdk::Rectangle::new(x as i32, y as i32, 1, 1)));
    popover.add_css_class("context-menu");

    let vbox = gtk4::Box::new(gtk4::Orientation::Vertical, 0);

    add_filetree_menu_item(&vbox, "Open in Folder", &popover, {
        let path = path.clone();
        let parent_w = parent.clone();
        move || {
            if let Err(e) = open_path_in_folder_app(&path, is_dir) {
                show_filetree_error(&parent_w, "Open in Folder failed", &e);
            }
        }
    });

    add_filetree_menu_item(&vbox, "Move to Trash", &popover, {
        let path = path.clone();
        let parent_w = parent.clone();
        let store = Rc::clone(store);
        move || {
            confirm_move_to_trash(
                &parent_w,
                &store,
                path.clone(),
                is_dir,
                Rc::clone(&on_refresh_git),
            );
        }
    });

    popover.set_child(Some(&vbox));
    popover.set_parent(parent);
    popover.connect_closed(|p| p.unparent());
    popover.popup();
}

fn add_filetree_menu_item<F: Fn() + 'static>(
    vbox: &gtk4::Box,
    label: &str,
    popover: &gtk4::Popover,
    on_click: F,
) {
    let btn = gtk4::Button::with_label(label);
    btn.add_css_class("context-menu-item");
    let popover = popover.clone();
    btn.connect_clicked(move |_| {
        popover.popdown();
        on_click();
    });
    vbox.append(&btn);
}

fn open_path_in_folder_app(path: &str, is_dir: bool) -> Result<(), String> {
    let path = std::path::Path::new(path);
    let folder = if is_dir {
        path
    } else {
        path.parent()
            .ok_or_else(|| "File has no parent folder.".to_string())?
    };
    let uri = gio::File::for_path(folder).uri();
    gio::AppInfo::launch_default_for_uri(uri.as_str(), None::<&gio::AppLaunchContext>)
        .map_err(|e| e.to_string())
}

#[allow(deprecated)]
fn confirm_move_to_trash(
    parent: &gtk4::Widget,
    store: &Rc<gtk4::TreeStore>,
    path: String,
    is_dir: bool,
    on_refresh_git: Rc<dyn Fn()>,
) {
    let name = std::path::Path::new(&path)
        .file_name()
        .map(|n| n.to_string_lossy().to_string())
        .unwrap_or_else(|| path.clone());
    let item_kind = if is_dir { "folder" } else { "file" };
    let dialog = gtk4::AlertDialog::builder()
        .message(format!("Move {item_kind} to Trash?"))
        .detail(format!("{name}\n\n{path}"))
        .buttons(["Cancel", "Move to Trash"])
        .cancel_button(0)
        .default_button(0)
        .build();
    let window = parent
        .root()
        .and_then(|r| r.downcast::<gtk4::Window>().ok());
    let parent_w = parent.clone();
    let store = Rc::clone(store);
    dialog.choose(window.as_ref(), None::<&gio::Cancellable>, move |choice| {
        if choice != Ok(1) {
            return;
        }
        let file = gio::File::for_path(&path);
        match file.trash(None::<&gio::Cancellable>) {
            Ok(()) => {
                if let Some(iter) = filetree::find_iter_by_file_path(&store, &path) {
                    store.remove(&iter);
                }
                on_refresh_git();
            }
            Err(e) => show_filetree_error(&parent_w, "Move to Trash failed", &e.to_string()),
        }
    });
}

fn show_filetree_error(parent: &gtk4::Widget, message: &str, detail: &str) {
    let window = parent
        .root()
        .and_then(|r| r.downcast::<gtk4::Window>().ok());
    gtk4::AlertDialog::builder()
        .message(message)
        .detail(detail)
        .build()
        .show(window.as_ref());
}

fn add_tab(
    notebook: &Notebook,
    cfg: &config::Config,
    cwd: Option<&str>,
    agent_map: &AgentMap,
) {
    add_tab_with_command(notebook, cfg, cwd, agent_map, None);
}

/// Spawn the user's shell in `terminal`, exporting SIDEKICK_TAB_ID so hooks
/// and sidekick-ctl can address this terminal. On failure `pid_cell` (if any)
/// is set to -1 so pollers can stop.
fn spawn_shell(
    terminal: &vte4::Terminal,
    cwd: Option<&str>,
    tab_id: u64,
    startup_command: Option<String>,
    pid_cell: Option<Rc<Cell<i32>>>,
) {
    let shell = std::env::var("SHELL").unwrap_or_else(|_| "/bin/bash".to_string());
    let tab_env = format!("SIDEKICK_TAB_ID={tab_id}");
    let envv = ["PROMPT_SP=", "PROMPT_CR=", tab_env.as_str()];
    let term_for_spawn = terminal.clone();

    terminal.spawn_async(
        vte4::PtyFlags::DEFAULT,
        cwd,
        &[shell.as_str()],
        &envv,
        glib::SpawnFlags::DEFAULT,
        || {},
        -1,
        None::<&gio::Cancellable>,
        move |result| match result {
            Ok(pid) => {
                if let Some(cell) = &pid_cell {
                    cell.set(pid.0);
                }
                if let Some(command) = &startup_command {
                    let mut bytes = command.as_bytes().to_vec();
                    bytes.push(b'\n');
                    term_for_spawn.feed_child(&bytes);
                }
            }
            Err(e) => {
                eprintln!("sidekick: shell spawn failed: {e}");
                if let Some(cell) = &pid_cell {
                    cell.set(-1);
                }
            }
        },
    );
}

fn add_tab_with_command(
    notebook: &Notebook,
    cfg: &config::Config,
    cwd: Option<&str>,
    agent_map: &AgentMap,
    startup_command: Option<String>,
) {
    let terminal = tab::build(cfg);
    let page_idx = notebook.n_pages();

    // Register per-terminal agent state
    let agent_state: AgentCell = Rc::new(Cell::new(AgentState::Idle));
    let tab_id = NEXT_TAB_ID.fetch_add(1, Ordering::Relaxed);
    let agent_key = terminal.as_ptr() as usize;
    agent_map
        .borrow_mut()
        .insert(agent_key, (tab_id, Rc::clone(&agent_state)));

    let (tab_label, tab_dot, tab_title, tab_detail) = build_terminal_tab_label();
    notebook.append_page(&terminal, Some(&tab_label));
    notebook.set_tab_reorderable(&terminal, true);
    notebook.set_current_page(Some(page_idx));

    // Right-click the tab label to rename it; a custom name overrides the
    // automatic cwd-based title until reset.
    let custom_title: Rc<RefCell<Option<String>>> = Rc::new(RefCell::new(None));
    {
        let custom = Rc::clone(&custom_title);
        let label_w: gtk4::Widget = tab_label.clone().upcast();
        let gesture = gtk4::GestureClick::new();
        gesture.set_button(3);
        gesture.connect_pressed(move |gesture, _n, x, y| {
            show_tab_context_menu(&label_w, x, y, Rc::clone(&custom));
            gesture.set_state(gtk4::EventSequenceState::Claimed);
        });
        tab_label.add_controller(gesture);
    }
    let t = terminal.clone();
    glib::idle_add_local(move || {
        t.grab_focus();
        glib::ControlFlow::Break
    });

    let pid_cell: Rc<Cell<i32>> = Rc::new(Cell::new(0));
    spawn_shell(
        &terminal,
        cwd,
        tab_id,
        startup_command,
        Some(Rc::clone(&pid_cell)),
    );

    // Notification ring: mark tab dirty when shell returns to prompt in a background tab.
    // Requires shell integration to emit VTE termprops (see shell-integration.zsh).
    let dirty: Rc<Cell<bool>> = Rc::new(Cell::new(false));
    let last_terminal_change: Rc<RefCell<Instant>> = Rc::new(RefCell::new(Instant::now()));
    let last_user_input: Rc<RefCell<Instant>> =
        Rc::new(RefCell::new(Instant::now() - Duration::from_secs(10)));
    let running_frame: Rc<Cell<usize>> = Rc::new(Cell::new(0));
    wire_agent_state_handlers(
        &terminal,
        &agent_state,
        Some((Rc::clone(&dirty), notebook.clone())),
    );
    {
        let agent_c = Rc::clone(&agent_state);
        let last_user_input_c = Rc::clone(&last_user_input);
        let key_ctrl = gtk4::EventControllerKey::new();
        key_ctrl.connect_key_pressed(move |_, key, _, _| {
            *last_user_input_c.borrow_mut() = Instant::now();
            if matches!(key, gdk::Key::Return | gdk::Key::KP_Enter)
                && matches!(agent_c.get(), AgentState::Ready)
            {
                agent_c.set(AgentState::AutoBusy);
            }
            glib::Propagation::Proceed
        });
        terminal.add_controller(key_ctrl);
    }
    {
        let last_terminal_change_c = Rc::clone(&last_terminal_change);
        let last_user_input_c = Rc::clone(&last_user_input);
        let agent_c = Rc::clone(&agent_state);
        terminal.connect_contents_changed(move |_| {
            *last_terminal_change_c.borrow_mut() = Instant::now();
            if matches!(agent_c.get(), AgentState::Ready)
                && last_user_input_c.borrow().elapsed() >= Duration::from_millis(500)
            {
                agent_c.set(AgentState::AutoBusy);
            }
        });
    }

    // Poll cwd + git branch; show agent/dirty indicator.
    {
        let tab_dot_ref = tab_dot.clone();
        let tab_title_ref = tab_title.clone();
        let tab_detail_ref = tab_detail.clone();
        let tab_label_ref = tab_label.clone();
        let pid_ref = Rc::clone(&pid_cell);
        let dirty_ref = Rc::clone(&dirty);
        let agent_ref = Rc::clone(&agent_state);
        let last_terminal_change_ref = Rc::clone(&last_terminal_change);
        let running_frame_ref = Rc::clone(&running_frame);
        let nb_ref = notebook.clone();
        let term_ref = terminal.clone();
        let agent_map_ref = Rc::clone(agent_map);
        let prev_state_ref: Rc<Cell<AgentState>> = Rc::new(Cell::new(AgentState::Idle));
        let custom_title_ref = Rc::clone(&custom_title);
        glib::timeout_add_local(Duration::from_millis(500), move || {
            let pid = pid_ref.get();
            if pid < 0 {
                // Spawn failed — stop polling so the timer (and its strong
                // widget refs) does not live forever.
                agent_map_ref.borrow_mut().remove(&agent_key);
                return glib::ControlFlow::Break;
            }
            if pid == 0 {
                return glib::ControlFlow::Continue;
            }
            if !std::path::Path::new(&format!("/proc/{}", pid)).exists() {
                // Shell is gone; child_exited normally cleans up, but cover
                // close paths where the widget was destroyed first.
                agent_map_ref.borrow_mut().remove(&agent_key);
                return glib::ControlFlow::Break;
            }

            let tw: gtk4::Widget = term_ref.clone().upcast();
            if let Some(page) = notebook_page_of(&tw, &nb_ref) {
                if nb_ref.page_num(&page) == nb_ref.current_page() {
                    dirty_ref.set(false);
                }
            }

            // Auto-detect: non-agent foreground commands are running. Known
            // agent TUIs stay in the foreground even while idle, so their
            // busy state comes from output changes or explicit status hooks.
            let agent_running = terminal_has_foreground_process(&term_ref, pid);
            let known_agent_running =
                agent_running && terminal_has_known_agent_foreground_process(&term_ref, pid);
            match (agent_running, known_agent_running, agent_ref.get()) {
                (true, false, AgentState::Idle) => agent_ref.set(AgentState::AutoBusy),
                (true, true, AgentState::Idle) => agent_ref.set(AgentState::Ready),
                (false, _, AgentState::AutoBusy) => agent_ref.set(AgentState::Idle),
                _ => {}
            }
            if matches!(agent_ref.get(), AgentState::AutoBusy)
                && last_terminal_change_ref.borrow().elapsed() >= Duration::from_secs(2)
                && known_agent_running
            {
                agent_ref.set(AgentState::Ready);
            }

            let (auto_title, detail_text) = tab::tab_title_parts(pid);
            let title_text = custom_title_ref
                .borrow()
                .clone()
                .unwrap_or(auto_title);
            let escaped_title = glib::markup_escape_text(&title_text);
            let state = agent_ref.get();

            // Desktop notification when an agent needs attention while the
            // window is unfocused. WAIT always notifies; DONE only when the
            // busy state came from an explicit agent hook (not every command).
            let prev = prev_state_ref.get();
            if state != prev {
                prev_state_ref.set(state);
                let needs_attention = matches!(state, AgentState::Ready)
                    || (matches!(state, AgentState::Done) && matches!(prev, AgentState::Busy));
                if needs_attention {
                    notify_agent_attention(&nb_ref, agent_key, state, &title_text, &detail_text);
                }
            }
            let status_label = if matches!(state, AgentState::Idle) && dirty_ref.get() {
                "NEW"
            } else {
                state.label()
            };
            tab_label_ref
                .set_tooltip_text(Some(&format!("Status: {}\n{}", status_label, detail_text)));

            match state {
                AgentState::AutoBusy | AgentState::Busy => {
                    const SPINNER_FRAMES: [&str; 10] =
                        ["⠋", "⠙", "⠹", "⠸", "⠼", "⠴", "⠦", "⠧", "⠇", "⠏"];
                    let frame = running_frame_ref.get();
                    running_frame_ref.set((frame + 1) % SPINNER_FRAMES.len());
                    set_terminal_tab_label(
                        &tab_dot_ref,
                        &tab_title_ref,
                        &tab_detail_ref,
                        "#f9e2af",
                        SPINNER_FRAMES[frame],
                        &escaped_title,
                        &detail_text,
                    );
                }
                AgentState::Ready => set_terminal_tab_label(
                    &tab_dot_ref,
                    &tab_title_ref,
                    &tab_detail_ref,
                    "#a6e3a1",
                    "●",
                    &escaped_title,
                    &detail_text,
                ),
                AgentState::Done => set_terminal_tab_label(
                    &tab_dot_ref,
                    &tab_title_ref,
                    &tab_detail_ref,
                    "#89b4fa",
                    "●",
                    &escaped_title,
                    &detail_text,
                ),
                AgentState::Idle => {
                    if dirty_ref.get() {
                        set_terminal_tab_label(
                            &tab_dot_ref,
                            &tab_title_ref,
                            &tab_detail_ref,
                            "#f38ba8",
                            "●",
                            &escaped_title,
                            &detail_text,
                        );
                    } else {
                        set_terminal_tab_label(
                            &tab_dot_ref,
                            &tab_title_ref,
                            &tab_detail_ref,
                            "#6c7086",
                            "●",
                            &escaped_title,
                            &detail_text,
                        );
                    }
                }
            }
            glib::ControlFlow::Continue
        });
    }

    let nb = notebook.clone();
    let weak = terminal.downgrade();
    let agent_map_close = Rc::clone(agent_map);
    terminal.connect_child_exited(move |_, _| {
        agent_map_close.borrow_mut().remove(&agent_key);
        if let Some(t) = weak.upgrade() {
            if pane::close_terminal(&t, &nb) {
                std::process::exit(0);
            }
        }
    });
}

fn open_config_in_nvim(notebook: &Notebook, cfg: &config::Config, agent_map: &AgentMap) {
    let path = config::config_path();
    open_path_in_nvim(notebook, cfg, agent_map, &path);
}

fn open_file_from_file_manager(
    path: &str,
    notebook: &Notebook,
    cfg: &config::Config,
    agent_map: &AgentMap,
    on_saved: Option<SaveCallback>,
) {
    match cfg.editor.file_manager_open.as_str() {
        "nvim" | "vim" | "neovim" => {
            open_path_in_nvim(notebook, cfg, agent_map, std::path::Path::new(path));
        }
        _ => {
            editor::open_with_save_callback(path, notebook, cfg, on_saved);
        }
    }
}

fn open_path_in_nvim(
    notebook: &Notebook,
    cfg: &config::Config,
    agent_map: &AgentMap,
    path: &std::path::Path,
) {
    let cwd = path.parent().and_then(|p| p.to_str());
    let command = format!("nvim {}", shell_quote_path(path));
    add_tab_with_command(notebook, cfg, cwd, agent_map, Some(command));
}

fn focused_terminal(window: &ApplicationWindow, notebook: &Notebook) -> Option<vte4::Terminal> {
    if let Some(term) =
        gtk4::prelude::GtkWindowExt::focus(window).and_then(|w| w.downcast::<vte4::Terminal>().ok())
    {
        let term_w: gtk4::Widget = term.clone().upcast();
        if let Some(page) = notebook_page_of(&term_w, notebook) {
            if notebook.page_num(&page) == notebook.current_page() {
                return Some(term);
            }
        }
    }
    let page = notebook.nth_page(Some(notebook.current_page()?))?;
    pane::collect_terminals_pub(&page).into_iter().next()
}

fn focused_terminal_cwd(window: &ApplicationWindow, notebook: &Notebook) -> Option<String> {
    focused_terminal(window, notebook).and_then(|t| terminal_cwd(&t))
}

/// True when the clipboard advertises image content. Checked synchronously
/// from the key handler so plain text Ctrl+V can fall through to the shell.
fn clipboard_has_image(terminal: &vte4::Terminal) -> bool {
    let formats = terminal.clipboard().formats();
    formats.contains_type(gdk::Texture::static_type())
        || formats
            .mime_types()
            .iter()
            .any(|m| m.starts_with("image/"))
}

fn paste_clipboard_image(terminal: &vte4::Terminal) {
    let clipboard = terminal.clipboard();
    let term = terminal.clone();
    clipboard.read_texture_async(None::<&gio::Cancellable>, move |result| match result {
        Ok(Some(texture)) => match save_clipboard_texture(&texture) {
            Ok(path) => {
                let text = format!("{} ", path.display());
                term.feed_child(text.as_bytes());
            }
            Err(err) => {
                eprintln!("sidekick: failed to save clipboard image: {err}");
                term.paste_clipboard();
            }
        },
        Ok(None) => term.paste_clipboard(),
        Err(err) => {
            eprintln!("sidekick: failed to read clipboard image: {err}");
            term.paste_clipboard();
        }
    });
}

/// Apply the current font zoom to every terminal in the window.
fn apply_font_zoom_all(window: &ApplicationWindow, zoom: f64) {
    fn walk(widget: &gtk4::Widget, zoom: f64) {
        if let Ok(term) = widget.clone().downcast::<vte4::Terminal>() {
            term.set_font_scale(zoom);
        }
        let mut child = widget.first_child();
        while let Some(w) = child {
            let next = w.next_sibling();
            walk(&w, zoom);
            child = next;
        }
    }
    if let Some(child) = window.child() {
        walk(&child.upcast(), zoom);
    }
}

fn save_clipboard_texture(texture: &gdk::Texture) -> Result<std::path::PathBuf, String> {
    use std::os::unix::fs::PermissionsExt;

    // XDG_RUNTIME_DIR is per-user and 0700. Never write predictable filenames
    // into shared /tmp, where another local user could pre-plant a symlink.
    let dir = std::env::var("XDG_RUNTIME_DIR")
        .map(std::path::PathBuf::from)
        .unwrap_or_else(|_| std::env::temp_dir())
        .join("sidekick");
    std::fs::create_dir_all(&dir).map_err(|e| e.to_string())?;
    let _ = std::fs::set_permissions(&dir, std::fs::Permissions::from_mode(0o700));
    prune_old_clipboard_images(&dir);

    let mut path = dir;
    path.push(format!(
        "clipboard-{}-{}.png",
        std::process::id(),
        glib::monotonic_time()
    ));
    texture.save_to_png(&path).map_err(|e| e.to_string())?;
    Ok(path)
}

/// Best-effort cleanup so pasted images don't accumulate forever.
fn prune_old_clipboard_images(dir: &std::path::Path) {
    const MAX_AGE: Duration = Duration::from_secs(24 * 60 * 60);
    let Ok(entries) = std::fs::read_dir(dir) else {
        return;
    };
    let now = std::time::SystemTime::now();
    for entry in entries.flatten() {
        let expired = entry
            .metadata()
            .and_then(|m| m.modified())
            .ok()
            .and_then(|t| now.duration_since(t).ok())
            .map(|age| age > MAX_AGE)
            .unwrap_or(false);
        if expired {
            let _ = std::fs::remove_file(entry.path());
        }
    }
}

fn focused_window_terminal_on_current_page(
    window: &ApplicationWindow,
    notebook: &Notebook,
) -> Option<vte4::Terminal> {
    let term = gtk4::prelude::GtkWindowExt::focus(window)
        .and_then(|w| w.downcast::<vte4::Terminal>().ok())?;
    let term_w: gtk4::Widget = term.clone().upcast();
    let page = notebook_page_of(&term_w, notebook)?;
    (notebook.page_num(&page) == notebook.current_page()).then_some(term)
}

fn current_page_is_final_terminal_tab(notebook: &Notebook) -> bool {
    notebook.n_pages() <= 1
        && notebook
            .nth_page(Some(notebook.current_page().unwrap_or(0)))
            .map(|page| page.is::<vte4::Terminal>())
            .unwrap_or(false)
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

/// Read the title text out of a terminal tab's label widget
/// (built by build_terminal_tab_label: [dot, [title, detail]]).
fn tab_label_title(notebook: &Notebook, page: &gtk4::Widget) -> Option<String> {
    let label_box = notebook.tab_label(page)?;
    let text_box = label_box.first_child()?.next_sibling()?;
    let title = text_box.first_child()?.downcast::<gtk4::Label>().ok()?;
    Some(title.text().to_string())
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

/// Capture the current tabs (terminal layout + cwds) for session restore.
/// Editor/diff tabs are not captured.
fn snapshot_session(notebook: &Notebook) -> session::Session {
    let mut tabs = Vec::new();
    for i in 0..notebook.n_pages() {
        if let Some(page) = notebook.nth_page(Some(i)) {
            if let Some(node) = capture_layout(&page) {
                tabs.push(node);
            }
        }
    }
    session::Session { tabs }
}

fn capture_layout(widget: &gtk4::Widget) -> Option<session::Node> {
    if let Ok(term) = widget.clone().downcast::<vte4::Terminal>() {
        let cwd = terminal_cwd(&term)
            .or_else(|| std::env::var("HOME").ok())
            .unwrap_or_else(|| "/".to_string());
        return Some(session::Node::Terminal { cwd });
    }
    if let Ok(paned) = widget.clone().downcast::<gtk4::Paned>() {
        let first = paned.start_child().and_then(|w| capture_layout(&w));
        let second = paned.end_child().and_then(|w| capture_layout(&w));
        return match (first, second) {
            (Some(a), Some(b)) => Some(session::Node::Split {
                orientation: if paned.orientation() == gtk4::Orientation::Vertical {
                    "v".to_string()
                } else {
                    "h".to_string()
                },
                first: Box::new(a),
                second: Box::new(b),
            }),
            (Some(only), None) | (None, Some(only)) => Some(only),
            (None, None) => None,
        };
    }
    None
}

/// Recreate tabs from the saved session. Returns false when there was
/// nothing usable to restore.
fn restore_session(notebook: &Notebook, cfg: &config::Config, agent_map: &AgentMap) -> bool {
    let Some(saved) = session::load() else {
        return false;
    };
    if saved.tabs.is_empty() {
        return false;
    }
    for node in &saved.tabs {
        add_tab(notebook, cfg, Some(node.first_cwd()), agent_map);
        // The tab's root terminal is the page we just appended.
        let Some(term) = notebook
            .nth_page(Some(notebook.n_pages() - 1))
            .and_then(|p| p.downcast::<vte4::Terminal>().ok())
        else {
            continue;
        };
        expand_layout(notebook, cfg, agent_map, &term, node);
    }
    notebook.set_current_page(Some(0));
    true
}

/// Recursively recreate a saved split layout. `term` currently occupies the
/// slot described by `node`.
fn expand_layout(
    notebook: &Notebook,
    cfg: &config::Config,
    agent_map: &AgentMap,
    term: &vte4::Terminal,
    node: &session::Node,
) {
    let session::Node::Split {
        orientation,
        first,
        second,
    } = node
    else {
        return;
    };
    let orient = if orientation == "v" {
        gtk4::Orientation::Vertical
    } else {
        gtk4::Orientation::Horizontal
    };
    let new_term = split_terminal(
        notebook,
        cfg,
        agent_map,
        term,
        orient,
        Some(second.first_cwd()),
        None,
        None,
    );
    expand_layout(notebook, cfg, agent_map, term, first);
    expand_layout(notebook, cfg, agent_map, &new_term, second);
}

fn show_tab_context_menu(
    parent: &gtk4::Widget,
    x: f64,
    y: f64,
    custom_title: Rc<RefCell<Option<String>>>,
) {
    let popover = gtk4::Popover::new();
    popover.set_has_arrow(false);
    popover.set_pointing_to(Some(&gtk4::gdk::Rectangle::new(x as i32, y as i32, 1, 1)));
    popover.add_css_class("context-menu");

    let vbox = gtk4::Box::new(gtk4::Orientation::Vertical, 0);

    add_filetree_menu_item(&vbox, "Rename tab…", &popover, {
        let parent_w = parent.clone();
        let custom = Rc::clone(&custom_title);
        move || prompt_tab_rename(&parent_w, Rc::clone(&custom))
    });
    if custom_title.borrow().is_some() {
        add_filetree_menu_item(&vbox, "Reset name", &popover, {
            let custom = Rc::clone(&custom_title);
            move || {
                *custom.borrow_mut() = None;
            }
        });
    }

    popover.set_child(Some(&vbox));
    popover.set_parent(parent);
    popover.connect_closed(|p| p.unparent());
    popover.popup();
}

fn prompt_tab_rename(parent: &gtk4::Widget, custom_title: Rc<RefCell<Option<String>>>) {
    let parent_window = parent
        .root()
        .and_then(|r| r.downcast::<gtk4::Window>().ok());

    let win = gtk4::Window::new();
    win.set_transient_for(parent_window.as_ref());
    win.set_modal(true);
    win.set_decorated(false);
    win.set_resizable(false);
    win.set_default_width(300);
    win.add_css_class("quickopen-window");

    let entry = gtk4::Entry::new();
    entry.set_placeholder_text(Some("Tab name (empty resets)…"));
    entry.add_css_class("quickopen-entry");
    entry.set_margin_top(8);
    entry.set_margin_bottom(8);
    entry.set_margin_start(8);
    entry.set_margin_end(8);
    if let Some(current) = custom_title.borrow().as_ref() {
        entry.set_text(current);
    }
    win.set_child(Some(&entry));

    {
        let win_c = win.clone();
        entry.connect_activate(move |e| {
            let text = e.text().trim().to_string();
            *custom_title.borrow_mut() = if text.is_empty() { None } else { Some(text) };
            win_c.close();
        });
    }
    {
        let win_c = win.clone();
        let key = gtk4::EventControllerKey::new();
        key.connect_key_pressed(move |_, keyval, _, _| {
            if keyval == gdk::Key::Escape {
                win_c.close();
                return glib::Propagation::Stop;
            }
            glib::Propagation::Proceed
        });
        win.add_controller(key);
    }

    win.present();
    entry.grab_focus();
}

/// Send a desktop notification for an agent state change, but only when the
/// sidekick window is not focused (the tab dot covers the focused case).
fn notify_agent_attention(
    notebook: &Notebook,
    key: usize,
    state: AgentState,
    title: &str,
    detail: &str,
) {
    let Some(window) = notebook
        .root()
        .and_then(|r| r.downcast::<gtk4::Window>().ok())
    else {
        return;
    };
    if window.is_active() {
        return;
    }
    let Some(app) = window.application() else {
        return;
    };
    let summary = match state {
        AgentState::Ready => "Agent waiting for input",
        AgentState::Done => "Agent finished",
        _ => return,
    };
    let notification = gio::Notification::new(summary);
    notification.set_body(Some(&format!("{title} — {detail}")));
    // One notification id per terminal so updates replace instead of stack.
    app.send_notification(Some(&format!("sidekick-agent-{key}")), &notification);
}

/// Desktop notification when a long-running command finishes while the
/// window is unfocused. Exit code comes from the shell-integration termprop;
/// without it the notification still fires, just without a failure marker.
fn notify_long_command_finished(terminal: &vte4::Terminal, duration: Duration) {
    let Some(window) = terminal
        .root()
        .and_then(|r| r.downcast::<gtk4::Window>().ok())
    else {
        return;
    };
    if window.is_active() {
        return;
    }
    let Some(app) = window.application() else {
        return;
    };
    let (exit_value, _) = terminal.termprop_string(CMD_EXIT_TERMPROP);
    let exit_code = exit_value.as_ref().and_then(|v| v.as_str().parse::<i32>().ok());
    let summary = match exit_code {
        Some(code) if code != 0 => format!("Command failed (exit {code})"),
        _ => "Command finished".to_string(),
    };
    let place = terminal_cwd(terminal)
        .map(|cwd| {
            std::path::Path::new(&cwd)
                .file_name()
                .map(|n| n.to_string_lossy().to_string())
                .unwrap_or(cwd)
        })
        .unwrap_or_default();
    let notification = gio::Notification::new(&summary);
    notification.set_body(Some(&format!(
        "{} — {place}",
        agentpanel::format_elapsed(duration.as_secs())
    )));
    // One notification id per terminal so updates replace instead of stack.
    let key = terminal.as_ptr() as usize;
    app.send_notification(Some(&format!("sidekick-cmd-{key}")), &notification);
}

fn set_agent_state(
    window: &ApplicationWindow,
    notebook: &Notebook,
    agent_map: &AgentMap,
    tab: Option<u64>,
    state: AgentState,
) {
    // Tab-addressed updates land exactly on the terminal the sender ran in.
    if let Some(id) = tab {
        if let Some((_, agent_state)) = agent_map.borrow().values().find(|(tid, _)| *tid == id) {
            agent_state.set(state);
        }
        // Unknown id: that terminal is gone — drop the update instead of
        // guessing at another tab.
        return;
    }
    // Legacy senders without a tab id: best effort on the focused terminal.
    if let Some(term) = focused_terminal(window, notebook) {
        let key = term.as_ptr() as usize;
        if let Some((_, agent_state)) = agent_map.borrow().get(&key) {
            agent_state.set(state);
        }
    }
}

/// Connect the termprop handlers that drive a terminal's agent state:
/// shell precmd/preexec transitions and explicit OSC status updates.
/// `dirty_ctx` additionally marks the tab's notification dot when a command
/// finishes while the tab is in the background.
fn wire_agent_state_handlers(
    terminal: &vte4::Terminal,
    agent_state: &AgentCell,
    dirty_ctx: Option<(Rc<Cell<bool>>, Notebook)>,
) {
    // When the last command started, for long-command notifications.
    let command_started: Rc<Cell<Option<Instant>>> = Rc::new(Cell::new(None));
    {
        let agent_c = Rc::clone(agent_state);
        let term_c = terminal.clone();
        let started_c = Rc::clone(&command_started);
        terminal.connect_termprop_changed(Some("vte.shell.precmd"), move |_, _| {
            // Explicit agent hooks already notify on Busy -> Done; skip the
            // long-command notification for those to avoid doubling up.
            let was_explicit_busy = matches!(agent_c.get(), AgentState::Busy);
            if matches!(agent_c.get(), AgentState::AutoBusy | AgentState::Busy) {
                agent_c.set(AgentState::Done);
            }
            if let Some(started) = started_c.take() {
                let duration = started.elapsed();
                if !was_explicit_busy && duration.as_secs() >= LONG_COMMAND_NOTIFY_SECS {
                    notify_long_command_finished(&term_c, duration);
                }
            }
            if let Some((dirty, nb)) = &dirty_ctx {
                if dirty.get() {
                    return;
                }
                let tw: gtk4::Widget = term_c.clone().upcast();
                if let Some(page) = notebook_page_of(&tw, nb) {
                    if nb.page_num(&page) != nb.current_page() {
                        dirty.set(true);
                    }
                }
            }
        });
    }
    {
        let agent_c = Rc::clone(agent_state);
        let started_c = Rc::clone(&command_started);
        terminal.connect_termprop_changed(Some("vte.shell.preexec"), move |_, _| {
            started_c.set(Some(Instant::now()));
            if matches!(
                agent_c.get(),
                AgentState::Idle | AgentState::Ready | AgentState::Done
            ) {
                agent_c.set(AgentState::AutoBusy);
            }
        });
    }
    {
        let agent_c = Rc::clone(agent_state);
        terminal.connect_termprop_changed(Some(AGENT_STATUS_TERMPROP), move |term, _| {
            let (value, _) = term.termprop_string(AGENT_STATUS_TERMPROP);
            if let Some(state) = value
                .as_ref()
                .and_then(|value| agent_state_from_status(value.as_str()))
            {
                agent_c.set(state);
            }
        });
    }
}

/// Split the focused terminal, giving the new pane the same first-class
/// agent wiring as a tab terminal: it shares the tab's agent state (any pane
/// can drive the tab dot), gets its own SIDEKICK_TAB_ID-addressable entry,
/// and inherits the focused pane's working directory.
fn split_focused(
    window: &ApplicationWindow,
    notebook: &Notebook,
    cfg: &config::Config,
    agent_map: &AgentMap,
    orientation: gtk4::Orientation,
) {
    let Some(focused) = pane::split_target(window, notebook) else {
        return;
    };
    let cwd = terminal_cwd(&focused);
    let new_term = split_terminal(
        notebook,
        cfg,
        agent_map,
        &focused,
        orientation,
        cwd.as_deref(),
        None,
        None,
    );
    new_term.grab_focus();
}

/// Split `target` and return the new, fully wired terminal: it shares the
/// tab's agent state (any pane can drive the tab dot), gets its own
/// SIDEKICK_TAB_ID-addressable entry, and closes cleanly on shell exit.
#[allow(clippy::too_many_arguments)]
fn split_terminal(
    notebook: &Notebook,
    cfg: &config::Config,
    agent_map: &AgentMap,
    target: &vte4::Terminal,
    orientation: gtk4::Orientation,
    cwd: Option<&str>,
    startup_command: Option<String>,
    pid_cell: Option<Rc<Cell<i32>>>,
) -> vte4::Terminal {
    let target_key = target.as_ptr() as usize;
    let (tab_id, agent_state) = match agent_map.borrow().get(&target_key) {
        Some((id, state)) => (*id, Rc::clone(state)),
        None => (
            NEXT_TAB_ID.fetch_add(1, Ordering::Relaxed),
            Rc::new(Cell::new(AgentState::Idle)),
        ),
    };

    let new_term = tab::build(cfg);
    let new_key = new_term.as_ptr() as usize;
    agent_map
        .borrow_mut()
        .insert(new_key, (tab_id, Rc::clone(&agent_state)));
    wire_agent_state_handlers(&new_term, &agent_state, None);

    spawn_shell(&new_term, cwd, tab_id, startup_command, pid_cell);

    {
        let nb = notebook.clone();
        let weak = new_term.downgrade();
        let agent_map_c = Rc::clone(agent_map);
        new_term.connect_child_exited(move |_, _| {
            agent_map_c.borrow_mut().remove(&new_key);
            if let Some(t) = weak.upgrade() {
                if pane::close_terminal(&t, &nb) {
                    // All shells exited deliberately — start fresh next launch.
                    session::save(&session::Session::default());
                    std::process::exit(0);
                }
            }
        });
    }

    pane::split_with(notebook, target, &new_term, orientation);
    new_term
}

/// Poll the foreground process of a task split and flip the run-panel status
/// label from "running" to "done" when the command finishes.
fn track_task_status(terminal: &vte4::Terminal, pid_cell: Rc<Cell<i32>>, label: &gtk4::Label) {
    label.set_markup("<span foreground=\"#f9e2af\">●</span>");
    label.set_tooltip_text(Some("running"));
    let term_weak = terminal.downgrade();
    let label_weak = label.downgrade();
    let started = Cell::new(false);
    let begun = Instant::now();
    glib::timeout_add_local(Duration::from_millis(1000), move || {
        let (Some(term), Some(label)) = (term_weak.upgrade(), label_weak.upgrade()) else {
            return glib::ControlFlow::Break;
        };
        let pid = pid_cell.get();
        if pid < 0 {
            label.set_text("");
            return glib::ControlFlow::Break;
        }
        if pid == 0 {
            return glib::ControlFlow::Continue;
        }
        let running = terminal_has_foreground_process(&term, pid);
        if running {
            started.set(true);
            glib::ControlFlow::Continue
        } else if started.get() || begun.elapsed() > Duration::from_secs(3) {
            // Either the command ended, or it finished faster than our poll.
            label.set_markup("<span foreground=\"#a6e3a1\">✓</span>");
            label.set_tooltip_text(Some("finished"));
            glib::ControlFlow::Break
        } else {
            glib::ControlFlow::Continue
        }
    });
}

/// Returns true if the terminal has a foreground process other than the shell itself.
fn terminal_has_foreground_process(terminal: &vte4::Terminal, shell_pid: i32) -> bool {
    let Some(foreground_pgid) = terminal_foreground_pgid(terminal) else {
        return false;
    };
    let shell_pgid = unsafe { libc::getpgid(shell_pid) };
    shell_pgid > 0 && foreground_pgid != shell_pgid
}

fn terminal_has_known_agent_foreground_process(terminal: &vte4::Terminal, shell_pid: i32) -> bool {
    let Some(foreground_pgid) = terminal_foreground_pgid(terminal) else {
        return false;
    };
    let shell_pgid = unsafe { libc::getpgid(shell_pid) };
    if shell_pgid <= 0 || foreground_pgid == shell_pgid {
        return false;
    }

    foreground_process_command(foreground_pgid)
        .map(|command| is_known_agent_command(&command))
        .unwrap_or(false)
}

fn terminal_foreground_pgid(terminal: &vte4::Terminal) -> Option<i32> {
    use std::os::unix::io::AsRawFd;
    let pty = terminal.pty()?;
    let foreground_pgid = unsafe { libc::tcgetpgrp(pty.fd().as_raw_fd()) };
    if foreground_pgid <= 0 {
        return None;
    }
    Some(foreground_pgid)
}

fn foreground_process_command(pid: i32) -> Option<String> {
    let cmdline = std::fs::read(format!("/proc/{pid}/cmdline")).ok()?;
    if !cmdline.is_empty() {
        let command = cmdline
            .split(|byte| *byte == 0)
            .filter(|part| !part.is_empty())
            .map(|part| String::from_utf8_lossy(part))
            .collect::<Vec<_>>()
            .join(" ");
        if !command.is_empty() {
            return Some(command);
        }
    }

    std::fs::read_to_string(format!("/proc/{pid}/comm"))
        .ok()
        .map(|command| command.trim().to_string())
        .filter(|command| !command.is_empty())
}

fn is_known_agent_command(command: &str) -> bool {
    let command = command.to_ascii_lowercase();
    command.contains("claude") || command.contains("codex")
}

fn build_terminal_tab_label() -> (gtk4::Box, gtk4::Label, gtk4::Label, gtk4::Label) {
    let row = gtk4::Box::new(gtk4::Orientation::Horizontal, 8);
    row.add_css_class("session-tab");
    row.set_size_request(SESSION_TAB_WIDTH, -1);

    let dot = gtk4::Label::new(None);
    dot.add_css_class("session-tab-dot");
    dot.set_markup("<span foreground=\"#6c7086\">●</span>");
    dot.set_valign(gtk4::Align::Start);
    dot.set_margin_top(2);

    let text = gtk4::Box::new(gtk4::Orientation::Vertical, 2);
    text.set_hexpand(true);
    text.set_size_request(SESSION_TAB_WIDTH - 28, -1);

    let title = gtk4::Label::new(Some("~"));
    title.add_css_class("session-tab-title");
    title.set_xalign(0.0);
    title.set_ellipsize(pango::EllipsizeMode::End);
    title.set_width_chars(1);
    title.set_max_width_chars(24);
    title.set_hexpand(true);

    let detail = gtk4::Label::new(Some("~"));
    detail.add_css_class("session-tab-detail");
    detail.set_xalign(0.0);
    detail.set_ellipsize(pango::EllipsizeMode::End);
    detail.set_width_chars(1);
    detail.set_max_width_chars(28);
    detail.set_hexpand(true);

    text.append(&title);
    text.append(&detail);
    row.append(&dot);
    row.append(&text);

    (row, dot, title, detail)
}

fn set_terminal_tab_label(
    dot: &gtk4::Label,
    title: &gtk4::Label,
    detail: &gtk4::Label,
    color: &str,
    marker: &str,
    escaped_title: &str,
    detail_text: &str,
) {
    dot.set_markup(&format!(
        "<span foreground=\"{}\">{}</span>",
        color,
        glib::markup_escape_text(marker)
    ));
    title.set_markup(&format!("<b>{}</b>", escaped_title));
    detail.set_text(detail_text);
}

fn agent_state_from_status(status: &str) -> Option<AgentState> {
    match status.trim().to_ascii_lowercase().as_str() {
        "busy" | "working" | "running" => Some(AgentState::Busy),
        "ready" | "prompt" | "waiting" | "needs-user" | "needs_user" => Some(AgentState::Ready),
        "done" | "finished" | "complete" => Some(AgentState::Done),
        "idle" | "clear" | "reset" => Some(AgentState::Idle),
        _ => None,
    }
}

fn install_agent_status_termprop() {
    for prop in [AGENT_STATUS_TERMPROP, CMD_EXIT_TERMPROP] {
        let Ok(name) = CString::new(prop) else {
            continue;
        };
        unsafe {
            vte4::ffi::vte_install_termprop(
                name.as_ptr(),
                vte4::ffi::VTE_PROPERTY_STRING,
                vte4::ffi::VTE_PROPERTY_FLAG_NONE,
            );
        }
    }
}

fn allow_window_transparency(window: &ApplicationWindow) {
    if let Some(native) = window.native() {
        if let Some(surface) = native.surface() {
            surface.set_opaque_region(None);
        }
    }
}

fn shell_quote_path(path: &std::path::Path) -> String {
    let text = path.to_string_lossy();
    format!("'{}'", text.replace('\'', "'\\''"))
}

fn apply_config_to_open_widgets(widget: &gtk4::Widget, cfg: &config::Config) {
    if let Ok(term) = widget.clone().downcast::<vte4::Terminal>() {
        tab::apply_config(&term, cfg);
    }

    if let Ok(view) = widget.clone().downcast::<sourceview5::View>() {
        view.set_wrap_mode(if cfg.editor.word_wrap {
            gtk4::WrapMode::Word
        } else {
            gtk4::WrapMode::None
        });
    }

    let mut child = widget.first_child();
    while let Some(w) = child {
        let next = w.next_sibling();
        apply_config_to_open_widgets(&w, cfg);
        child = next;
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

        .content-paned,
        .terminal-notebook,
        .terminal-notebook > stack,
        paned {{
            background: transparent;
        }}

        vte-terminal {{
            background: transparent;
            padding: {p}px;
        }}

        notebook header {{
            background-color: #181825;
            padding: 0;
        }}
        notebook header.left {{
            border-right: 1px solid #313244;
            min-width: {side_rail_width}px;
            max-width: {side_rail_width}px;
        }}
        notebook header.top {{
            border-bottom: 1px solid #313244;
        }}
        notebook header tab {{
            color: #6c7086;
            padding: 0;
            border-radius: 0;
            border: none;
            box-shadow: none;
            margin-right: 2px;
            min-width: {notebook_tab_width}px;
            max-width: {notebook_tab_width}px;
        }}
        notebook header tab:checked {{
            color: #cdd6f4;
            background-color: #1e1e2e;
            box-shadow: inset 2px 0 0 #89b4fa;
        }}
        notebook header tab:hover:not(:checked) {{
            color: #bac2de;
            background-color: #1e1e2e;
        }}
        .session-tab {{
            padding: 9px 10px;
            min-width: {session_tab_width}px;
            max-width: {session_tab_width}px;
        }}
        .session-tab-dot {{
            font-size: 9pt;
        }}
        .session-tab-title {{
            color: #cdd6f4;
            font-family: {font};
            font-size: {sidebar_pt}pt;
        }}
        .session-tab-detail {{
            color: #a6adc8;
            font-family: {font};
            font-size: {run_task_pt}pt;
        }}
        notebook > stack {{ background-color: transparent; }}

        .sidebar {{
            background-color: #181825;
            border-right: 1px solid #313244;
            min-width: {tool_panel_width}px;
            max-width: {tool_panel_width}px;
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
        .git-branch-label {{
            color: #cba6f7;
            font-family: {font};
            font-size: {run_task_pt}pt;
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
        .agent-badge {{
            color: #1e1e2e;
            background-color: #a6e3a1;
            border-radius: 10px;
            font-size: 9pt;
            font-weight: bold;
            margin: 6px 10px;
            padding: 1px 0;
        }}

        .push-btn {{
            background-color: #313244;
            color: #a6e3a1;
            border: none;
            border-radius: 4px;
            padding: 6px 8px;
            font-size: {sidebar_pt}pt;
            font-weight: bold;
        }}
        .push-btn:hover {{
            background-color: #45475a;
        }}
        .push-btn:disabled {{
            color: #6c7086;
        }}

        .pull-btn {{
            background-color: #313244;
            color: #89b4fa;
            border: none;
            border-radius: 4px;
            padding: 6px 8px;
            font-size: {sidebar_pt}pt;
            font-weight: bold;
        }}
        .pull-btn:hover {{
            background-color: #45475a;
        }}
        .pull-btn:disabled {{
            color: #6c7086;
        }}

        .commit-scroll {{
            border: 1px solid #313244;
            border-radius: 4px;
        }}
        .commit-view,
        .commit-view text {{
            background-color: #181825;
            color: #cdd6f4;
            font-family: {font};
            font-size: {sidebar_pt}pt;
            padding: 4px 6px;
        }}
        .commit-btn {{
            background-color: #cba6f7;
            color: #1e1e2e;
            border: none;
            border-radius: 4px;
            padding: 5px 12px;
            font-size: {sidebar_pt}pt;
            font-weight: bold;
        }}
        .commit-btn:hover {{
            background-color: #d4b1ff;
        }}
        .commit-btn:disabled {{
            background-color: #313244;
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

        .agent-panel-empty {{
            color: #6c7086;
            font-family: {font};
            font-size: {sidebar_pt}pt;
        }}
        .agent-panel-state {{
            color: #a6adc8;
            font-family: {font};
            font-size: {run_task_pt}pt;
            font-weight: bold;
        }}

        .shortcuts-title {{
            color: #cdd6f4;
            font-family: {font};
            font-size: {fsize}pt;
            font-weight: bold;
        }}
        .shortcuts-section {{
            color: #89b4fa;
            font-family: {font};
            font-size: {sidebar_pt}pt;
            font-weight: bold;
        }}
        .shortcuts-keys {{
            color: #f9e2af;
            font-family: {font};
            font-size: {sidebar_pt}pt;
        }}
        .shortcuts-action {{
            color: #cdd6f4;
            font-family: {font};
            font-size: {sidebar_pt}pt;
        }}
        ",
        p = cfg.window.padding,
        side_rail_width = SIDE_RAIL_WIDTH,
        tool_panel_width = TOOL_PANEL_WIDTH,
        notebook_tab_width = NOTEBOOK_TAB_WIDTH,
        session_tab_width = SESSION_TAB_WIDTH,
    )
}
