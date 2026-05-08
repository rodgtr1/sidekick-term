use crate::git;
use gtk4::prelude::*;
use std::rc::Rc;

pub struct GitPanel {
    pub widget: gtk4::Box,
    pub header: gtk4::Label,
    pub list: gtk4::ListBox,
    pub push_btn: gtk4::Button,
    pub pull_btn: gtk4::Button,
    pub commit_view: gtk4::TextView,
    pub commit_btn: gtk4::Button,
}

pub fn build() -> GitPanel {
    let header = gtk4::Label::new(Some("GIT CHANGES"));
    header.set_xalign(0.0);
    header.add_css_class("sidebar-header");

    let list = gtk4::ListBox::new();
    list.set_selection_mode(gtk4::SelectionMode::Single);
    list.add_css_class("file-tree");

    let list_scroll = gtk4::ScrolledWindow::new();
    list_scroll.set_child(Some(&list));
    list_scroll.set_hscrollbar_policy(gtk4::PolicyType::Never);
    list_scroll.set_vscrollbar_policy(gtk4::PolicyType::Automatic);
    list_scroll.set_vexpand(true);

    // Commit message area
    let commit_view = gtk4::TextView::new();
    commit_view.set_wrap_mode(gtk4::WrapMode::WordChar);
    commit_view.set_accepts_tab(false);
    commit_view.add_css_class("commit-view");

    let commit_scroll = gtk4::ScrolledWindow::new();
    commit_scroll.set_child(Some(&commit_view));
    commit_scroll.set_hscrollbar_policy(gtk4::PolicyType::Never);
    commit_scroll.set_vscrollbar_policy(gtk4::PolicyType::Automatic);
    commit_scroll.set_height_request(64);
    commit_scroll.set_margin_start(8);
    commit_scroll.set_margin_end(8);
    commit_scroll.set_margin_top(6);
    commit_scroll.set_margin_bottom(0);
    commit_scroll.add_css_class("commit-scroll");

    let commit_btn = gtk4::Button::with_label("Commit staged");
    commit_btn.add_css_class("commit-btn");
    commit_btn.set_sensitive(false);
    commit_btn.set_margin_start(8);
    commit_btn.set_margin_end(8);
    commit_btn.set_margin_top(4);
    commit_btn.set_margin_bottom(4);

    // Pull / Push row
    let pull_btn = gtk4::Button::with_label("↓  pull");
    pull_btn.add_css_class("pull-btn");
    pull_btn.set_hexpand(true);

    let push_btn = gtk4::Button::with_label("↑  push");
    push_btn.add_css_class("push-btn");
    push_btn.set_hexpand(true);

    let action_row = gtk4::Box::new(gtk4::Orientation::Horizontal, 4);
    action_row.set_margin_start(8);
    action_row.set_margin_end(8);
    action_row.set_margin_bottom(8);
    action_row.append(&pull_btn);
    action_row.append(&push_btn);

    let panel = gtk4::Box::new(gtk4::Orientation::Vertical, 0);
    panel.append(&header);
    panel.append(&list_scroll);
    panel.append(&commit_scroll);
    panel.append(&commit_btn);
    panel.append(&action_row);

    GitPanel {
        widget: panel,
        header,
        list,
        push_btn,
        pull_btn,
        commit_view,
        commit_btn,
    }
}

pub fn update_push_button(btn: &gtk4::Button, ahead: u32) {
    if ahead == 0 {
        btn.set_label("↑  push");
    } else {
        btn.set_label(&format!("↑  push  {ahead}"));
    }
}

pub fn update_commit_button(btn: &gtk4::Button, staged_count: usize) {
    btn.set_sensitive(staged_count > 0);
}

