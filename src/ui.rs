use crate::theme::{home_dir, ThemeConfig};
use gtk::gio;
use gtk::gio::prelude::*;
use gtk::prelude::*;
use gtk::{
    Align, Application, ApplicationWindow, Box as GtkBox, Button, Image, Label, ListBox,
    ListBoxRow, Orientation, ScrolledWindow, SelectionMode,
};
use std::cell::RefCell;
use std::fs;
use std::path::{Path, PathBuf};
use std::rc::Rc;

#[derive(Debug)]
struct BrowserState {
    current_dir: PathBuf,
    back_stack: Vec<PathBuf>,
    forward_stack: Vec<PathBuf>,
    visible_entries: Vec<PathBuf>,
}

impl BrowserState {
    fn new(current_dir: PathBuf) -> Self {
        Self {
            current_dir,
            back_stack: Vec::new(),
            forward_stack: Vec::new(),
            visible_entries: Vec::new(),
        }
    }
}

#[derive(Clone)]
struct BrowserWidgets {
    location: Label,
    folder_title: Label,
    file_list: ListBox,
    details_name: Label,
    details_meta: Label,
    details_path: Label,
    status: Label,
}

pub fn build_window(application: &Application, theme: &ThemeConfig) -> ApplicationWindow {
    let initial_dir = home_dir().unwrap_or_else(|| PathBuf::from("/"));
    let state = Rc::new(RefCell::new(BrowserState::new(initial_dir)));

    let file_list = ListBox::builder()
        .selection_mode(SelectionMode::Single)
        .activate_on_single_click(false)
        .css_classes(["teral-file-list"])
        .build();

    let widgets = BrowserWidgets {
        location: Label::builder().xalign(0.0).hexpand(true).build(),
        folder_title: Label::builder().xalign(0.0).build(),
        file_list,
        details_name: Label::builder().xalign(0.0).wrap(true).build(),
        details_meta: Label::builder().xalign(0.0).wrap(true).build(),
        details_path: Label::builder().xalign(0.0).wrap(true).selectable(true).build(),
        status: Label::builder().xalign(0.0).build(),
    };

    widgets.location.add_css_class("teral-path");
    widgets.folder_title.add_css_class("teral-title");
    widgets.details_name.add_css_class("teral-title");
    widgets.details_meta.add_css_class("teral-muted");
    widgets.details_path.add_css_class("teral-path");
    widgets.status.add_css_class("teral-muted");

    connect_file_list(&state, &widgets);

    let toolbar = build_toolbar(&state, &widgets);
    let sidebar = build_sidebar(&state, &widgets, theme.sidebar_width());
    let file_view = build_file_view(&widgets, theme.spacing());
    let details = build_details(&widgets, theme.details_width(), theme.spacing());

    let content = GtkBox::new(Orientation::Horizontal, 0);
    content.set_vexpand(true);
    content.append(&sidebar);
    content.append(&file_view);
    content.append(&details);

    let root = GtkBox::new(Orientation::Vertical, 0);
    root.add_css_class("teral-root");
    root.append(&toolbar);
    root.append(&content);

    let window = ApplicationWindow::builder()
        .application(application)
        .title("Teral")
        .default_width(theme.window_width())
        .default_height(theme.window_height())
        .child(&root)
        .build();

    refresh_current_directory(&state, &widgets);
    window
}

fn build_toolbar(state: &Rc<RefCell<BrowserState>>, widgets: &BrowserWidgets) -> GtkBox {
    let toolbar = GtkBox::new(Orientation::Horizontal, 8);
    toolbar.add_css_class("teral-toolbar");

    let brand = Label::new(Some("teral"));
    brand.add_css_class("teral-title");
    brand.set_margin_end(10);

    let back = icon_button("go-previous-symbolic", "Back");
    let forward = icon_button("go-next-symbolic", "Forward");
    let up = icon_button("go-up-symbolic", "Parent folder");

    {
        let state = Rc::clone(state);
        let widgets = widgets.clone();
        back.connect_clicked(move |_| navigate_back(&state, &widgets));
    }

    {
        let state = Rc::clone(state);
        let widgets = widgets.clone();
        forward.connect_clicked(move |_| navigate_forward(&state, &widgets));
    }

    {
        let state = Rc::clone(state);
        let widgets = widgets.clone();
        up.connect_clicked(move |_| {
            let parent = state.borrow().current_dir.parent().map(Path::to_path_buf);
            if let Some(parent) = parent {
                navigate_to(&parent, &state, &widgets);
            }
        });
    }

    toolbar.append(&brand);
    toolbar.append(&back);
    toolbar.append(&forward);
    toolbar.append(&up);
    toolbar.append(&widgets.location);
    toolbar
}

