use gtk4::prelude::*;

#[derive(Clone, Debug, serde::Deserialize)]
pub struct Task {
    pub name: String,
    pub cmd: String,
    #[serde(default)]
    pub llm: Option<String>,
    /// URL to open in the embedded browser panel when the task runs
    /// (e.g. "http://localhost:3000").
    #[serde(default)]
    pub open_browser: Option<String>,
}

/// What the user asked a task row to do.
#[derive(Clone, Copy, PartialEq)]
pub enum TaskAction {
    /// Type the command into the focused terminal without running it.
    Paste,
    /// Run the command in a dedicated split pane.
    Run,
}

pub fn load_tasks(root: &str) -> Vec<Task> {
    let path = format!("{}/.sidekick.toml", root);
    let content = match std::fs::read_to_string(&path) {
        Ok(c) => c,
        Err(_) => return vec![],
    };
    #[derive(serde::Deserialize)]
    struct TaskFile {
        #[serde(default)]
        tasks: Vec<Task>,
    }
    toml::from_str::<TaskFile>(&content)
        .map(|f| f.tasks)
        .unwrap_or_default()
}

pub fn build() -> (gtk4::Box, gtk4::ListBox) {
    let header = gtk4::Label::new(Some("RUN"));
    header.set_xalign(0.0);
    header.add_css_class("sidebar-header");

    let list = gtk4::ListBox::new();
    list.set_selection_mode(gtk4::SelectionMode::None);
    list.add_css_class("file-tree");

    let scroll = gtk4::ScrolledWindow::new();
    scroll.set_child(Some(&list));
    scroll.set_hscrollbar_policy(gtk4::PolicyType::Never);
    scroll.set_vscrollbar_policy(gtk4::PolicyType::Automatic);
    scroll.set_vexpand(true);

    let panel = gtk4::Box::new(gtk4::Orientation::Vertical, 0);
    panel.append(&header);
    panel.append(&scroll);

    (panel, list)
}

/// Populate the task list from global (config.toml) and local (.sidekick.toml) tasks.
/// Shows section headers when both are present. `inject` is called with the
/// task, the requested action, and the row's status label when a button is
/// clicked.
pub fn populate<F>(list: &gtk4::ListBox, global: &[Task], local: &[Task], inject: F)
where
    F: Fn(&Task, TaskAction, &gtk4::Label) + Clone + 'static,
{
    while let Some(row) = list.row_at_index(0) {
        list.remove(&row);
    }

    if global.is_empty() && local.is_empty() {
        let row = gtk4::ListBoxRow::new();
        let label = gtk4::Label::new(Some(
            "No tasks — add [[tasks]] to ~/.config/sidekick/config.toml or .sidekick.toml",
        ));
        label.set_margin_top(8);
        label.set_margin_bottom(8);
        label.set_margin_start(8);
        label.set_margin_end(8);
        label.set_wrap(true);
        label.set_xalign(0.0);
        label.add_css_class("sidebar-header");
        row.set_child(Some(&label));
        row.set_activatable(false);
        row.set_selectable(false);
        list.insert(&row, -1);
        return;
    }

    let show_headers = !global.is_empty() && !local.is_empty();

    if !global.is_empty() {
        if show_headers {
            add_section_header(list, "GLOBAL");
        }
        for task in global {
            add_task_row(list, task, &inject);
        }
    }

    if !local.is_empty() {
        if show_headers {
            add_section_header(list, "PROJECT");
        }
        for task in local {
            add_task_row(list, task, &inject);
        }
    }
}

fn add_section_header(list: &gtk4::ListBox, text: &str) {
    let row = gtk4::ListBoxRow::new();
    let label = gtk4::Label::new(Some(text));
    label.set_xalign(0.0);
    label.set_margin_start(8);
    label.set_margin_top(6);
    label.set_margin_bottom(2);
    label.add_css_class("git-section-header");
    row.set_child(Some(&label));
    row.set_activatable(false);
    row.set_selectable(false);
    list.insert(&row, -1);
}

fn add_task_row<F>(list: &gtk4::ListBox, task: &Task, inject: &F)
where
    F: Fn(&Task, TaskAction, &gtk4::Label) + Clone + 'static,
{
    let row = gtk4::ListBoxRow::new();
    row.set_activatable(false);
    row.set_selectable(false);

    let hbox = gtk4::Box::new(gtk4::Orientation::Horizontal, 4);
    hbox.set_margin_start(8);
    hbox.set_margin_end(6);
    hbox.set_margin_top(4);
    hbox.set_margin_bottom(4);

    let name_label = gtk4::Label::new(Some(&task.name));
    name_label.set_xalign(0.0);
    name_label.set_hexpand(true);
    name_label.set_ellipsize(pango::EllipsizeMode::End);
    name_label.set_tooltip_text(Some(&task.cmd));
    name_label.add_css_class("run-task-name");

    // Live run status (set by main when the task runs in a split).
    let status_label = gtk4::Label::new(None);
    status_label.add_css_class("run-task-name");

    let paste_btn = gtk4::Button::with_label("→");
    paste_btn.add_css_class("run-btn");
    paste_btn.set_tooltip_text(Some("Paste to prompt"));
    {
        let task = task.clone();
        let status = status_label.clone();
        let inject_c = inject.clone();
        paste_btn.connect_clicked(move |_| inject_c(&task, TaskAction::Paste, &status));
    }

    let run_btn = gtk4::Button::with_label("▶");
    run_btn.add_css_class("run-btn");
    run_btn.set_tooltip_text(Some("Run in a split below"));
    {
        let task = task.clone();
        let status = status_label.clone();
        let inject_c = inject.clone();
        run_btn.connect_clicked(move |_| inject_c(&task, TaskAction::Run, &status));
    }

    hbox.append(&name_label);
    hbox.append(&status_label);
    hbox.append(&paste_btn);
    hbox.append(&run_btn);

    if let Some(prompt) = &task.llm {
        let llm_btn = gtk4::Button::with_label("✦");
        llm_btn.add_css_class("run-btn");
        llm_btn.set_tooltip_text(Some("Copy prompt to clipboard"));
        let prompt = prompt.clone();
        llm_btn.connect_clicked(move |btn| {
            btn.display().clipboard().set_text(&prompt);
        });
        hbox.append(&llm_btn);
    }

    row.set_child(Some(&hbox));
    list.insert(&row, -1);
}
