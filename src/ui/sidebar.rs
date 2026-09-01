//! Left navigation: XDG locations, mounted devices and pinned folders.

use super::{App, AppInner, section_title};
use crate::files::format_size;
use crate::files::scan;
use crate::places::{self, Device};
use gtk::gio;
use gtk::glib;
use gtk::prelude::*;
use std::path::{Path, PathBuf};
use std::rc::{Rc, Weak};

/// Containers the sidebar fills in once the application object exists.
pub struct Sidebar {
    pub root: gtk::Box,
    pub places: gtk::Box,
    pub devices: gtk::Box,
    pub pinned: gtk::Box,
    pub pin_drop: gtk::Label,
    pub tags: gtk::Box,
    pub add_tag: gtk::Button,
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

    // One row does both jobs: it is the "No bookmarks" placeholder while the section is
    // empty, and turns into the drop hint in that same place during a drag. With
    // bookmarks present it stays hidden until a folder is dragged over.
    let pin_drop = gtk::Label::new(Some("No bookmarks"));
    pin_drop.set_xalign(0.0);
    pin_drop.add_css_class("teral-bookmarks-empty");

    let bookmarks = gtk::Box::new(gtk::Orientation::Vertical, 0);
    bookmarks.add_css_class("teral-bookmarks");
    // The hint sits directly under the heading, where the first bookmark goes: that is
    // where the eye already is, and where the placeholder it replaces has always been.
    bookmarks.append(&section_title("BOOKMARKS"));
    bookmarks.append(&pin_drop);
    bookmarks.append(&pinned);
    content.append(&bookmarks);

    let tags = section_box();
    let add_tag = super::icon_button(crate::icons::ui(crate::icons::names::ADD), "Create a tag");
    content.append(&section_header("TAGS", &add_tag));
    content.append(&tags);

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
        tags,
        add_tag,
    }
}

/// A section heading with a control on its right.
fn section_header(title: &str, button: &impl IsA<gtk::Widget>) -> gtk::Box {
    let heading = section_title(title);
    heading.set_hexpand(true);

    let row = gtk::Box::new(gtk::Orientation::Horizontal, 4);
    row.set_margin_end(6);
    row.append(&heading);
    row.append(button);
    row
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
    rebuild_tags(app);

    app.widgets.add_tag.connect_clicked({
        let app = Rc::clone(app);
        move |_| super::dialogs::edit_tag(&app, None)
    });

    let monitor = &app.state.volume_monitor;
    let handler = monitor.connect_mount_added({
        let app = Rc::downgrade(app);
        move |_, _| devices_changed(&app)
    });
    app.state.volume_handlers.borrow_mut().push(handler);
    let handler = monitor.connect_mount_removed({
        let app = Rc::downgrade(app);
        move |_, _| devices_changed(&app)
    });
    app.state.volume_handlers.borrow_mut().push(handler);
    let handler = monitor.connect_mount_changed({
        let app = Rc::downgrade(app);
        move |_, _| devices_changed(&app)
    });
    app.state.volume_handlers.borrow_mut().push(handler);
    // A removable device may announce its volume before GVfs publishes the mount.
    // Rebuilding for both layers keeps labels and mount state current throughout the
    // hot-plug sequence instead of waiting for another application restart.
    let handler = monitor.connect_volume_added({
        let app = Rc::downgrade(app);
        move |_, _| devices_changed(&app)
    });
    app.state.volume_handlers.borrow_mut().push(handler);
    let handler = monitor.connect_volume_removed({
        let app = Rc::downgrade(app);
        move |_, _| devices_changed(&app)
    });
    app.state.volume_handlers.borrow_mut().push(handler);
    let handler = monitor.connect_volume_changed({
        let app = Rc::downgrade(app);
        move |_, _| devices_changed(&app)
    });
    app.state.volume_handlers.borrow_mut().push(handler);
}

fn devices_changed(weak: &Weak<AppInner>) {
    let Some(app) = weak.upgrade() else {
        return;
    };
    if app.state.device_refresh_queued.replace(true) {
        return;
    }
    glib::idle_add_local_once(move || {
        app.state.device_refresh_queued.set(false);
        rebuild_devices(&app);
        mark_active(&app, &app.current_dir());
        // A disk that has just appeared may carry its own trash, and one that has gone
        // takes its trash with it. The filesystem probes remain off the GTK thread.
        let for_trash = Rc::clone(&app);
        crate::files::ops::refresh_trash_dirs(move || rebuild_places(&for_trash));
    });
}

