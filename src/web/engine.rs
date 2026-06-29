//! minijinja template environment with templates embedded at compile time.

use minijinja::Environment;
use std::sync::Arc;

/// Owns the configured template environment. Cheaply cloneable.
#[derive(Clone)]
pub struct TemplateEngine {
    env: Arc<Environment<'static>>,
}

/// Register every embedded template. Adding a new `.html` file means adding one
/// line here.
fn register(env: &mut Environment<'static>) {
    let templates: &[(&str, &str)] = &[
        ("base.html", include_str!("../../templates/base.html")),
        ("home.html", include_str!("../../templates/home.html")),
        ("login.html", include_str!("../../templates/login.html")),
        ("_login_form.html", include_str!("../../templates/_login_form.html")),
        ("settings.html", include_str!("../../templates/settings.html")),
        ("jobs.html", include_str!("../../templates/jobs.html")),
        ("job_detail.html", include_str!("../../templates/job_detail.html")),
        ("_platform_tabs.html", include_str!("../../templates/_platform_tabs.html")),
        ("platform_list.html", include_str!("../../templates/platform_list.html")),
        ("platform_detail.html", include_str!("../../templates/platform_detail.html")),
        ("platform_app_detail.html", include_str!("../../templates/platform_app_detail.html")),
        ("platform_graph.html", include_str!("../../templates/platform_graph.html")),
        ("dashboard.html", include_str!("../../templates/dashboard.html")),
        ("c4.html", include_str!("../../templates/c4.html")),
    ];
    for (name, source) in templates {
        env.add_template(name, source)
            .unwrap_or_else(|e| panic!("invalid template {name}: {e}"));
    }
}

impl TemplateEngine {
    /// Build the engine, compiling all embedded templates. Uses a placeholder
    /// asset version; prefer [`TemplateEngine::with_version`] in production.
    pub fn new() -> Self {
        Self::with_version("dev")
    }

    /// Build the engine with the cache-busting asset version exposed to every
    /// template as the `asset_version` global (used in `?v=` on asset URLs).
    pub fn with_version(version: impl Into<String>) -> Self {
        let mut env = Environment::new();
        register(&mut env);
        env.add_global("asset_version", minijinja::value::Value::from(version.into()));
        Self { env: Arc::new(env) }
    }

    /// Render a template by name with the given context value.
    pub fn render(
        &self,
        template: &str,
        ctx: minijinja::value::Value,
    ) -> Result<String, minijinja::Error> {
        let tmpl = self.env.get_template(template)?;
        tmpl.render(ctx)
    }
}

impl Default for TemplateEngine {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use minijinja::context;

    #[test]
    fn renders_home_with_user() {
        let engine = TemplateEngine::new();
        let html = engine
            .render("home.html", context! { current_user => "admin", active_nav => "home" })
            .unwrap();
        assert!(html.contains("Dashboard"));
        assert!(html.contains("admin"));
        assert!(html.contains("/assets/vendor/jquery.min.js"));
    }

    #[test]
    fn renders_login_form_for_each_provider() {
        let engine = TemplateEngine::new();
        // Admin (default): the password form with the CSRF field is rendered
        // (the `_login_form.html` partial must be embedded).
        let admin = engine
            .render(
                "login.html",
                context! { csrf => "tok123", error => (), auth_provider => "admin", github_mode => "" },
            )
            .unwrap();
        assert!(admin.contains("name=\"csrf\" value=\"tok123\""));
        assert!(admin.contains("Password"));

        // GitHub oauth_app mode shows the "Sign in with GitHub" button.
        let gh = engine
            .render(
                "login.html",
                context! { csrf => "t", error => (), auth_provider => "github", github_mode => "oauth_app" },
            )
            .unwrap();
        assert!(gh.contains("/auth/github/login"));
        assert!(gh.contains("Sign in with GitHub"));

        // GitHub personal_token mode relabels the form for a token.
        let pat = engine
            .render(
                "login.html",
                context! { csrf => "t", error => (), auth_provider => "github", github_mode => "personal_token" },
            )
            .unwrap();
        assert!(pat.contains("Personal access token"));
        assert!(pat.contains("name=\"csrf\""));
    }

    #[test]
    fn renders_platform_templates_with_tabs() {
        let engine = TemplateEngine::new();
        // Graph page includes the shared tab partial (exercises its `{% set %}`).
        let graph = engine
            .render(
                "platform_graph.html",
                context! { current_user => "admin", active_nav => "platform", active_tab => "graph" },
            )
            .unwrap();
        assert!(graph.contains("Tools"));
        assert!(graph.contains("Cloud providers"));
        // Application detail page renders its containers + assets.
        let detail = engine
            .render(
                "platform_app_detail.html",
                context! { current_user => "admin", active_nav => "platform",
                           active_tab => "applications", entity => "applications", entity_id => "x" },
            )
            .unwrap();
        assert!(detail.contains("app-detail"));
        assert!(detail.contains("platform-app-detail.js"));
    }
}
