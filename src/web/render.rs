//! Helpers for rendering full pages into Axum responses.

use super::engine::TemplateEngine;
use crate::error::{AppError, AppResult};
use axum::response::Html;
use minijinja::value::Value;

/// Common context shared by every rendered page.
pub struct PageContext {
    pub current_user: Option<String>,
    pub active_nav: String,
}

impl PageContext {
    pub fn new(current_user: Option<String>, active_nav: impl Into<String>) -> Self {
        Self {
            current_user,
            active_nav: active_nav.into(),
        }
    }
}

/// Merge the page context into a page-specific context map and render.
///
/// `extra` carries page-specific values; the shared `current_user`/`active_nav`
/// are always injected.
pub fn render_page(
    engine: &TemplateEngine,
    template: &str,
    page: &PageContext,
    extra: Value,
) -> AppResult<Html<String>> {
    let base = minijinja::context! {
        current_user => page.current_user,
        active_nav => page.active_nav,
    };
    let merged = minijinja::context! { ..base, ..extra };
    engine
        .render(template, merged)
        .map(Html)
        .map_err(|e| AppError::internal(format!("template error: {e}")))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn render_page_injects_shared_context() {
        let engine = TemplateEngine::new();
        let page = PageContext::new(Some("admin".into()), "home");
        let html = render_page(&engine, "home.html", &page, minijinja::context! {})
            .unwrap()
            .0;
        assert!(html.contains("admin"));
        assert!(html.contains("Dashboard"));
    }

    #[test]
    fn missing_template_is_internal_error() {
        let engine = TemplateEngine::new();
        let page = PageContext::new(None, "home");
        let err = render_page(&engine, "nope.html", &page, minijinja::context! {}).unwrap_err();
        assert!(matches!(err, AppError::Internal(_)));
    }
}
