//! Server-side rendering: the template engine and render helpers.

mod engine;
mod render;

pub use engine::TemplateEngine;
pub use render::{PageContext, render_page};
