use maud::{Markup, PreEscaped, html};

use super::shell::page_shell;
use crate::config;

fn keybind_row(keys: &[&str], desc: &str) -> Markup {
    html! {
        div.keybind-row {
            @for key in keys {
                span.keybind-key { (key) }
            }
            span.keybind-desc { (desc) }
        }
    }
}

pub fn render_settings_page() -> String {
    page_shell(
        super::SETTINGS_CSS,
        html! {
            main #main-content .settings {
                header.settings-header {
                    a href="/" { "← tty1" }
                    h1 { "settings" }
                }
                noscript {
                    p.noscript-msg { "JavaScript is required for settings." }
                }

                div.settings-grid {
                    div.settings-col {
                        section.settings-section {
                            span.settings-label { "theme" }
                            div.theme-buttons {
                                button.theme-btn data-theme="dark" { "dark" }
                                button.theme-btn data-theme="light" { "light" }
                            }
                        }

                        section.settings-section {
                            span.settings-label { "hackernews page" }
                            div.theme-buttons {
                                @for &(name, _) in config::HN_PAGES {
                                    button.hn-btn data-hn=(name) { (name) }
                                }
                            }
                        }

                        section.settings-section {
                            span.settings-label { "github period" }
                            div.theme-buttons {
                                @for &period in config::GITHUB_PERIODS {
                                    button.period-btn data-period=(period) { (period) }
                                }
                            }
                        }

                        section.settings-section {
                            span.settings-label { "panels · drag or use arrow keys to reorder" }
                            div.panel-toggles {
                                button.panel-toggle.active data-panel="hn" {
                                    (PreEscaped(include_str!("../static/icons/hn.svg")))
                                    "hackernews"
                                }
                                button.panel-toggle.active data-panel="gh" {
                                    (PreEscaped(include_str!("../static/icons/github.svg")))
                                    "github"
                                }
                                button.panel-toggle.active data-panel="reddit" {
                                    (PreEscaped(include_str!("../static/icons/reddit.svg")))
                                    "reddit"
                                }
                            }
                        }

                        section.settings-section {
                            div.settings-label-row {
                                span.settings-label { "subreddits" }
                                button.select-toggle data-target="sub" { "deselect all" }
                            }
                            div.checkbox-grid {
                                @for sub in config::REDDIT_SUBREDDITS {
                                    label.checkbox-item {
                                        input.sub-check type="checkbox" value=(sub) checked;
                                        span { "r/" (sub) }
                                    }
                                }
                            }
                        }

                        section.settings-section {
                            div.settings-label-row {
                                span.settings-label { "languages" }
                                button.select-toggle data-target="lang" { "deselect all" }
                            }
                            div.checkbox-grid {
                                @for &(name, _) in config::GITHUB_LANGUAGES {
                                    label.checkbox-item {
                                        input.lang-check type="checkbox" value=(name) checked;
                                        span { (name) }
                                    }
                                }
                            }
                        }

                        section.settings-section {
                            span.settings-label { "share profile" }
                            div.share-row {
                                input.share-url type="text" aria-label="Share profile URL" readonly;
                                button.share-btn { "copy link" }
                            }
                        }

                        section.settings-section {
                            button.reset-btn { "reset to defaults" }
                        }
                    }

                    aside.settings-aside {
                        section.settings-section {
                            span.settings-label { "keyboard shortcuts" }
                            div.keybind-list {
                                (keybind_row(&["h", "l"], "switch panels"))
                                (keybind_row(&["j", "k"], "navigate items"))
                                (keybind_row(&["f"], "focus filter"))
                                (keybind_row(&["Enter"], "open link"))
                                (keybind_row(&["Shift+Enter"], "open in new tab"))
                                (keybind_row(&["c"], "open comments"))
                                (keybind_row(&["r"], "refresh"))
                                (keybind_row(&["t"], "toggle theme"))
                                (keybind_row(&["Escape"], "unfocus"))
                                (keybind_row(&[","], "settings / back"))
                            }
                        }
                        div.settings-footer {
                            a.github-link href="https://github.com/saanuregh/tty1" target="_blank" rel="noopener" {
                                (PreEscaped(include_str!("../static/icons/github.svg")))
                                "saanuregh/tty1"
                            }
                            " · MIT · 2025"
                        }
                    }
                }
            }
            script { (PreEscaped(super::SETTINGS_JS)) }
        },
    )
    .into_string()
}
