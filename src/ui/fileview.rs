//! The central file area: a polished grid and a dense list, sharing one selection.

use super::App;
use crate::files::{FileEntry, format_size, format_time};
use crate::icons;
use gtk::gdk;
use gtk::glib;
use gtk::prelude::*;
use std::cell::RefCell;
use std::collections::HashMap;
use std::rc::Rc;

/// Signal handlers created while a list item is bound, disconnected on unbind.
type Bindings = Rc<RefCell<HashMap<gtk::ListItem, (FileEntry, Vec<glib::SignalHandlerId>)>>>;

/// Install the grid factory, the list columns and every view interaction.
pub fn connect(app: &App) {
    refresh_grid_factory(app);
    build_columns(app);

    app.widgets.grid.connect_activate({
        let app = Rc::clone(app);
        move |_, position| activate(&app, position)
    });
    app.widgets.list.connect_activate({
        let app = Rc::clone(app);
        move |_, position| activate(&app, position)
    });

    app.state.selection.connect_selection_changed({
        let app = Rc::clone(app);
        move |_, _, _| {
            if !app.state.updating.get() {
                app.update_details();
                app.update_status();
            }
        }
    });
}

fn activate(app: &App, position: u32) {
    let Some(entry) = app
        .state
        .selection
        .item(position)
        .and_downcast::<FileEntry>()
    else {
        return;
    };

    if entry.is_directory() {
        app.navigate(entry.path());
    } else {
        super::window::open_entry(app, &entry);
    }
}

// -------------------------------------------------------------------- grid ----

/// Rebuild every grid cell, for example after the zoom slider moves.
pub fn refresh_grid_factory(app: &App) {
    app.widgets.grid.set_factory(Some(&grid_factory(app)));
}

fn grid_factory(app: &App) -> gtk::SignalListItemFactory {
    let factory = gtk::SignalListItemFactory::new();
    let bindings: Bindings = Rc::new(RefCell::new(HashMap::new()));

    factory.connect_setup({
        let app = Rc::clone(app);
        move |_, item| {
            let Some(item) = item.downcast_ref::<gtk::ListItem>() else {
                return;
            };
            item.set_child(Some(&build_grid_item(&app, item)));
        }
    });

    factory.connect_bind({
        let bindings = Rc::clone(&bindings);
        move |_, item| {
            let Some(item) = item.downcast_ref::<gtk::ListItem>() else {
                return;
            };
            bind_grid_item(item, &bindings);
        }
    });

    factory.connect_unbind(move |_, item| {
        if let Some(item) = item.downcast_ref::<gtk::ListItem>()
            && let Some((entry, handlers)) = bindings.borrow_mut().remove(item)
        {
            for handler in handlers {
                entry.disconnect(handler);
            }
        }
    });

    factory
}

/// Widgets inside one grid cell, looked up again on bind.
struct GridItem {
    tile: gtk::Box,
    icon: gtk::Image,
    picture: gtk::Picture,
    name: gtk::Label,
    subtitle: gtk::Label,
}

fn build_grid_item(app: &App, item: &gtk::ListItem) -> gtk::Widget {
    let icon_size = app.state.icon_size.get();
    let tile_width = icon_size * 2 + 28;
    let tile_height = icon_size * 2 + 14;

    let icon = gtk::Image::new();
    icon.set_pixel_size(icon_size);
    icon.set_hexpand(true);
    icon.set_vexpand(true);

    let picture = gtk::Picture::new();
    picture.set_content_fit(gtk::ContentFit::Cover);
    picture.set_hexpand(true);
    picture.set_vexpand(true);
    picture.set_visible(false);

    let tile = gtk::Box::new(gtk::Orientation::Horizontal, 0);
    tile.add_css_class("teral-tile");
    tile.set_overflow(gtk::Overflow::Hidden);
    tile.set_size_request(tile_width, tile_height);
    tile.append(&icon);
    tile.append(&picture);

    let badge = gtk::Image::from_icon_name(icons::ui(icons::names::SELECTED));
    badge.add_css_class("teral-selection-badge");
    badge.set_pixel_size(14);
    badge.set_halign(gtk::Align::End);
    badge.set_valign(gtk::Align::Start);
    badge.set_margin_top(6);
    badge.set_margin_end(6);

    let overlay = gtk::Overlay::new();
    overlay.set_child(Some(&tile));
    overlay.add_overlay(&badge);

    let name = gtk::Label::new(None);
    name.add_css_class("teral-item-name");
    name.set_ellipsize(gtk::pango::EllipsizeMode::Middle);
    name.set_max_width_chars(1);
    name.set_justify(gtk::Justification::Center);

    let subtitle = gtk::Label::new(None);
    subtitle.add_css_class("teral-item-subtitle");
    subtitle.set_ellipsize(gtk::pango::EllipsizeMode::End);
    subtitle.set_max_width_chars(1);

    let root = gtk::Box::new(gtk::Orientation::Vertical, 5);
    root.set_halign(gtk::Align::Center);
    root.set_size_request(tile_width, -1);
    root.append(&overlay);
    root.append(&name);
    root.append(&subtitle);

    // The badge follows the list item's own selection state for its whole lifetime.
    item.bind_property("selected", &badge, "visible")
        .sync_create()
        .build();

    attach_context_gesture(app, &root, item);

    let widgets = GridItem {
        tile,
        icon,
        picture,
        name,
        subtitle,
    };
    GRID_ITEMS.with_borrow_mut(|items| items.insert(root.clone(), widgets));
    root.connect_destroy(|root| {
        GRID_ITEMS.with_borrow_mut(|items| items.remove(root));
    });

    root.upcast()
}

