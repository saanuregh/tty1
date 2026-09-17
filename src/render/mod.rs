mod dashboard;
mod settings;
mod shell;
mod utils;

pub use dashboard::render_page;
pub use settings::render_settings_page;
pub use shell::{SW_REGISTER, THEME_BOOTSTRAP, render_loading_page};

// Shared with `csp`, which hashes the exact text of every inline <script>.
pub const COMMON_JS: &str = include_str!("../static/common.js");
pub const APP_JS: &str = include_str!("../static/app.js");
pub const SETTINGS_JS: &str = include_str!("../static/settings.js");