fn build_sidebar(
    state: &Rc<RefCell<BrowserState>>,
    widgets: &BrowserWidgets,
    width: i32,
) -> GtkBox {
    let sidebar = GtkBox::new(Orientation::Vertical, 6);
    sidebar.add_css_class("teral-sidebar");
    sidebar.set_width_request(width);

    let locations = Label::new(Some("LOCATIONS"));
    locations.set_xalign(0.0);
    locations.add_css_class("teral-section-title");
    locations.set_margin_bottom(4);
    sidebar.append(&locations);

    if let Some(home) = home_dir() {
        sidebar.append(&sidebar_location(
            "user-home-symbolic",
            "Home",
            home,
            state,
            widgets,
        ));
    }

    sidebar.append(&sidebar_location(
        "drive-harddisk-symbolic",
        "Filesystem",
        PathBuf::from("/"),
        state,
        widgets,
    ));


    sidebar
}

fn sidebar_location(
    icon_name: &str,
    title: &str,
    path: PathBuf,
    state: &Rc<RefCell<BrowserState>>,
    widgets: &BrowserWidgets,
) -> Button {
    let row = GtkBox::new(Orientation::Horizontal, 9);
    let icon = Image::from_icon_name(icon_name);
    icon.set_pixel_size(18);

    let label = Label::new(Some(title));
    label.set_xalign(0.0);
    label.set_hexpand(true);

    row.append(&icon);
    row.append(&label);

    let button = Button::builder().child(&row).build();
    button.add_css_class("flat");

    let state = Rc::clone(state);
    let widgets = widgets.clone();
    button.connect_clicked(move |_| navigate_to(&path, &state, &widgets));
    button
}

fn build_file_view(widgets: &BrowserWidgets, spacing: i32) -> GtkBox {
    let container = GtkBox::new(Orientation::Vertical, spacing);
    container.set_hexpand(true);
    container.set_vexpand(true);
    container.set_margin_top(16);
    container.set_margin_bottom(10);
    container.set_margin_start(16);
    container.set_margin_end(16);

    container.append(&widgets.folder_title);

    let scroller = ScrolledWindow::builder()
        .hexpand(true)
        .vexpand(true)
        .child(&widgets.file_list)
        .build();
    container.append(&scroller);
    container.append(&widgets.status);
    container
}

fn build_details(widgets: &BrowserWidgets, width: i32, spacing: i32) -> GtkBox {
    let details = GtkBox::new(Orientation::Vertical, spacing);
    details.add_css_class("teral-details");
    details.set_width_request(width);

    let heading = Label::new(Some("DETAILS"));
    heading.set_xalign(0.0);
    heading.add_css_class("teral-section-title");

    widgets.details_name.set_text("Nothing selected");
    widgets
        .details_meta
        .set_text("Select a file or folder to inspect it.");
    widgets.details_path.set_text("");

    details.append(&heading);
    details.append(&widgets.details_name);
    details.append(&widgets.details_meta);
    details.append(&widgets.details_path);
    details
}