pub fn rebuild_places(app: &App) {
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

/// Dropping a folder on the bookmarks area bookmarks it instead of moving it.
///
/// The hint only appears once a drag reaches the section, so it never occupies the
/// sidebar during ordinary browsing.
fn connect_pin_target(app: &App) {
    let target = gtk::DropTarget::new(
        gtk::gdk::FileList::static_type(),
        gtk::gdk::DragAction::COPY,
    );

    target.connect_drop({
        let app = Rc::clone(app);
        move |_target, value, _, _| {
            show_drop_hint(&app, false);
            let Ok(files) = value.get::<gtk::gdk::FileList>() else {
                return false;
            };

            let paths: Vec<PathBuf> = files
                .files()
                .iter()
                .filter_map(gtk::gio::File::path)
                .collect();
            if paths.is_empty() {
                app.set_message("Only local folders can be bookmarked", false);
                return false;
            }

            let weak = Rc::downgrade(&app);
            glib::spawn_future_local(async move {
                let mut pinned = 0usize;
                for path in paths {
                    let info = gio::File::for_path(&path)
                        .query_info_future(
                            gio::FILE_ATTRIBUTE_STANDARD_TYPE,
                            gio::FileQueryInfoFlags::NONE,
                            glib::Priority::DEFAULT,
                        )
                        .await;
                    let Some(app) = weak.upgrade() else {
                        return;
                    };
                    if info.is_ok_and(|info| info.file_type() == gio::FileType::Directory)
                        && !app.is_pinned(&path)
                    {
                        app.toggle_pin(&path);
                        pinned += 1;
                    }
                }
                let Some(app) = weak.upgrade() else {
                    return;
                };
                if pinned == 0 {
                    app.set_message("Only folders can be bookmarked", false);
                } else {
                    app.set_message(
                        &format!("Bookmarked {}", crate::files::item_count_label(pinned)),
                        false,
                    );
                }
            });
            true
        }
    });

    target.connect_enter({
        let app = Rc::clone(app);
        move |_, _, _| {
            show_drop_hint(&app, true);
            gtk::gdk::DragAction::COPY
        }
    });
    target.connect_leave({
        let app = Rc::clone(app);
        move |_| show_drop_hint(&app, false)
    });

    // The target lives on the whole section so the hint appears as soon as a drag
    // arrives, whether or not the section already holds bookmarks.
    app.widgets
        .pin_drop
        .parent()
        .expect("the hint always sits inside the bookmarks section")
        .add_controller(target);
}

/// Rebuild the tag list, and the row of controls each tag reveals on hover.
pub fn rebuild_tags(app: &App) {
    clear(&app.widgets.tags);

    for tag in crate::tags::current().tags {
        let dot = gtk::Label::new(Some("●"));
        dot.add_css_class("teral-tag-dot");
        super::apply_color(&dot, &tag.color);

        let label = gtk::Label::new(Some(&tag.name));
        label.set_xalign(0.0);
        label.set_hexpand(true);
        label.set_ellipsize(gtk::pango::EllipsizeMode::End);
        label.add_css_class("teral-place-label");

        let count = gtk::Label::new(Some(&tag.paths.len().to_string()));
        count.add_css_class("teral-place-count");

        let edit = super::icon_button(crate::icons::ui(crate::icons::names::RENAME), "Edit tag");
        let delete =
            super::icon_button(crate::icons::ui(crate::icons::names::DELETE), "Delete tag");

        let controls = gtk::Box::new(gtk::Orientation::Horizontal, 2);
        controls.add_css_class("teral-tag-controls");
        controls.append(&edit);
        controls.append(&delete);
        controls.set_visible(false);

        let content = gtk::Box::new(gtk::Orientation::Horizontal, 10);
        content.append(&dot);
        content.append(&label);
        content.append(&count);
        content.append(&controls);

        // A button inside a button never receives clicks in GTK, so the row is a box
        // with its own gesture and the hover controls are ordinary siblings.
        let row = gtk::Box::new(gtk::Orientation::Horizontal, 0);
        row.add_css_class("teral-place");
        row.add_css_class("teral-tag-row");
        row.append(&content);
        content.set_hexpand(true);

        let hover = gtk::EventControllerMotion::new();
        hover.connect_enter({
            let controls = controls.clone();
            let count = count.clone();
            move |_, _, _| {
                controls.set_visible(true);
                count.set_visible(false);
            }
        });
        hover.connect_leave({
            let controls = controls.clone();
            let count = count.clone();
            move |_| {
                controls.set_visible(false);
                count.set_visible(true);
            }
        });
        row.add_controller(hover);

        let open = gtk::GestureClick::new();
        open.set_button(gtk::gdk::BUTTON_PRIMARY);
        open.connect_released({
            let app = Rc::clone(app);
            let name = tag.name.clone();
            move |_, _, _, _| app.show_tag(&name)
        });
        row.add_controller(open);

        edit.connect_clicked({
            let app = Rc::clone(app);
            let name = tag.name.clone();
            move |_| {
                let app = Rc::clone(&app);
                let name = name.clone();
                super::defer(move || super::dialogs::edit_tag(&app, Some(&name)));
            }
        });

        delete.connect_clicked({
            let app = Rc::clone(app);
            let name = tag.name.clone();
            move |_| {
                let app = Rc::clone(&app);
                let name = name.clone();
                super::defer(move || super::dialogs::confirm_delete_tag(&app, &name));
            }
        });

        app.widgets.tags.append(&row);
    }

    let active = app.state.tag_view.borrow().clone();
    mark_active_tag(app, active.as_deref());
}

/// Highlight the tag whose contents are on screen.
pub fn mark_active_tag(app: &App, active: Option<&str>) {
    let mut child = app.widgets.tags.first_child();
    let mut index = 0usize;
    let tags = crate::tags::current().tags;

    while let Some(widget) = child {
        child = widget.next_sibling();
        let matches = tags
            .get(index)
            .zip(active)
            .is_some_and(|(tag, active)| tag.matches(active));
        if matches {
            widget.add_css_class("active");
        } else {
            widget.remove_css_class("active");
        }
        index += 1;
    }
}

/// Turn the bookmarks placeholder row into the drop hint, and back again.
///
/// The same row carries both states, so the hint appears exactly where "No bookmarks"
/// was rather than beside it, and disappears again with the section still empty.
fn show_drop_hint(app: &App, dragging: bool) {
    let label = &app.widgets.pin_drop;
    let empty = app.state.pinned.borrow().is_empty();

    if dragging {
        label.set_text("Drop to bookmark");
        label.remove_css_class("teral-bookmarks-empty");
        label.add_css_class("teral-pin-target");
        label.add_css_class("drop-target");
        label.set_visible(true);
    } else {
        label.set_text("No bookmarks");
        label.remove_css_class("drop-target");
        label.remove_css_class("teral-pin-target");
        label.add_css_class("teral-bookmarks-empty");
        label.set_visible(empty);
    }
}

/// Rebuild the bookmarks section, showing a placeholder while it is empty.
pub fn rebuild_pinned(app: &App) {
    clear(&app.widgets.pinned_box);

    let pinned = app.state.pinned.borrow().clone();
    show_drop_hint(app, false);

    for path in pinned {
        let label = places::bookmark_label(&path);
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

    if let Some(target) = device.path.clone() {
        button.set_tooltip_text(Some(&target.to_string_lossy()));
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
    } else if let Some(volume) = device.volume.clone() {
        button.set_tooltip_text(Some(&format!("Mount {}", device.label)));
        button.set_sensitive(volume.can_mount());
        button.connect_clicked({
            let app = Rc::clone(app);
            move |button| {
                button.set_sensitive(false);
                let operation = gtk::MountOperation::new(Some(&app.widgets.window));
                let mounting =
                    volume.mount_future(gtk::gio::MountMountFlags::NONE, Some(&operation));
                let app = Rc::clone(&app);
                let volume = volume.clone();
                let button = button.clone();
                glib::spawn_future_local(async move {
                    match mounting.await {
                        Ok(()) => {
                            if let Some(path) = volume
                                .get_mount()
                                .and_then(|mount| mount.root().path())
                            {
                                app.navigate(&path);
                            } else {
                                app.show_error("The device mounted without a browsable local path");
                            }
                        }
                        Err(error) => {
                            button.set_sensitive(volume.can_mount());
                            app.show_error(&format!("Could not mount the device: {error}"));
                        }
                    }
                });
            }
        });
    }

    button
}

/// Sidebar rows accept dropped files, so a folder can be filed away without opening it.
fn attach_drop_target(app: &App, button: &gtk::Button, path: PathBuf) {
    let target = gtk::DropTarget::new(
        gtk::gdk::FileList::static_type(),
        gtk::gdk::DragAction::COPY | gtk::gdk::DragAction::MOVE | gtk::gdk::DragAction::LINK,
    );

    target.connect_drop({
        let app = Rc::clone(app);
        let path = path.clone();
        let button = button.clone();
        move |target, value, _, _| {
            button.remove_css_class("drop-target");
            let Ok(files) = value.get::<gtk::gdk::FileList>() else {
                return false;
            };
            let action = super::window::drop_action(target);
            super::window::drop_files(&app, &files, &path, action)
        }
    });

    target.connect_enter({
        let app = Rc::clone(app);
        let button = button.clone();
        move |target, _, _| {
            button.add_css_class("drop-target");
            let action = super::window::drop_action(target);
            super::window::show_drop_action(&app, action);
            action
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
    popover.set_has_arrow(false);
    popover.set_halign(gtk::Align::Start);
    popover.set_position(gtk::PositionType::Bottom);

    let content = gtk::Box::new(gtk::Orientation::Vertical, 2);
    content.set_width_request(190);

    let new_tab = super::header::menu_item(
        crate::icons::ui(crate::icons::names::ADD),
        "Open in New Tab",
    );
    let new_window = super::header::menu_item(
        crate::icons::ui(crate::icons::names::WINDOW),
        "Open in New Window",
    );
    let pin = super::header::menu_item(crate::icons::ui(crate::icons::names::PIN), "Bookmark");
    let rename_bookmark = super::header::menu_item(
        crate::icons::ui(crate::icons::names::RENAME),
        "Rename Bookmark",
    );
    let move_up = super::header::menu_item(crate::icons::ui(crate::icons::names::UP), "Move Up");
    let move_down = super::header::menu_item(
        crate::icons::ui(crate::icons::names::DOWN),
        "Move Down",
    );
    let terminal = super::header::menu_item(
        crate::icons::ui(crate::icons::names::TERMINAL),
        "Open Terminal Here",
    );
    let empty_trash =
        super::header::menu_item(crate::icons::ui(crate::icons::names::TRASH), "Empty Trash");
    let is_trash = crate::files::ops::is_in_trash(&path.join("x"));
    empty_trash.set_visible(is_trash);
    pin.set_visible(!is_trash);
    terminal.set_visible(!is_trash);

    content.append(&new_tab);
    content.append(&new_window);
    content.append(&pin);
    content.append(&rename_bookmark);
    content.append(&move_up);
    content.append(&move_down);
    content.append(&terminal);
    content.append(&empty_trash);
    popover.set_child(Some(&content));

    new_tab.connect_clicked({
        let app = Rc::clone(app);
        let path = path.clone();
        let popover = popover.clone();
        move |_| {
            popover.popdown();
            super::window::open_in_new_tab(&app, &path);
        }
    });

    new_window.connect_clicked({
        let app = Rc::clone(app);
        let path = path.clone();
        let popover = popover.clone();
        move |_| {
            popover.popdown();
            super::window::open_in_new_window(&app, &path);
        }
    });

    empty_trash.connect_clicked({
        let app = Rc::clone(app);
        let popover = popover.clone();
        move |_| {
            popover.popdown();
            super::window::empty_trash(&app);
        }
    });

    pin.connect_clicked({
        let app = Rc::clone(app);
        let path = path.clone();
        let popover = popover.clone();
        move |_| {
            popover.popdown();
            app.toggle_pin(&path);
        }
    });

    rename_bookmark.connect_clicked({
        let app = Rc::clone(app);
        let path = path.clone();
        let popover = popover.clone();
        move |_| {
            popover.popdown();
            super::dialogs::prompt_text(
                &app,
                "Bookmark label",
                "Save",
                &places::bookmark_label(&path),
                {
                    let path = path.clone();
                    move |app, label| app.label_pin(&path, label)
                },
            );
        }
    });
    move_up.connect_clicked({
        let app = Rc::clone(app);
        let path = path.clone();
        let popover = popover.clone();
        move |_| {
            popover.popdown();
            app.reorder_pin(&path, -1);
        }
    });
    move_down.connect_clicked({
        let app = Rc::clone(app);
        let path = path.clone();
        let popover = popover.clone();
        move |_| {
            popover.popdown();
            app.reorder_pin(&path, 1);
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
        move |_, _, x, y| {
            // Anchor on the pointer, not on the row, so the menu opens where the click
            // happened instead of against the sidebar's edge.
            popover.set_pointing_to(Some(&gtk::gdk::Rectangle::new(x as i32, y as i32, 1, 1)));
            let pinned = app.is_pinned(&path);
            rename_bookmark.set_visible(pinned);
            move_up.set_visible(pinned);
            move_down.set_visible(pinned);
            if let Some(row) = pin.child().and_downcast::<gtk::Box>()
                && let Some(label) = row.last_child().and_downcast::<gtk::Label>()
            {
                label.set_text(if pinned {
                    "Remove Bookmark"
                } else {
                    "Bookmark"
                });
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
