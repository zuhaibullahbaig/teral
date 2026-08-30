//! Teral's wordmark.
//!
//! Restrained by design: a small, letter-spaced word, not a logo lockup.

use gtk::prelude::*;

/// The `TERAL` wordmark.
pub fn build() -> gtk::Label {
    let wordmark = super::tracked_label("TERAL", 4);
    wordmark.add_css_class("teral-brand");
    wordmark.set_valign(gtk::Align::Center);
    wordmark
}
