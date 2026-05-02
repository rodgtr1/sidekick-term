use crate::git;
use gtk4::prelude::*;

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

pub fn populate(list: &gtk4::ListBox, files: &[git::GitFile]) {
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
            add_file_row(list, file, true);
        }
    }

    if !unstaged.is_empty() {
        add_section_header(list, "UNSTAGED");
        for file in &unstaged {
            add_file_row(list, file, false);
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

fn add_file_row(list: &gtk4::ListBox, file: &git::GitFile, staged: bool) {
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

    list.insert(&row, -1);
}