thread_local! {
    /// Lets `bind` find the widgets built in `setup` without unsafe object data.
    static GRID_ITEMS: RefCell<HashMap<gtk::Box, GridItem>> = RefCell::new(HashMap::new());
}

fn bind_grid_item(item: &gtk::ListItem, bindings: &Bindings) {
    let Some(root) = item.child().and_downcast::<gtk::Box>() else {
        return;
    };
    let Some(entry) = item.item().and_downcast::<FileEntry>() else {
        return;
    };

    GRID_ITEMS.with_borrow(|items| {
        let Some(widgets) = items.get(&root) else {
            return;
        };

        widgets.name.set_text(entry.display_name());
        widgets.subtitle.set_text(&entry.subtitle());
        icons::set_entry_icon(&widgets.icon, &entry);

        widgets.tile.remove_css_class("directory");
        widgets.tile.remove_css_class("image");
        if entry.is_directory() {
            widgets.tile.add_css_class("directory");
        }

        let thumbnail = entry.thumbnail();
        apply_thumbnail(widgets, thumbnail.as_ref());

        let mut handlers = Vec::new();

        handlers.push(entry.connect_notify_local(Some("child-count"), {
            let subtitle = widgets.subtitle.clone();
            move |entry, _| {
                let Some(entry) = entry.downcast_ref::<FileEntry>() else {
                    return;
                };
                subtitle.set_text(&entry.subtitle());
            }
        }));

        handlers.push(entry.connect_notify_local(Some("thumbnail"), {
            let root = root.clone();
            move |entry, _| {
                let Some(entry) = entry.downcast_ref::<FileEntry>() else {
                    return;
                };
                let thumbnail = entry.thumbnail();
                GRID_ITEMS.with_borrow(|items| {
                    if let Some(widgets) = items.get(&root) {
                        apply_thumbnail(widgets, thumbnail.as_ref());
                    }
                });
            }
        }));

        bindings
            .borrow_mut()
            .insert(item.clone(), (entry.clone(), handlers));
    });

    icons::request_thumbnail(&entry);
}

fn apply_thumbnail(widgets: &GridItem, texture: Option<&gdk::Texture>) {
    match texture {
        Some(texture) => {
            widgets.picture.set_paintable(Some(texture));
            widgets.picture.set_visible(true);
            widgets.icon.set_visible(false);
            widgets.tile.add_css_class("image");
        }
        None => {
            widgets.picture.set_paintable(gdk::Paintable::NONE);
            widgets.picture.set_visible(false);
            widgets.icon.set_visible(true);
        }
    }
}

// -------------------------------------------------------------------- list ----

fn build_columns(app: &App) {
    let list = &app.widgets.list;
    while let Some(column) = list
        .columns()
        .item(0)
        .and_downcast::<gtk::ColumnViewColumn>()
    {
        list.remove_column(&column);
    }

    let name = gtk::ColumnViewColumn::new(Some("Name"), Some(name_column_factory()));
    name.set_expand(true);
    name.set_resizable(true);
    list.append_column(&name);

    let size = gtk::ColumnViewColumn::new(
        Some("Size"),
        Some(watching_column_factory(|entry| {
            if entry.is_directory() {
                let count = entry.child_count();
                if count < 0 {
                    String::new()
                } else {
                    crate::files::item_count_label(usize::try_from(count).unwrap_or(0))
                }
            } else {
                format_size(entry.data().size)
            }
        })),
    );
    size.set_fixed_width(110);
    list.append_column(&size);

    let kind = gtk::ColumnViewColumn::new(
        Some("Type"),
        Some(text_column_factory(|entry| entry.data().kind.clone())),
    );
    kind.set_fixed_width(170);
    kind.set_resizable(true);
    list.append_column(&kind);

    let modified = gtk::ColumnViewColumn::new(
        Some("Modified"),
        Some(text_column_factory(|entry| {
            entry
                .data()
                .modified
                .as_ref()
                .map(format_time)
                .unwrap_or_default()
        })),
    );
    modified.set_fixed_width(170);
    list.append_column(&modified);
}