fn connect_file_list(state: &Rc<RefCell<BrowserState>>, widgets: &BrowserWidgets) {
    {
        let state = Rc::clone(state);
        let widgets = widgets.clone();
        let file_list = widgets.file_list.clone();
        file_list.connect_row_selected(move |_, row| {
            let Some(row) = row else {
                clear_details(&widgets);
                return;
            };

            let index = row.index();
            let Ok(index) = usize::try_from(index) else {
                return;
            };

            if let Some(path) = state.borrow().visible_entries.get(index).cloned() {
                show_details(&path, &widgets);
            }
        });
    }

    {
        let state = Rc::clone(state);
        let widgets = widgets.clone();
        let file_list = widgets.file_list.clone();
        file_list.connect_row_activated(move |_, row| {
            let index = row.index();
            let Ok(index) = usize::try_from(index) else {
                return;
            };

            let path = state.borrow().visible_entries.get(index).cloned();
            let Some(path) = path else {
                return;
            };

            if path.is_dir() {
                navigate_to(&path, &state, &widgets);
            } else {
                open_file(&path, &widgets);
            }
        });
    }
}

fn navigate_to(path: &Path, state: &Rc<RefCell<BrowserState>>, widgets: &BrowserWidgets) {
    match read_directory(path) {
        Ok(entries) => {
            {
                let mut state = state.borrow_mut();
                if state.current_dir != path {
                    let current = state.current_dir.clone();
                    state.back_stack.push(current);
                    state.forward_stack.clear();
                    state.current_dir = path.to_path_buf();
                }
                state.visible_entries = entries;
            }
            render_directory(state, widgets);
        }
        Err(error) => set_status(widgets, &format!("Cannot open {}: {error}", path.display())),
    }
}

fn navigate_back(state: &Rc<RefCell<BrowserState>>, widgets: &BrowserWidgets) {
    let target = state.borrow().back_stack.last().cloned();
    let Some(target) = target else {
        return;
    };

    match read_directory(&target) {
        Ok(entries) => {
            let mut state_ref = state.borrow_mut();
            state_ref.back_stack.pop();
            let current = state_ref.current_dir.clone();
            state_ref.forward_stack.push(current);
            state_ref.current_dir = target;
            state_ref.visible_entries = entries;
            drop(state_ref);
            render_directory(state, widgets);
        }
        Err(error) => set_status(widgets, &format!("Cannot go back: {error}")),
    }
}

fn navigate_forward(state: &Rc<RefCell<BrowserState>>, widgets: &BrowserWidgets) {
    let target = state.borrow().forward_stack.last().cloned();
    let Some(target) = target else {
        return;
    };

    match read_directory(&target) {
        Ok(entries) => {
            let mut state_ref = state.borrow_mut();
            state_ref.forward_stack.pop();
            let current = state_ref.current_dir.clone();
            state_ref.back_stack.push(current);
            state_ref.current_dir = target;
            state_ref.visible_entries = entries;
            drop(state_ref);
            render_directory(state, widgets);
        }
        Err(error) => set_status(widgets, &format!("Cannot go forward: {error}")),
    }
}

fn refresh_current_directory(state: &Rc<RefCell<BrowserState>>, widgets: &BrowserWidgets) {
    let current = state.borrow().current_dir.clone();
    match read_directory(&current) {
        Ok(entries) => {
            state.borrow_mut().visible_entries = entries;
            render_directory(state, widgets);
        }
        Err(error) => set_status(
            widgets,
            &format!("Cannot open initial directory {}: {error}", current.display()),
        ),
    }
}

fn read_directory(path: &Path) -> Result<Vec<PathBuf>, std::io::Error> {
    let mut entries = fs::read_dir(path)?
        .filter_map(Result::ok)
        .map(|entry| entry.path())
        .collect::<Vec<_>>();

    entries.sort_by(|left, right| {
        right
            .is_dir()
            .cmp(&left.is_dir())
            .then_with(|| file_name(left).to_lowercase().cmp(&file_name(right).to_lowercase()))
    });

    Ok(entries)
}

fn render_directory(state: &Rc<RefCell<BrowserState>>, widgets: &BrowserWidgets) {
    while let Some(child) = widgets.file_list.first_child() {
        widgets.file_list.remove(&child);
    }

    let state_ref = state.borrow();
    let current = &state_ref.current_dir;
    widgets.location.set_text(&current.to_string_lossy());
    widgets.folder_title.set_text(&directory_title(current));

    for path in &state_ref.visible_entries {
        widgets.file_list.append(&file_row(path));
    }

    widgets.status.set_text(&format!(
        "{} item{}",
        state_ref.visible_entries.len(),
        if state_ref.visible_entries.len() == 1 { "" } else { "s" }
    ));
    clear_details(widgets);
}

