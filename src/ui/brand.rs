//! Teral's wordmark.
//!
//! The mark is drawn rather than shipped as an asset so it follows the resolved accent
//! colour, stays crisp at any scale factor, and needs no icon-theme lookup.

use super::App;
use gtk::cairo;
use gtk::prelude::*;

const MARK_SIZE: i32 = 22;

/// A drawn `T` mark plus the letter-spaced wordmark.
pub fn build(app_accent: &str) -> gtk::Box {
    let mark = gtk::DrawingArea::new();
    mark.add_css_class("teral-brand-mark");
    mark.set_content_width(MARK_SIZE);
    mark.set_content_height(MARK_SIZE);
    mark.set_valign(gtk::Align::Center);

    let accent = accent_rgb(app_accent);
    mark.set_draw_func(move |_, context, width, height| draw_mark(context, width, height, accent));

    let wordmark = super::tracked_label("TERAL", 4);
    wordmark.add_css_class("teral-brand");
    wordmark.set_valign(gtk::Align::Center);

    let root = gtk::Box::new(gtk::Orientation::Horizontal, 9);
    root.add_css_class("teral-brand-box");
    root.set_valign(gtk::Align::Center);
    root.append(&mark);
    root.append(&wordmark);
    root
}

/// Redraw the mark after a theme change.
pub fn restyle(app: &App) {
    let accent = accent_rgb(app.theme.borrow().color(crate::theme::ColorRole::Accent));
    let mark = app.widgets.brand_mark.clone();
    mark.set_draw_func(move |_, context, width, height| draw_mark(context, width, height, accent));
    mark.queue_draw();
}

/// A serif-less `T` inside a rounded square: a stem and a crossbar, nothing else.
fn draw_mark(context: &cairo::Context, width: i32, height: i32, accent: (f64, f64, f64)) {
    let size = f64::from(width.min(height));
    let radius = size * 0.28;
    let (red, green, blue) = accent;

    // Rounded plate.
    context.set_source_rgba(red, green, blue, 0.16);
    rounded_rect(context, 0.0, 0.0, size, size, radius);
    let _ = context.fill();

    // Hairline edge, so the mark keeps its shape on light backgrounds too.
    context.set_source_rgba(red, green, blue, 0.55);
    context.set_line_width(1.0);
    rounded_rect(context, 0.5, 0.5, size - 1.0, size - 1.0, radius);
    let _ = context.stroke();

    // The letter itself.
    let stroke = (size * 0.13).max(2.0);
    let inset = size * 0.24;
    context.set_source_rgb(red, green, blue);
    context.set_line_width(stroke);
    context.set_line_cap(cairo::LineCap::Round);

    context.move_to(inset, inset + stroke * 0.5);
    context.line_to(size - inset, inset + stroke * 0.5);
    let _ = context.stroke();

    context.move_to(size / 2.0, inset + stroke * 0.5);
    context.line_to(size / 2.0, size - inset);
    let _ = context.stroke();
}

fn rounded_rect(context: &cairo::Context, x: f64, y: f64, width: f64, height: f64, radius: f64) {
    let radius = radius.min(width / 2.0).min(height / 2.0);
    context.new_sub_path();
    context.arc(
        x + width - radius,
        y + radius,
        radius,
        -std::f64::consts::FRAC_PI_2,
        0.0,
    );
    context.arc(
        x + width - radius,
        y + height - radius,
        radius,
        0.0,
        std::f64::consts::FRAC_PI_2,
    );
    context.arc(
        x + radius,
        y + height - radius,
        radius,
        std::f64::consts::FRAC_PI_2,
        std::f64::consts::PI,
    );
    context.arc(
        x + radius,
        y + radius,
        radius,
        std::f64::consts::PI,
        1.5 * std::f64::consts::PI,
    );
    context.close_path();
}

fn accent_rgb(color: &str) -> (f64, f64, f64) {
    color
        .parse::<gtk::gdk::RGBA>()
        .map(|rgba| {
            (
                f64::from(rgba.red()),
                f64::from(rgba.green()),
                f64::from(rgba.blue()),
            )
        })
        .unwrap_or((0.88, 0.65, 0.24))
}
