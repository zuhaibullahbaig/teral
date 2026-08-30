//! The floating search panel.
//!
//! Search slides down from the top of the file area rather than pushing the content
//! out of the way, and carries its own close button so it never depends on the user
//! knowing about Escape.

use super::App;
use crate::icons;
use gtk::prelude::*;
use std::rc::Rc;

/// Widgets of the floating search panel.
pub struct Search {
    pub root: gtk::Revealer,
    pub entry: gtk::SearchEntry,
    pub matches: gtk::Label,
    pub close: gtk::Button,
}

pub fn build() -> Search {
    let entry = gtk::SearchEntry::new();
    entry.add_css_class("teral-search");
    entry.set_hexpand(true);
    entry.set_width_chars(28);
    entry.set_placeholder_text(Some("Filter this folder by name"));

    let matches = gtk::Label::new(None);
    matches.add_css_class("teral-status-item");

    let close = super::icon_button(icons::ui(icons::names::CLOSE), "Close search (Escape)");

    let panel = gtk::Box::new(gtk::Orientation::Horizontal, 8);
    panel.add_css_class("teral-search-panel");
    panel.append(&entry);
    panel.append(&matches);
    panel.append(&close);

    let root = gtk::Revealer::new();
    root.add_css_class("teral-search-overlay");
    root.set_child(Some(&panel));
    root.set_transition_type(gtk::RevealerTransitionType::SlideDown);
    root.set_transition_duration(160);
    root.set_reveal_child(false);
    root.set_halign(gtk::Align::Center);
    root.set_valign(gtk::Align::Start);

    Search {
        root,
        entry,
        matches,
        close,
    }
}

pub fn connect(app: &App) {
    app.widgets.search_overlay.entry.connect_search_changed({
        let app = Rc::clone(app);
        move |entry| {
            *app.state.query.borrow_mut() = entry.text().to_string();
            app.apply_filter();
        }
    });

    // Enter opens the first match, the way a filter box is expected to behave.
    app.widgets.search_overlay.entry.connect_activate({
        let app = Rc::clone(app);
        move |_| {
            if app.state.selection.n_items() > 0 {
                app.state.selection.select_item(0, true);
                super::window::activate_selection(&app);
            }
        }
    });

    app.widgets.search_overlay.close.connect_clicked({
        let app = Rc::clone(app);
        move |_| close(&app)
    });

    let keys = gtk::EventControllerKey::new();
    keys.connect_key_pressed({
        let app = Rc::clone(app);
        move |_, key, _, _| {
            if key == gtk::gdk::Key::Escape {
                close(&app);
                gtk::glib::Propagation::Stop
            } else {
                gtk::glib::Propagation::Proceed
            }
        }
    });
    app.widgets.search_overlay.entry.add_controller(keys);
}

/// Reveal the panel and put the cursor in it.
pub fn open(app: &App) {
    app.widgets.search_overlay.root.set_reveal_child(true);
    sync_toggle(app, true);
    app.widgets.search_overlay.entry.grab_focus();
}

/// Hide the panel and clear whatever filter it applied.
pub fn close(app: &App) {
    app.widgets.search_overlay.root.set_reveal_child(false);
    sync_toggle(app, false);

    if !app.state.query.borrow().is_empty() {
        app.state.query.borrow_mut().clear();
        app.widgets.search_overlay.entry.set_text("");
        app.apply_filter();
    }
    super::window::focus_file_view(app);
}

pub fn is_open(app: &App) -> bool {
    app.widgets.search_overlay.root.reveals_child()
}

/// Show how many entries the current filter matches.
pub fn update_matches(app: &App, visible: usize, total: usize) {
    let text = if app.state.query.borrow().is_empty() {
        String::new()
    } else {
        format!("{visible} of {total}")
    };
    app.widgets.search_overlay.matches.set_text(&text);
}

fn sync_toggle(app: &App, active: bool) {
    app.state.updating.set(true);
    app.widgets.search_button.set_active(active);
    app.state.updating.set(false);
}
