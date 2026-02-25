use maud::{PreEscaped, html};

use super::shell::page_shell;
use crate::config;

pub fn render_settings_page() -> String {
    page_shell(
        include_str!("../static/settings.css"),
        html! {
            main #main-content .settings {
                header.settings-header {
                    a href="/" { "← tty1" }
                    h1 { "settings" }
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
                            span.settings-label { "panels · drag to reorder" }
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
                                input.share-url type="text" readonly;
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
                                div.keybind-row {
                                    span.keybind-key { "h" }
                                    span.keybind-key { "l" }
                                    span.keybind-desc { "switch panels" }
                                }
                                div.keybind-row {
                                    span.keybind-key { "j" }
                                    span.keybind-key { "k" }
                                    span.keybind-desc { "navigate items" }
                                }
                                div.keybind-row {
                                    span.keybind-key { "f" }
                                    span.keybind-desc { "focus filter" }
                                }
                                div.keybind-row {
                                    span.keybind-key { "Enter" }
                                    span.keybind-desc { "open link" }
                                }
                                div.keybind-row {
                                    span.keybind-key { "Shift+Enter" }
                                    span.keybind-desc { "open in new tab" }
                                }
                                div.keybind-row {
                                    span.keybind-key { "c" }
                                    span.keybind-desc { "open comments" }
                                }
                                div.keybind-row {
                                    span.keybind-key { "x" }
                                    span.keybind-desc { "expand description" }
                                }
                                div.keybind-row {
                                    span.keybind-key { "r" }
                                    span.keybind-desc { "refresh" }
                                }
                                div.keybind-row {
                                    span.keybind-key { "t" }
                                    span.keybind-desc { "toggle theme" }
                                }
                                div.keybind-row {
                                    span.keybind-key { "Escape" }
                                    span.keybind-desc { "unfocus" }
                                }
                                div.keybind-row {
                                    span.keybind-key { "," }
                                    span.keybind-desc { "settings / back" }
                                }
                            }
                        }
                    }
                }
            }
            script { (PreEscaped(include_str!("../static/common.js"))) (PreEscaped(include_str!("../static/settings.js"))) }
        },
    )
    .into_string()
}
