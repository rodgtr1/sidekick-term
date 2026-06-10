use gtk4::prelude::*;
use std::cell::RefCell;
use std::rc::Rc;

/// One dashboard row: a tab with its agent state and time in that state.
pub struct Row {
    pub page_index: u32,
    pub title: String,
    pub state_label: &'static str,
    pub color: &'static str,
    pub elapsed_secs: u64,
}

/// Cached widgets for in-place updates: rebuilding rows every second would
/// fight row activation, so only the elapsed label updates when the row
/// identity (page, title, state) is unchanged.
struct RowWidgets {
    page_index: u32,
    title: String,
    state_label: &'static str,
    elapsed: gtk4::Label,
}

pub struct AgentPanel {
    pub widget: gtk4::Box,
    pub list: gtk4::ListBox,
    cache: Rc<RefCell<Vec<RowWidgets>>>,
    empty: gtk4::Label,
}

pub fn build() -> AgentPanel {
    let widget = gtk4::Box::new(gtk4::Orientation::Vertical, 0);

    let header = gtk4::Label::new(Some("AGENTS"));
    header.add_css_class("sidebar-header");
    header.set_xalign(0.0);
    widget.append(&header);

    let empty = gtk4::Label::new(Some("No terminal tabs."));
    empty.add_css_class("agent-panel-empty");
    empty.set_xalign(0.0);
    empty.set_margin_start(12);
    empty.set_margin_top(4);
    widget.append(&empty);

    let list = gtk4::ListBox::new();
    list.set_selection_mode(gtk4::SelectionMode::None);
    list.add_css_class("quickopen-list");
    list.set_margin_start(4);
    list.set_margin_end(4);

    let scroll = gtk4::ScrolledWindow::new();
    scroll.set_child(Some(&list));
    scroll.set_hscrollbar_policy(gtk4::PolicyType::Never);
    scroll.set_vscrollbar_policy(gtk4::PolicyType::Automatic);
    scroll.set_vexpand(true);
    widget.append(&scroll);

    AgentPanel {
        widget,
        list,
        cache: Rc::new(RefCell::new(Vec::new())),
        empty,
    }
}

pub fn format_elapsed(secs: u64) -> String {
    if secs >= 3600 {
        format!("{}h {}m", secs / 3600, (secs % 3600) / 60)
    } else if secs >= 60 {
        format!("{}m {}s", secs / 60, secs % 60)
    } else {
        format!("{secs}s")
    }
}

impl AgentPanel {
    /// Render `rows`, updating elapsed labels in place when the row set is
    /// unchanged. Row widget names carry the page index for activation.
    pub fn populate(&self, rows: &[Row]) {
        self.empty.set_visible(rows.is_empty());

        let unchanged = {
            let cache = self.cache.borrow();
            cache.len() == rows.len()
                && cache.iter().zip(rows).all(|(c, r)| {
                    c.page_index == r.page_index
                        && c.title == r.title
                        && c.state_label == r.state_label
                })
        };
        if unchanged {
            for (c, r) in self.cache.borrow().iter().zip(rows) {
                c.elapsed.set_text(&format_elapsed(r.elapsed_secs));
            }
            return;
        }

        while let Some(row) = self.list.row_at_index(0) {
            self.list.remove(&row);
        }
        let mut cache = Vec::with_capacity(rows.len());
        for row in rows {
            let hbox = gtk4::Box::new(gtk4::Orientation::Horizontal, 8);
            hbox.set_margin_top(4);
            hbox.set_margin_bottom(4);
            hbox.set_margin_start(8);
            hbox.set_margin_end(8);

            let dot = gtk4::Label::new(None);
            dot.set_markup(&format!("<span foreground=\"{}\">●</span>", row.color));

            let title = gtk4::Label::new(Some(&row.title));
            title.add_css_class("quickopen-name");
            title.set_xalign(0.0);
            title.set_hexpand(true);
            title.set_ellipsize(pango::EllipsizeMode::End);

            let state = gtk4::Label::new(Some(row.state_label));
            state.add_css_class("agent-panel-state");
            state.set_xalign(1.0);

            let elapsed = gtk4::Label::new(Some(&format_elapsed(row.elapsed_secs)));
            elapsed.add_css_class("quickopen-path");
            elapsed.set_xalign(1.0);
            elapsed.set_width_chars(6);

            hbox.append(&dot);
            hbox.append(&title);
            hbox.append(&state);
            hbox.append(&elapsed);

            let list_row = gtk4::ListBoxRow::new();
            list_row.set_child(Some(&hbox));
            list_row.set_widget_name(&row.page_index.to_string());
            list_row.set_activatable(true);
            self.list.append(&list_row);

            cache.push(RowWidgets {
                page_index: row.page_index,
                title: row.title.clone(),
                state_label: row.state_label,
                elapsed,
            });
        }
        *self.cache.borrow_mut() = cache;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn elapsed_formats_scale_with_duration() {
        assert_eq!(format_elapsed(0), "0s");
        assert_eq!(format_elapsed(59), "59s");
        assert_eq!(format_elapsed(83), "1m 23s");
        assert_eq!(format_elapsed(3700), "1h 1m");
    }
}
