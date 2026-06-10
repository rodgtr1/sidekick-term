use gtk4::prelude::*;

/// Returns the terminal that a split should attach next to, if any.
pub fn split_target(
    window: &gtk4::ApplicationWindow,
    notebook: &gtk4::Notebook,
) -> Option<vte4::Terminal> {
    focused_terminal(window, notebook)
}

/// Insert `new_term` next to `focused` in a new Paned. The caller is
/// responsible for building, spawning, and wiring `new_term` (agent status,
/// child-exited handling) so split panes are first-class terminals.
pub fn split_with(
    notebook: &gtk4::Notebook,
    focused: &vte4::Terminal,
    new_term: &vte4::Terminal,
    orientation: gtk4::Orientation,
) {
    let focused_w: gtk4::Widget = focused.clone().upcast();

    let paned = gtk4::Paned::new(orientation);

    // Case 1: focused is a direct notebook page child
    if let Some(idx) = notebook.page_num(&focused_w) {
        let label = notebook.tab_label(&focused_w);
        notebook.remove_page(Some(idx));
        paned.set_start_child(Some(focused));
        paned.set_end_child(Some(new_term));
        notebook.insert_page(&paned, label.as_ref(), Some(idx));
        notebook.set_current_page(Some(idx));

    // Case 2: focused is inside a Paned
    } else if let Some(pp) = focused_w
        .parent()
        .and_then(|p| p.downcast::<gtk4::Paned>().ok())
    {
        let is_start = same_widget(pp.start_child().as_ref(), &focused_w);
        let pp_pos = pp.position();

        // Use the container API (not unparent) to detach — unparent() is for
        // widget implementations only and triggers a double-free via the
        // container's remove handler.
        if is_start {
            pp.set_start_child(None::<&gtk4::Widget>);
        } else {
            pp.set_end_child(None::<&gtk4::Widget>);
        }

        paned.set_start_child(Some(focused));
        paned.set_end_child(Some(new_term));

        if is_start {
            pp.set_start_child(Some(&paned));
        } else {
            pp.set_end_child(Some(&paned));
        }

        // Restore parent divider after GTK's layout pass resets it.
        // idle_add runs after the current event (and any resulting layouts).
        let pp2 = pp.clone();
        glib::idle_add_local(move || {
            pp2.set_position(pp_pos);
            glib::ControlFlow::Break
        });
    }

    // Set new pane to 50/50 after layout so we know the real allocated size.
    let p = paned.clone();
    glib::idle_add_local(move || {
        let sz = match p.orientation() {
            gtk4::Orientation::Horizontal => p.width(),
            _ => p.height(),
        };
        p.set_position(if sz > 0 { sz / 2 } else { 300 });
        glib::ControlFlow::Break
    });

    new_term.grab_focus();
}

pub fn close(window: &gtk4::ApplicationWindow, notebook: &gtk4::Notebook) -> bool {
    focused_terminal(window, notebook)
        .map(|t| close_terminal(&t, notebook))
        .unwrap_or(false)
}

pub fn close_terminal(terminal: &vte4::Terminal, notebook: &gtk4::Notebook) -> bool {
    let term_w: gtk4::Widget = terminal.clone().upcast();

    // Sole pane — close the whole tab
    if let Some(idx) = notebook.page_num(&term_w) {
        notebook.remove_page(Some(idx));
        return notebook.n_pages() == 0;
    }

    let parent = match term_w
        .parent()
        .and_then(|p| p.downcast::<gtk4::Paned>().ok())
    {
        Some(p) => p,
        None => return false,
    };

    let start = parent.start_child();
    let end = parent.end_child();
    let term_is_start = same_widget(start.as_ref(), &term_w);
    let sibling = if term_is_start { end } else { start };

    let sibling = match sibling {
        Some(s) => s,
        None => return false,
    };

    // Detach sibling via the container API before re-parenting it.
    if term_is_start {
        parent.set_end_child(None::<&gtk4::Widget>);
    } else {
        parent.set_start_child(None::<&gtk4::Widget>);
    }

    let parent_w: gtk4::Widget = parent.clone().upcast();

    if let Some(idx) = notebook.page_num(&parent_w) {
        let label = notebook.tab_label(&parent_w);
        notebook.remove_page(Some(idx));
        notebook.insert_page(&sibling, label.as_ref(), Some(idx));
        notebook.set_current_page(Some(idx));
    } else if let Some(gp) = parent_w
        .parent()
        .and_then(|p| p.downcast::<gtk4::Paned>().ok())
    {
        if same_widget(gp.start_child().as_ref(), &parent_w) {
            gp.set_start_child(Some(&sibling));
        } else {
            gp.set_end_child(Some(&sibling));
        }
    }

    focus_first_terminal(&sibling);
    false
}

