use crate::config::{self, Config};
use crate::style;
use crate::theme::ThemeConfig;
use crate::ui::{self, App};
use gtk::Application;
use gtk::gio;
use gtk::gio::prelude::*;
use gtk::prelude::*;
use std::cell::RefCell;
use std::path::PathBuf;
use std::sync::atomic::Ordering;

thread_local! {
    /// The window this process is showing, if it has built one yet.
    ///
    /// Teral is a single-instance application: a second `teral` launch, and every
    /// folder the desktop asks it to open, is delivered to this process rather than
    /// starting another one. Keeping the running window here is what lets those
    /// requests land in it as tabs instead of piling up windows.
    static RUNNING: RefCell<Option<App>> = const { RefCell::new(None) };
}

/// Launched with no arguments, or activated again while already running.
pub fn activate(application: &Application) {
    present(application, None);
}

/// Launched with files or folders: `teral ~/Documents`, or the desktop entry's `%U`.
///
/// Directories open directly. A file opens the folder that contains it with the file
/// selected, which is what makes `teral some/report.pdf` useful — Teral is a file
/// manager, so it shows you the file rather than launching it.
pub fn open(application: &Application, files: &[gio::File], _hint: &str) {
    let mut requests = files.iter().filter_map(request_for).peekable();

    if requests.peek().is_none() {
        // Every argument was something Teral cannot show — a remote URI with no local
        // path, or a path that has since gone. Opening the default window is better
        // than starting nothing at all.
        present(application, None);
        return;
    }

    for request in requests {
        present(application, Some(request));
    }
}

/// What one command-line argument asks Teral to show.
struct Request {
    directory: PathBuf,
    select: Option<PathBuf>,
}

fn request_for(file: &gio::File) -> Option<Request> {
    let path = file.path()?;

    if path.is_dir() {
        return Some(Request {
            directory: path,
            select: None,
        });
    }

    // Not a directory: show the folder it lives in. This covers a regular file, and
    // also a path that no longer exists, where landing in the parent is more useful
    // than refusing to start.
    let parent = path.parent()?.to_path_buf();
    Some(Request {
        directory: parent,
        select: path.symlink_metadata().is_ok().then_some(path),
    })
}

/// Show `request`, building the window if this is the first thing to arrive.
fn present(application: &Application, request: Option<Request>) {
    let existing = RUNNING.with_borrow(Clone::clone);

    if let Some(app) = existing {
        if let Some(request) = request {
            // A second launch never replaces what is already on screen. Whatever was
            // being looked at stays in its tab, and the new location arrives beside it.
            *app.state.pending_selection.borrow_mut() = request.select;
            app.open_tab(request.directory);
        }
        app.widgets.window.present();
        return;
    }

    let tags_error = crate::tags::init().err();
    let (config, config_error) = match Config::load() {
        Ok(config) => (config, None),
        Err(error) => (Config::default(), Some(error)),
    };
    let theme = ThemeConfig::resolve(&config);
    config::set_current(config.clone());
    style::apply(&theme);

    // Wear whatever icon this desktop already uses for its file manager, instead of the
    // blank placeholder an application with no installed icon gets.
    if let Some(icon) = crate::icons::file_manager_icon_name() {
        gtk::Window::set_default_icon_name(&icon);
    }

    let restore_session = request.is_none();
    let (directory, select) = match request {
        Some(request) => (Some(request.directory), request.select),
        None => (None, None),
    };

    let app = ui::build_window_at(application, config, theme, directory);
    if let Some(error) = tags_error {
        app.show_error(&format!("Could not load tags: {error}"));
    }
    if let Some(error) = config_error {
        app.show_error(&format!("Could not load settings: {error}"));
    }
    *app.state.pending_selection.borrow_mut() = select;

    if restore_session {
        match crate::session::load() {
            Ok(Some(session)) => app.restore_session(session),
            Ok(None) => {}
            Err(error) => app.show_error(&format!("Could not restore the last session: {error}")),
        }
    }

    let window = app.widgets.window.clone();
    RUNNING.with_borrow_mut(|running| *running = Some(app.clone()));

    // The window outlives this function; dropping the record when it closes keeps a
    // later activation from presenting a window that is gone.
    let app_for_close = app.clone();
    let application_for_close = application.clone();
    window.connect_destroy(move |_| {
        if let Some(cancel) = app_for_close.state.running_transfer.borrow().as_ref() {
            cancel.cancel();
        }
        if let Some(cancel) = app_for_close.state.global_search_cancel.borrow().as_ref() {
            cancel.store(true, Ordering::Release);
        }
        if let Some(pid) = app_for_close.state.running_pid.get()
            && let Err(error) = crate::command::force_stop(pid)
        {
            eprintln!("Teral: could not stop Quick Command during shutdown: {error}");
        }
        if let Err(error) = app_for_close.save_session() {
            eprintln!("Teral: could not save the session: {error}");
        }
        app_for_close.disconnect_desktop_handlers();
        RUNNING.with_borrow_mut(|running| *running = None);
        application_for_close.quit();
    });
    window.present();
}
