//! Left navigation: XDG locations, mounted devices and pinned folders.

use super::{App, section_title};
use crate::files::format_size;
use crate::files::scan;
use crate::places::{self, Device};
use gtk::glib;
use gtk::prelude::*;
use std::path::{Path, PathBuf};
use std::rc::Rc;

/// Containers the sidebar fills in once the application object exists.
pub struct Sidebar {
    pub root: gtk::Box,
    pub places: gtk::Box,
    pub devices: gtk::Box,
    pub pinned: gtk::Box,
    pub pin_drop: gtk::Label,
}

pub fn build(width: i32) -> Sidebar {
    let places = section_box();
    let devices = section_box();
    let pinned = section_box();

    let content = gtk::Box::new(gtk::Orientation::Vertical, 0);
    content.set_margin_top(6);
    content.set_margin_bottom(10);
    content.set_margin_start(8);
    content.set_margin_end(8);

    content.append(&section_title("NAVIGATION"));
    content.append(&places);

    content.append(&section_title("DEVICES"));
    content.append(&devices);

    let pinned_section = gtk::Box::new(gtk::Orientation::Vertical, 0);
    pinned_section.append(&section_title("PLACES"));
    pinned_section.append(&pinned);
    content.append(&pinned_section);

    let pin_drop = gtk::Label::new(Some("Drop a folder here to pin it"));
    pin_drop.add_css_class("teral-pin-target");
    pin_drop.set_wrap(true);
    pin_drop.set_justify(gtk::Justification::Center);
    pinned_section.append(&pin_drop);

    let scroller = gtk::ScrolledWindow::builder()
        .child(&content)
        .hscrollbar_policy(gtk::PolicyType::Never)
        .vexpand(true)
        .build();

    let root = gtk::Box::new(gtk::Orientation::Vertical, 0);
    root.add_css_class("teral-sidebar");
    // Child labels expand; the pane itself must not inherit that.
    root.set_hexpand(false);
    root.set_size_request(width, -1);
    root.append(&scroller);

    Sidebar {
        root,
        places,
        devices,
        pinned,
        pin_drop,
    }
}

fn section_box() -> gtk::Box {
    let container = gtk::Box::new(gtk::Orientation::Vertical, 1);
    container.set_margin_bottom(10);
    container
}

/// Populate every sidebar section. Safe to call again when mounts change.
pub fn connect(app: &App) {
    connect_pin_target(app);
    rebuild_places(app);
    rebuild_devices(app);
    rebuild_pinned(app);

    let monitor = gtk::gio::VolumeMonitor::get();
    for signal in ["mount-added", "mount-removed", "mount-changed"] {
        let app = Rc::clone(app);
        monitor.connect_local(signal, false, move |_| {
            rebuild_devices(&app);
            mark_active(&app, &app.current_dir());
            None
        });
    }
}

fn rebuild_places(app: &App) {
    clear(&app.widgets.places_box);
    for place in places::user_places() {
        let row = place_row(app, &place.label, &place.icon_name, &place.path);
        app.widgets.places_box.append(&row);
    }
}

fn rebuild_devices(app: &App) {
    clear(&app.widgets.devices_box);
    for device in places::devices() {
        app.widgets.devices_box.append(&device_row(app, &device));
    }
}

/// Dropping a folder on the PLACES area pins it instead of moving it.
fn connect_pin_target(app: &App) {
    let target = gtk::DropTarget::new(
        gtk::gdk::FileList::static_type(),
        gtk::gdk::DragAction::COPY,
    );
    let label = app.widgets.pin_drop.clone();

    target.connect_drop({
        let app = Rc::clone(app);
        let label = label.clone();
        move |_, value, _, _| {
            label.remove_css_class("drop-target");
            let Ok(files) = value.get::<gtk::gdk::FileList>() else {
                return false;
            };

            let mut pinned = 0usize;
            for path in files.files().iter().filter_map(gtk::gio::File::path) {
                if path.is_dir() && !app.is_pinned(&path) {
                    app.toggle_pin(&path);
                    pinned += 1;
                }
            }

            if pinned == 0 {
                app.set_message("Only folders can be pinned", false);
                return false;
            }
            app.set_message(
                &format!("Pinned {}", crate::files::item_count_label(pinned)),
                false,
            );
            true
        }
    });

    target.connect_enter({
        let label = label.clone();
        move |_, _, _| {
            label.add_css_class("drop-target");
            gtk::gdk::DragAction::COPY
        }
    });
    target.connect_leave({
        let label = label.clone();
        move |_| label.remove_css_class("drop-target")
    });

    label.add_controller(target);
}