fn file_row(path: &Path) -> ListBoxRow {
    let row = ListBoxRow::new();
    let content = GtkBox::new(Orientation::Horizontal, 12);
    content.set_valign(Align::Center);

    let icon_name = if path.is_dir() {
        "folder-symbolic"
    } else {
        "text-x-generic-symbolic"
    };
    let icon = Image::from_icon_name(icon_name);
    icon.set_pixel_size(24);

    let labels = GtkBox::new(Orientation::Vertical, 2);
    labels.set_hexpand(true);

    let name = Label::new(Some(&file_name(path)));
    name.set_xalign(0.0);
    name.set_ellipsize(gtk::pango::EllipsizeMode::End);
    name.add_css_class("teral-file-name");

    let kind = Label::new(Some(if path.is_dir() { "Folder" } else { "File" }));
    kind.set_xalign(0.0);
    kind.add_css_class("teral-muted");

    labels.append(&name);
    labels.append(&kind);
    content.append(&icon);
    content.append(&labels);
    row.set_child(Some(&content));
    row
}

fn show_details(path: &Path, widgets: &BrowserWidgets) {
    widgets.details_name.set_text(&file_name(path));
    widgets.details_path.set_text(&path.to_string_lossy());

    match fs::symlink_metadata(path) {
        Ok(metadata) => {
            let kind = if metadata.file_type().is_symlink() {
                "Symbolic link"
            } else if metadata.is_dir() {
                "Folder"
            } else if metadata.is_file() {
                "File"
            } else {
                "Special file"
            };

            let size = if metadata.is_file() {
                format_size(metadata.len())
            } else {
                "—".to_owned()
            };

            widgets
                .details_meta
                .set_text(&format!("{kind}\nSize: {size}"));
        }
        Err(error) => widgets
            .details_meta
            .set_text(&format!("Metadata unavailable: {error}")),
    }
}

fn clear_details(widgets: &BrowserWidgets) {
    widgets.details_name.set_text("Nothing selected");
    widgets
        .details_meta
        .set_text("Select a file or folder to inspect it.");
    widgets.details_path.set_text("");
}

fn open_file(path: &Path, widgets: &BrowserWidgets) {
    let file = gio::File::for_path(path);
    let uri = file.uri();
    if let Err(error) = gio::AppInfo::launch_default_for_uri(&uri, None::<&gio::AppLaunchContext>) {
        set_status(
            widgets,
            &format!("Could not open {}: {error}", path.display()),
        );
    }
}

fn icon_button(icon_name: &str, tooltip: &str) -> Button {
    let button = Button::from_icon_name(icon_name);
    button.add_css_class("flat");
    button.set_tooltip_text(Some(tooltip));
    button
}

fn directory_title(path: &Path) -> String {
    path.file_name()
        .and_then(|name| name.to_str())
        .map_or_else(|| path.to_string_lossy().into_owned(), ToOwned::to_owned)
}

fn file_name(path: &Path) -> String {
    path.file_name()
        .map(|name| name.to_string_lossy().into_owned())
        .unwrap_or_else(|| path.to_string_lossy().into_owned())
}

fn format_size(bytes: u64) -> String {
    const KB: u64 = 1024;
    const MB: u64 = KB * 1024;
    const GB: u64 = MB * 1024;

    if bytes >= GB {
        format!("{:.1} GB", bytes as f64 / GB as f64)
    } else if bytes >= MB {
        format!("{:.1} MB", bytes as f64 / MB as f64)
    } else if bytes >= KB {
        format!("{:.1} KB", bytes as f64 / KB as f64)
    } else {
        format!("{bytes} B")
    }
}

fn set_status(widgets: &BrowserWidgets, message: &str) {
    widgets.status.set_text(message);
}
