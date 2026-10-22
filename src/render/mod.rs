mod dashboard;
mod settings;
mod shell;
mod utils;

pub use dashboard::{
    View, lang_display, render_gh_custom_fragment, render_gh_fragment, render_hn_fragment,
    render_page, render_reddit_custom_fragment, render_reddit_fragment, valid_hn, valid_lang,
    valid_period, valid_sub,
};
pub use settings::render_settings_page;
pub use shell::{SW_REGISTER, THEME_BOOTSTRAP, render_loading_page};

// Built by `mise run frontend`; shared with `csp`, which hashes the exact text
// of every inline <script>.
pub const DASHBOARD_JS: &str = include_str!("../static/dashboard.bundle.js");
pub const SETTINGS_JS: &str = include_str!("../static/settings.bundle.js");
pub const DASHBOARD_CSS: &str = include_str!("../static/dashboard.bundle.css");
pub const SETTINGS_CSS: &str = include_str!("../static/settings.bundle.css");