pub fn navigate(window: &gtk4::ApplicationWindow, notebook: &gtk4::Notebook, forward: bool) {
    let focused = match focused_terminal(window, notebook) {
        Some(t) => t,
        None => return,
    };
    let root = match notebook.nth_page(Some(notebook.current_page().unwrap_or(0))) {
        Some(r) => r,
        None => return,
    };

    let terminals = collect_terminals(&root);
    if terminals.len() <= 1 {
        return;
    }

    let fw: gtk4::Widget = focused.clone().upcast();
    let pos = terminals
        .iter()
        .position(|t| same_widget(Some(&t.clone().upcast()), &fw));

    if let Some(i) = pos {
        let next = if forward {
            (i + 1) % terminals.len()
        } else if i == 0 {
            terminals.len() - 1
        } else {
            i - 1
        };
        terminals[next].grab_focus();
    }
}

// ── helpers ──────────────────────────────────────────────────────────────────

fn focused_terminal(
    window: &gtk4::ApplicationWindow,
    notebook: &gtk4::Notebook,
) -> Option<vte4::Terminal> {
    let current_page = notebook.nth_page(Some(notebook.current_page()?))?;

    if let Some(term) =
        gtk4::prelude::GtkWindowExt::focus(window).and_then(|w| w.downcast::<vte4::Terminal>().ok())
    {
        let term_w: gtk4::Widget = term.clone().upcast();
        if widget_contains(&current_page, &term_w) {
            return Some(term);
        }
    }

    collect_terminals(&current_page).into_iter().next()
}

fn same_widget(a: Option<&gtk4::Widget>, b: &gtk4::Widget) -> bool {
    a.map(|w| w.as_ptr() == b.as_ptr()).unwrap_or(false)
}

fn widget_contains(root: &gtk4::Widget, child: &gtk4::Widget) -> bool {
    let mut widget = child.clone();
    loop {
        if same_widget(Some(&widget), root) {
            return true;
        }
        widget = match widget.parent() {
            Some(parent) => parent,
            None => return false,
        };
    }
}

fn focus_first_terminal(widget: &gtk4::Widget) {
    if let Ok(t) = widget.clone().downcast::<vte4::Terminal>() {
        t.grab_focus();
    } else if let Ok(p) = widget.clone().downcast::<gtk4::Paned>() {
        if let Some(s) = p.start_child() {
            focus_first_terminal(&s);
        }
    }
}

pub fn collect_terminals_pub(widget: &gtk4::Widget) -> Vec<vte4::Terminal> {
    collect_terminals(widget)
}

fn collect_terminals(widget: &gtk4::Widget) -> Vec<vte4::Terminal> {
    if let Ok(t) = widget.clone().downcast::<vte4::Terminal>() {
        return vec![t];
    }
    if let Ok(p) = widget.clone().downcast::<gtk4::Paned>() {
        let mut out = vec![];
        if let Some(s) = p.start_child() {
            out.extend(collect_terminals(&s));
        }
        if let Some(e) = p.end_child() {
            out.extend(collect_terminals(&e));
        }
        return out;
    }
    vec![]
}
