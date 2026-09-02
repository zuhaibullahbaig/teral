//! Folder search.
//!
//! The field lives in the toolbar, next to the view controls and styled like the
//! breadcrumb, and it opens the moment you start typing in the file list.

use super::App;
use crate::icons;
use gtk::prelude::*;
use std::rc::Rc;

/// Widgets of the toolbar search field.
pub struct Search {
    pub root: gtk::Revealer,
    pub entry: gtk::SearchEntry,
    pub matches: gtk::Label,
}

pub fn build() -> Search {
    let entry = gtk::SearchEntry::new();
    entry.add_css_class("teral-search-entry");
    entry.set_hexpand(true);
    entry.set_width_chars(16);
    entry.set_max_width_chars(24);
    entry.set_placeholder_text(Some("Filter this folder"));

    let matches = gtk::Label::new(None);
    matches.add_css_class("teral-search-matches");

    let close = super::icon_button(icons::ui(icons::names::CLOSE), "Close search (Escape)");

    let field = gtk::Box::new(gtk::Orientation::Horizontal, 4);
    field.add_css_class("teral-search-field");
    field.set_valign(gtk::Align::Center);
    field.append(&entry);
    field.append(&matches);
    field.append(&close);

    let root = gtk::Revealer::new();
    root.set_child(Some(&field));
    root.set_transition_type(gtk::RevealerTransitionType::SlideRight);
    root.set_transition_duration(140);
    root.set_reveal_child(false);
    root.set_valign(gtk::Align::Center);

    // The close button is only reachable through this struct's root, so it is wired
    // here rather than being carried around as another field.
    close.connect_clicked({
        let root = root.clone();
        move |_| root.set_reveal_child(false)
    });

    Search {
        root,
        entry,
        matches,
    }
}

pub fn connect(app: &App) {
    app.widgets.search.entry.connect_search_changed({
        let app = Rc::clone(app);
        move |entry| {
            if app.state.updating.get() {
                return;
            }
            *app.state.query.borrow_mut() = entry.text().to_string();
            app.queue_filter();
        }
    });

    // Enter opens the first match, the way a filter box is expected to behave.
    app.widgets.search.entry.connect_activate({
        let app = Rc::clone(app);
        move |_| {
            if app.state.selection.n_items() > 0 {
                app.state.selection.select_item(0, true);
                super::window::activate_selection(&app);
            }
        }
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
    app.widgets.search.entry.add_controller(keys);

    // Closing with the field's own button has to clear the filter too.
    app.widgets.search.root.connect_child_revealed_notify({
        let app = Rc::clone(app);
        move |revealer| {
            if !revealer.reveals_child() {
                close(&app);
            }
        }
    });
}

/// Reveal the field and put the cursor in it.
pub fn open(app: &App) {
    super::window::show_files_workspace(app);
    app.widgets.search.root.set_reveal_child(true);
    sync_toggle(app, true);
    super::window::apply_responsive_layout(app);
    app.widgets.search.entry.grab_focus();
}

/// Append a typed character, opening the field first if it is not showing yet.
///
/// Focus moves to the entry as soon as the field opens, so the entry itself handles
/// every character after this one; appending here covers the keystrokes that arrive
/// while focus is still on its way.
pub fn type_ahead(app: &App, character: char) {
    if !is_open(app) {
        app.widgets.search.root.set_reveal_child(true);
        sync_toggle(app, true);
        super::window::apply_responsive_layout(app);
    }

    let entry = &app.widgets.search.entry;
    let mut text = entry.text().to_string();
    text.push(character);
    entry.set_text(&text);
    entry.grab_focus();
    entry.set_position(-1);
}

/// Hide the field and clear whatever filter it applied.
pub fn close(app: &App) {
    let was_open = app.widgets.search.root.reveals_child();
    app.widgets.search.root.set_reveal_child(false);
    sync_toggle(app, false);
    super::window::apply_responsive_layout(app);

    if !app.state.query.borrow().is_empty() {
        app.state.query.borrow_mut().clear();
        app.state.updating.set(true);
        app.widgets.search.entry.set_text("");
        app.state.updating.set(false);
        if let Some(source) = app.state.filter_source.replace(None) {
            source.remove();
        }
        app.apply_filter();
    }

    if was_open {
        super::window::focus_file_view(app);
    }
}

pub fn is_open(app: &App) -> bool {
    app.widgets.search.root.reveals_child()
}

/// Show how many entries the current filter matches.
pub fn update_matches(app: &App, visible: usize, total: usize) {
    let text = if app.state.query.borrow().is_empty() {
        String::new()
    } else {
        format!("{visible}/{total}")
    };
    app.widgets.search.matches.set_text(&text);
}

fn sync_toggle(app: &App, active: bool) {
    app.state.updating.set(true);
    app.widgets.search_button.set_active(active);
    app.state.updating.set(false);
}