fn name_column_factory() -> gtk::SignalListItemFactory {
    let factory = gtk::SignalListItemFactory::new();

    factory.connect_setup(|_, item| {
        let Some(item) = item.downcast_ref::<gtk::ListItem>() else {
            return;
        };
        let row = gtk::Box::new(gtk::Orientation::Horizontal, 9);
        let icon = gtk::Image::new();
        icon.set_pixel_size(16);
        let label = gtk::Label::new(None);
        label.set_xalign(0.0);
        label.set_hexpand(true);
        label.set_ellipsize(gtk::pango::EllipsizeMode::Middle);
        row.append(&icon);
        row.append(&label);
        item.set_child(Some(&row));
    });

    factory.connect_bind(|_, item| {
        let Some(item) = item.downcast_ref::<gtk::ListItem>() else {
            return;
        };
        let Some(row) = item.child().and_downcast::<gtk::Box>() else {
            return;
        };
        let Some(entry) = item.item().and_downcast::<FileEntry>() else {
            return;
        };
        if let Some(icon) = row.first_child().and_downcast::<gtk::Image>() {
            icons::set_entry_icon(&icon, &entry);
        }
        if let Some(label) = row.last_child().and_downcast::<gtk::Label>() {
            label.set_text(entry.display_name());
        }
    });

    factory
}

/// A text column that also refreshes when a folder's item count arrives.
fn watching_column_factory(
    value: impl Fn(&FileEntry) -> String + Clone + 'static,
) -> gtk::SignalListItemFactory {
    let factory = text_column_factory(value.clone());
    let handlers: Bindings = Rc::new(RefCell::new(HashMap::new()));

    factory.connect_bind({
        let handlers = Rc::clone(&handlers);
        move |_, item| {
            let Some(item) = item.downcast_ref::<gtk::ListItem>() else {
                return;
            };
            let Some(label) = item.child().and_downcast::<gtk::Label>() else {
                return;
            };
            let Some(entry) = item.item().and_downcast::<FileEntry>() else {
                return;
            };

            let value = value.clone();
            let handler = entry.connect_notify_local(Some("child-count"), move |entry, _| {
                if let Some(entry) = entry.downcast_ref::<FileEntry>() {
                    label.set_text(&value(entry));
                }
            });
            handlers
                .borrow_mut()
                .insert(item.clone(), (entry.clone(), vec![handler]));
        }
    });

    factory.connect_unbind(move |_, item| {
        if let Some(item) = item.downcast_ref::<gtk::ListItem>()
            && let Some((entry, created)) = handlers.borrow_mut().remove(item)
        {
            for handler in created {
                entry.disconnect(handler);
            }
        }
    });

    factory
}

fn text_column_factory(
    value: impl Fn(&FileEntry) -> String + 'static,
) -> gtk::SignalListItemFactory {
    let factory = gtk::SignalListItemFactory::new();

    factory.connect_setup(|_, item| {
        let Some(item) = item.downcast_ref::<gtk::ListItem>() else {
            return;
        };
        let label = gtk::Label::new(None);
        label.set_xalign(0.0);
        label.set_ellipsize(gtk::pango::EllipsizeMode::End);
        label.add_css_class("teral-item-subtitle");
        item.set_child(Some(&label));
    });

    factory.connect_bind(move |_, item| {
        let Some(item) = item.downcast_ref::<gtk::ListItem>() else {
            return;
        };
        let Some(label) = item.child().and_downcast::<gtk::Label>() else {
            return;
        };
        let Some(entry) = item.item().and_downcast::<FileEntry>() else {
            return;
        };
        label.set_text(&value(&entry));
    });

    factory
}

// ----------------------------------------------------------- context menu ----

fn attach_context_gesture(app: &App, widget: &gtk::Box, item: &gtk::ListItem) {
    let gesture = gtk::GestureClick::new();
    gesture.set_button(gdk::BUTTON_SECONDARY);

    let app = Rc::clone(app);
    let item = item.clone();
    let owner = widget.clone();
    let widget = widget.clone();
    gesture.connect_pressed(move |_, _, x, y| {
        let position = item.position();
        if position != gtk::INVALID_LIST_POSITION && !app.state.selection.is_selected(position) {
            app.state.selection.select_item(position, true);
        }
        super::window::show_context_menu(&app, &widget, x, y);
    });

    owner.add_controller(gesture);
}
