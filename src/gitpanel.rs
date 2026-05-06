use crate::git;
use gtk4::prelude::*;
use std::rc::Rc;

pub fn build() -> (gtk4::Box, gtk4::Label, gtk4::ListBox, gtk4::Button) {
    let header = gtk4::Label::new(Some("GIT CHANGES"));
    header.set_xalign(0.0);
    header.add_css_class("sidebar-header");

    let list = gtk4::ListBox::new();
    list.set_selection_mode(gtk4::SelectionMode::Single);
    list.add_css_class("file-tree");

    let scroll = gtk4::ScrolledWindow::new();
    scroll.set_child(Some(&list));
    scroll.set_hscrollbar_policy(gtk4::PolicyType::Never);
    scroll.set_vscrollbar_policy(gtk4::PolicyType::Automatic);
    scroll.set_vexpand(true);

    let push_btn = gtk4::Button::with_label("↑  push");
    push_btn.add_css_class("push-btn");
    push_btn.set_visible(false);

    let panel = gtk4::Box::new(gtk4::Orientation::Vertical, 0);
    panel.append(&header);
    panel.append(&scroll);
    panel.append(&push_btn);

    (panel, header, list, push_btn)
}

pub fn update_push_button(btn: &gtk4::Button, ahead: u32) {
    if ahead == 0 {
        btn.set_visible(false);
    } else {
        btn.set_label(&format!("↑  push  {ahead}"));
        btn.set_sensitive(true);
        btn.set_visible(true);
    }
}

pub fn populate(
    list: &gtk4::ListBox,
    files: &[git::GitFile],
    root: &str,
    on_refresh: &Rc<dyn Fn()>,
) {
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
        return;
    }

    let staged: Vec<_> = files.iter().filter(|f| f.staged).collect();
    let unstaged: Vec<_> = files.iter().filter(|f| !f.staged).collect();

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

    // Right-click context menu
    let rel_path = file.rel_path.clone();
    let root_s = root.to_string();
    let is_untracked = file.status == git::GitStatus::Untracked;
    let refresh = Rc::clone(on_refresh);
    let gesture = gtk4::GestureClick::new();
    gesture.set_button(3);
    gesture.connect_pressed(move |gesture, _n, x, y| {
        let Some(widget) = gesture.widget() else { return };
        show_context_menu(&widget, x, y, staged, &rel_path, &root_s, is_untracked, &refresh);
        gesture.set_state(gtk4::EventSequenceState::Claimed);
    });
    row.add_controller(gesture);

    list.insert(&row, -1);
}

fn show_git_error(widget: &gtk4::Widget, msg: &str) {
    let window = widget.root().and_then(|r| r.downcast::<gtk4::Window>().ok());
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