/// Returns the number of staged files.
pub fn populate(
    list: &gtk4::ListBox,
    files: &[git::GitFile],
    root: &str,
    on_refresh: &Rc<dyn Fn()>,
) -> usize {
    while let Some(row) = list.row_at_index(0) {
        list.remove(&row);
    }

    if files.is_empty() {
        let row = gtk4::ListBoxRow::new();
        let label = gtk4::Label::new(Some("No changes"));
        label.set_margin_top(8);
        label.set_margin_bottom(8);
        label.add_css_class("sidebar-header");
        row.set_child(Some(&label));
        row.set_activatable(false);
        row.set_selectable(false);
        list.insert(&row, -1);
        return 0;
    }

    let staged: Vec<_> = files.iter().filter(|f| f.staged).collect();
    let unstaged: Vec<_> = files.iter().filter(|f| !f.staged).collect();
    let staged_count = staged.len();

    if !staged.is_empty() {
        add_section_header(list, "STAGED");
        for file in &staged {
            add_file_row(list, file, true, root, on_refresh);
        }
    }

    if !unstaged.is_empty() {
        add_section_header(list, "UNSTAGED");
        for file in &unstaged {
            add_file_row(list, file, false, root, on_refresh);
        }
    }

    staged_count
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

fn add_file_row(
    list: &gtk4::ListBox,
    file: &git::GitFile,
    staged: bool,
    root: &str,
    on_refresh: &Rc<dyn Fn()>,
) {
    let row = gtk4::ListBoxRow::new();
    let prefix = if staged { "s:" } else { "u:" };
    row.set_widget_name(&format!("{}{}", prefix, file.rel_path));

    let hbox = gtk4::Box::new(gtk4::Orientation::Horizontal, 6);
    hbox.set_margin_start(8);
    hbox.set_margin_end(8);
    hbox.set_margin_top(2);
    hbox.set_margin_bottom(2);

    let status_label = gtk4::Label::new(None);
    status_label.set_markup(&format!(
        "<span foreground=\"{}\" weight=\"bold\">{}</span>",
        file.status.color(),
        file.status.symbol(),
    ));
    status_label.set_width_chars(2);
    status_label.set_xalign(0.5);

    let filename = std::path::Path::new(&file.rel_path)
        .file_name()
        .map(|n| n.to_string_lossy().to_string())
        .unwrap_or_else(|| file.rel_path.clone());

    let name_label = gtk4::Label::new(Some(&filename));
    name_label.set_xalign(0.0);
    name_label.set_hexpand(true);
    name_label.set_ellipsize(pango::EllipsizeMode::Start);
    name_label.set_tooltip_text(Some(&file.rel_path));

    hbox.append(&status_label);
    hbox.append(&name_label);
    row.set_child(Some(&hbox));

    let rel_path = file.rel_path.clone();
    let root_s = root.to_string();
    let is_untracked = file.status == git::GitStatus::Untracked;
    let refresh = Rc::clone(on_refresh);
    let gesture = gtk4::GestureClick::new();
    gesture.set_button(3);
    gesture.connect_pressed(move |gesture, _n, x, y| {
        let Some(widget) = gesture.widget() else {
            return;
        };
        show_context_menu(
            &widget,
            x,
            y,
            staged,
            &rel_path,
            &root_s,
            is_untracked,
            &refresh,
        );
        gesture.set_state(gtk4::EventSequenceState::Claimed);
    });
    row.add_controller(gesture);

    list.insert(&row, -1);
}

fn show_git_error(widget: &gtk4::Widget, msg: &str) {
    let window = widget
        .root()
        .and_then(|r| r.downcast::<gtk4::Window>().ok());
    gtk4::AlertDialog::builder()
        .message("Git operation failed")
        .detail(msg)
        .build()
        .show(window.as_ref());
}

fn show_context_menu(
    parent: &gtk4::Widget,
    x: f64,
    y: f64,
    staged: bool,
    rel_path: &str,
    root: &str,
    is_untracked: bool,
    on_refresh: &Rc<dyn Fn()>,
) {
    let popover = gtk4::Popover::new();
    popover.set_has_arrow(false);
    popover.set_pointing_to(Some(&gtk4::gdk::Rectangle::new(x as i32, y as i32, 1, 1)));
    popover.add_css_class("context-menu");

    let vbox = gtk4::Box::new(gtk4::Orientation::Vertical, 0);

    if staged {
        add_menu_item(&vbox, "Unstage", &popover, {
            let path = rel_path.to_string();
            let root = root.to_string();
            let refresh = Rc::clone(on_refresh);
            let parent_w = parent.clone();
            move || match git::unstage(&root, &path) {
                Ok(()) => refresh(),
                Err(e) => show_git_error(&parent_w, &e),
            }
        });
    } else {
        add_menu_item(&vbox, "Stage", &popover, {
            let path = rel_path.to_string();
            let root = root.to_string();
            let refresh = Rc::clone(on_refresh);
            let parent_w = parent.clone();
            move || match git::stage(&root, &path) {
                Ok(()) => refresh(),
                Err(e) => show_git_error(&parent_w, &e),
            }
        });
        add_menu_item(&vbox, "Discard changes", &popover, {
            let path = rel_path.to_string();
            let root = root.to_string();
            let refresh = Rc::clone(on_refresh);
            let parent_w = parent.clone();
            move || match git::discard(&root, &path, is_untracked) {
                Ok(()) => refresh(),
                Err(e) => show_git_error(&parent_w, &e),
            }
        });
    }

    popover.set_child(Some(&vbox));
    popover.set_parent(parent);
    popover.connect_closed(|p| p.unparent());
    popover.popup();
}

fn add_menu_item<F: Fn() + 'static>(
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
