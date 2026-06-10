use gtk4::gdk;
use gtk4::prelude::*;
use std::cell::RefCell;
use std::rc::Rc;

/// One runnable entry in the command palette.
pub struct Action {
    pub title: &'static str,
    pub shortcut: Option<&'static str>,
    pub run: Rc<dyn Fn()>,
}

/// Case-insensitive subsequence match: every query char must appear in order.
/// Returns a score (lower is better) so tighter matches rank first, or None.
pub fn match_score(title: &str, query: &str) -> Option<usize> {
    if query.is_empty() {
        return Some(0);
    }
    let title: Vec<char> = title.to_lowercase().chars().collect();
    let query: Vec<char> = query.to_lowercase().chars().collect();
    let mut score = 0;
    let mut ti = 0;
    for qc in &query {
        let start = ti;
        loop {
            if ti >= title.len() {
                return None;
            }
            if title[ti] == *qc {
                break;
            }
            ti += 1;
        }
        // Gaps between matched chars push the entry down the list.
        score += ti - start;
        ti += 1;
    }
    Some(score)
}

/// Indices of `actions` matching `query`, best matches first.
pub fn filter_actions(titles: &[&str], query: &str) -> Vec<usize> {
    let mut scored: Vec<(usize, usize)> = titles
        .iter()
        .enumerate()
        .filter_map(|(i, t)| match_score(t, query).map(|s| (s, i)))
        .collect();
    scored.sort();
    scored.into_iter().map(|(_, i)| i).collect()
}

pub fn show(parent: &gtk4::ApplicationWindow, actions: Rc<Vec<Action>>) {
    let win = gtk4::Window::new();
    win.set_transient_for(Some(parent));
    win.set_modal(true);
    win.set_decorated(false);
    win.set_resizable(false);
    win.set_default_size(540, 340);
    win.add_css_class("quickopen-window");

    let vbox = gtk4::Box::new(gtk4::Orientation::Vertical, 0);

    let entry = gtk4::Entry::new();
    entry.set_placeholder_text(Some("Run a command…"));
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

    // Visible row order -> action index, refreshed on every filter pass.
    let visible: Rc<RefCell<Vec<usize>>> = Rc::new(RefCell::new(Vec::new()));

    let populate = Rc::new({
        let list = list.clone();
        let actions = Rc::clone(&actions);
        let visible = Rc::clone(&visible);
        move |query: &str| {
            while let Some(row) = list.row_at_index(0) {
                list.remove(&row);
            }
            let titles: Vec<&str> = actions.iter().map(|a| a.title).collect();
            let matched = filter_actions(&titles, query);
            for idx in &matched {
                let action = &actions[*idx];
                let row = gtk4::Box::new(gtk4::Orientation::Horizontal, 8);
                row.set_margin_top(4);
                row.set_margin_bottom(4);
                row.set_margin_start(8);
                row.set_margin_end(8);
                let name = gtk4::Label::new(Some(action.title));
                name.add_css_class("quickopen-name");
                name.set_xalign(0.0);
                name.set_hexpand(true);
                row.append(&name);
                if let Some(sc) = action.shortcut {
                    let key = gtk4::Label::new(Some(sc));
                    key.add_css_class("quickopen-path");
                    key.set_xalign(1.0);
                    row.append(&key);
                }
                list.append(&row);
            }
            *visible.borrow_mut() = matched;
            if let Some(first) = list.row_at_index(0) {
                list.select_row(Some(&first));
            }
        }
    });
    populate("");

    {
        let populate = Rc::clone(&populate);
        entry.connect_changed(move |e| {
            populate(&e.text());
        });
    }

    let activate_row = Rc::new({
        let actions = Rc::clone(&actions);
        let visible = Rc::clone(&visible);
        let win = win.clone();
        move |row_index: i32| {
            let action_run = visible
                .borrow()
                .get(row_index as usize)
                .map(|idx| Rc::clone(&actions[*idx].run));
            if let Some(run) = action_run {
                win.close();
                run();
            }
        }
    });

    {
        let activate = Rc::clone(&activate_row);
        list.connect_row_activated(move |_, row| {
            activate(row.index());
        });
    }

    // Enter runs the selected row; arrows move the selection from the entry.
    {
        let list = list.clone();
        let activate = Rc::clone(&activate_row);
        entry.connect_activate(move |_| {
            if let Some(row) = list.selected_row() {
                activate(row.index());
            }
        });
    }
    {
        let list = list.clone();
        let win_c = win.clone();
        let key = gtk4::EventControllerKey::new();
        key.set_propagation_phase(gtk4::PropagationPhase::Capture);
        key.connect_key_pressed(move |_, keyval, _, _| match keyval {
            gdk::Key::Escape => {
                win_c.close();
                glib::Propagation::Stop
            }
            gdk::Key::Down | gdk::Key::Up => {
                let count = {
                    let mut n = 0;
                    while list.row_at_index(n).is_some() {
                        n += 1;
                    }
                    n
                };
                if count > 0 {
                    let cur = list.selected_row().map(|r| r.index()).unwrap_or(-1);
                    let next = if keyval == gdk::Key::Down {
                        (cur + 1).min(count - 1)
                    } else {
                        (cur - 1).max(0)
                    };
                    if let Some(row) = list.row_at_index(next) {
                        list.select_row(Some(&row));
                        row.grab_focus();
                    }
                }
                glib::Propagation::Stop
            }
            _ => glib::Propagation::Proceed,
        });
        entry.add_controller(key);
    }

    win.present();
    entry.grab_focus();
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn empty_query_matches_everything() {
        assert_eq!(match_score("New Tab", ""), Some(0));
    }

    #[test]
    fn subsequence_matches_case_insensitively() {
        assert!(match_score("Split Terminal Right", "str").is_some());
        assert!(match_score("Split Terminal Right", "RIGHT").is_some());
        assert!(match_score("Split Terminal Right", "xyz").is_none());
    }

    #[test]
    fn tighter_matches_rank_first() {
        let titles = ["Toggle Sidebar", "Split Terminal Right", "New Tab"];
        let order = filter_actions(&titles, "tab");
        assert_eq!(order.first(), Some(&2));
    }

    #[test]
    fn non_matches_are_excluded() {
        let titles = ["New Tab", "Zoom In"];
        assert_eq!(filter_actions(&titles, "zzz"), Vec::<usize>::new());
    }
}
