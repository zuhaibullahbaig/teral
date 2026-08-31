//! The header wordmark.
//!
//! Restrained by design: a small, letter-spaced word, not a logo lockup. It reads
//! `Files`, which is what people expect a file manager to call itself in its own
//! window; the application is still Teral in its title, About window and configuration.

use gtk::prelude::*;

/// The header wordmark.
pub fn build() -> gtk::Label {
    let wordmark = super::tracked_label("Files", 4);
    wordmark.add_css_class("teral-brand");
    wordmark.set_valign(gtk::Align::Center);
    wordmark
}