/// Rebuild the pinned section, hiding the hint once folders are pinned.
pub fn rebuild_pinned(app: &App) {
    clear(&app.widgets.pinned_box);

    let pinned = app.state.pinned.borrow().clone();
    app.widgets.pin_drop.set_visible(pinned.is_empty());

    for path in pinned {
        let label = places::display_label(&path);
        let row = place_row(
            app,
            &label,
            crate::icons::ui(crate::icons::names::OPEN_FOLDER),
            &path,
        );
        app.widgets.pinned_box.append(&row);
    }
}

/// Highlight the sidebar row matching the current directory.
pub fn mark_active(app: &App, current: &Path) {
    for container in [
        &app.widgets.places_box,
        &app.widgets.devices_box,
        &app.widgets.pinned_box,
    ] {
        let mut child = container.first_child();
        while let Some(widget) = child {
            child = widget.next_sibling();
            let Some(button) = widget.downcast_ref::<gtk::Button>() else {
                continue;
            };
            let matches = button
                .tooltip_text()
                .is_some_and(|tooltip| Path::new(tooltip.as_str()) == current);
            if matches {
                button.add_css_class("active");
            } else {
                button.remove_css_class("active");
            }
        }
    }
}

fn place_row(app: &App, label: &str, icon_name: &str, path: &Path) -> gtk::Button {
    let content = gtk::Box::new(gtk::Orientation::Horizontal, 10);

    let image = gtk::Image::from_icon_name(icon_name);
    image.set_pixel_size(16);

    let text = gtk::Label::new(Some(label));
    text.set_xalign(0.0);
    text.set_hexpand(true);
    text.set_ellipsize(gtk::pango::EllipsizeMode::Middle);
    text.add_css_class("teral-place-label");

    content.append(&image);
    content.append(&text);

    let button = gtk::Button::new();
    button.set_child(Some(&content));
    button.add_css_class("teral-place");
    button.set_has_frame(false);
    button.set_tooltip_text(Some(&path.to_string_lossy()));

    let target = path.to_path_buf();
    button.connect_clicked({
        let app = Rc::clone(app);
        let target = target.clone();
        move |_| app.navigate(&target)
    });

    attach_context_menu(app, &button, target.clone());
    attach_drop_target(app, &button, target);
    button
}

fn device_row(app: &App, device: &Device) -> gtk::Button {
    let content = gtk::Box::new(gtk::Orientation::Horizontal, 10);

    let image = match device.icon.as_ref() {
        Some(icon) => gtk::Image::from_gicon(icon),
        None => gtk::Image::from_icon_name(crate::icons::ui(crate::icons::names::DRIVE)),
    };
    image.set_pixel_size(16);
    image.set_valign(gtk::Align::Start);
    image.set_margin_top(2);

    let column = gtk::Box::new(gtk::Orientation::Vertical, 3);
    column.set_hexpand(true);

    let name = gtk::Label::new(Some(&device.label));
    name.set_xalign(0.0);
    name.set_ellipsize(gtk::pango::EllipsizeMode::Middle);
    name.add_css_class("teral-place-label");

    let meter = gtk::LevelBar::new();
    meter.add_css_class("teral-meter");
    meter.set_mode(gtk::LevelBarMode::Continuous);
    meter.set_visible(false);

    let capacity = gtk::Label::new(None);
    capacity.set_xalign(0.0);
    capacity.add_css_class("teral-device-detail");
    capacity.set_visible(false);

    column.append(&name);
    column.append(&meter);
    column.append(&capacity);

    content.append(&image);
    content.append(&column);

    let button = gtk::Button::new();
    button.set_child(Some(&content));
    button.add_css_class("teral-place");
    button.set_has_frame(false);
    button.set_tooltip_text(Some(&device.path.to_string_lossy()));

    let target = device.path.clone();
    button.connect_clicked({
        let app = Rc::clone(app);
        let target = target.clone();
        move |_| app.navigate(&target)
    });

    attach_context_menu(app, &button, target.clone());
    attach_drop_target(app, &button, target.clone());

    // Capacity is queried asynchronously: a slow mount must not stall the sidebar.
    glib::spawn_future_local(async move {
        let Some((free, total)) = scan::filesystem_usage(&target).await else {
            return;
        };
        let used = total.saturating_sub(free);
        let fraction = (used as f64 / total as f64).clamp(0.0, 1.0);
        meter.set_value(fraction);
        if fraction > 0.9 {
            meter.add_css_class("critical");
        }
        meter.set_visible(true);
        capacity.set_text(&format!("{} / {}", format_size(used), format_size(total)));
        capacity.set_visible(true);
    });

    button
}

