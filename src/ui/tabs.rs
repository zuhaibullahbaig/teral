//! The tab strip.
//!
//! Each tab keeps its own location and its own back/forward history. The strip hides
//! itself while only one tab is open, so the single-tab case stays uncluttered.

use super::App;
use crate::icons;
use crate::places;
use gtk::gdk;
use gtk::prelude::*;
use std::rc::Rc;

/// Widgets of the tab strip.
pub struct Tabs {
    pub root: gtk::Box,
    pub strip: gtk::Box,
    pub add: gtk::Button,
}

pub fn build() -> Tabs {
    let strip = gtk::Box::new(gtk::Orientation::Horizontal, 4);
    strip.set_hexpand(true);

    let scroller = gtk::ScrolledWindow::builder()
        .child(&strip)
        .hscrollbar_policy(gtk::PolicyType::External)
        .vscrollbar_policy(gtk::PolicyType::Never)
        .hexpand(true)
        .build();

    let add = super::icon_button(icons::ui(icons::names::ADD), "New tab (Ctrl+T)");

    let root = gtk::Box::new(gtk::Orientation::Horizontal, 6);
    root.add_css_class("teral-tab-bar");
    root.append(&scroller);
    root.append(&add);
    root.set_visible(false);

    Tabs { root, strip, add }
}

pub fn connect(app: &App) {
    app.widgets.tabs.add.connect_clicked({
        let app = Rc::clone(app);
        move |_| {
            let current = app.current_dir();
            app.open_tab(current);
        }
    });
}

/// Redraw the strip from the current tab list.
pub fn rebuild(app: &App) {
    let strip = &app.widgets.tabs.strip;
    while let Some(child) = strip.first_child() {
        strip.remove(&child);
    }

    let tabs = app.state.tabs.borrow().clone();
    app.widgets.tabs.root.set_visible(tabs.len() > 1);
    if tabs.len() < 2 {
        return;
    }

    let active = app.state.active_tab.get();
    let closable = tabs.len() > 1;

    for (index, tab) in tabs.iter().enumerate() {
        // Every tab reads its own stored path, including the active one: using the
        // live directory here made a tab briefly show the previous tab's name while
        // the new folder was still loading.
        let path = tab.path.clone();

        let label = gtk::Label::new(Some(&places::display_label(&path)));
        label.set_ellipsize(gtk::pango::EllipsizeMode::Middle);
        label.set_max_width_chars(18);
        label.add_css_class("teral-tab-label");

        // A button inside a button never receives clicks in GTK, so the tab is a box
        // with its own gesture and the close button is a real sibling.
        let close = gtk::Button::from_icon_name(icons::ui(icons::names::CLOSE));
        close.add_css_class("teral-tab-close");
        close.set_has_frame(false);
        close.set_valign(gtk::Align::Center);
        close.set_tooltip_text(Some("Close tab (Ctrl+W)"));
        close.set_visible(closable);

        let tab_box = gtk::Box::new(gtk::Orientation::Horizontal, 6);
        tab_box.add_css_class("teral-tab");
        tab_box.set_tooltip_text(Some(&path.to_string_lossy()));
        tab_box.append(&label);
        tab_box.append(&close);
        if index == active {
            tab_box.add_css_class("active");
        }

        close.connect_clicked({
            let app = Rc::clone(app);
            move |_| app.close_tab(index)
        });

        let activate = gtk::GestureClick::new();
        activate.set_button(gdk::BUTTON_PRIMARY);
        activate.connect_released({
            let app = Rc::clone(app);
            move |_, _, _, _| app.activate_tab(index)
        });
        tab_box.add_controller(activate);

        // Middle-click closes, the way every other tabbed application behaves.
        let middle = gtk::GestureClick::new();
        middle.set_button(gdk::BUTTON_MIDDLE);
        middle.connect_pressed({
            let app = Rc::clone(app);
            move |_, _, _, _| app.close_tab(index)
        });
        tab_box.add_controller(middle);

        strip.append(&tab_box);
    }
}