/// Sidebar rows accept dropped files, so a folder can be filed away without opening it.
fn attach_drop_target(app: &App, button: &gtk::Button, path: PathBuf) {
    let target = gtk::DropTarget::new(
        gtk::gdk::FileList::static_type(),
        gtk::gdk::DragAction::COPY | gtk::gdk::DragAction::MOVE,
    );

    target.connect_drop({
        let app = Rc::clone(app);
        let path = path.clone();
        let button = button.clone();
        move |_, value, _, _| {
            button.remove_css_class("drop-target");
            let Ok(files) = value.get::<gtk::gdk::FileList>() else {
                return false;
            };
            super::window::drop_files(&app, &files, &path)
        }
    });

    target.connect_enter({
        let button = button.clone();
        move |_, _, _| {
            button.add_css_class("drop-target");
            gtk::gdk::DragAction::COPY
        }
    });

    target.connect_leave({
        let button = button.clone();
        move |_| button.remove_css_class("drop-target")
    });

    button.add_controller(target);
}

/// Right-click on any sidebar row offers pinning and a terminal.
fn attach_context_menu(app: &App, button: &gtk::Button, path: PathBuf) {
    let popover = gtk::Popover::new();
    popover.add_css_class("teral-popover");
    popover.set_parent(button);
    popover.set_position(gtk::PositionType::Right);

    let content = gtk::Box::new(gtk::Orientation::Vertical, 2);
    content.set_width_request(190);

    let pin = super::header::menu_item(crate::icons::ui(crate::icons::names::PIN), "Pin");
    let terminal = super::header::menu_item(
        crate::icons::ui(crate::icons::names::TERMINAL),
        "Open Terminal Here",
    );
    content.append(&pin);
    content.append(&terminal);
    popover.set_child(Some(&content));

    pin.connect_clicked({
        let app = Rc::clone(app);
        let path = path.clone();
        let popover = popover.clone();
        move |_| {
            popover.popdown();
            app.toggle_pin(&path);
        }
    });

    terminal.connect_clicked({
        let app = Rc::clone(app);
        let path = path.clone();
        let popover = popover.clone();
        move |_| {
            popover.popdown();
            super::window::open_terminal(&app, &path);
        }
    });

    let gesture = gtk::GestureClick::new();
    gesture.set_button(gtk::gdk::BUTTON_SECONDARY);
    gesture.connect_pressed({
        let app = Rc::clone(app);
        let popover = popover.clone();
        let path = path.clone();
        move |_, _, _, _| {
            let pinned = app.is_pinned(&path);
            if let Some(row) = pin.child().and_downcast::<gtk::Box>()
                && let Some(label) = row.last_child().and_downcast::<gtk::Label>()
            {
                label.set_text(if pinned { "Unpin" } else { "Pin" });
            }
            popover.popup();
        }
    });
    button.add_controller(gesture);

    // The popover is parented to the button, so it must be released with it.
    button.connect_destroy(move |_| popover.unparent());
}

fn clear(container: &gtk::Box) {
    while let Some(child) = container.first_child() {
        container.remove(&child);
    }
}
