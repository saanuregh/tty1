use std::sync::LazyLock;

use sha2::{Digest, Sha256};

use crate::render;

/// Built once at startup: every inline script is an `include_str!` constant, so
/// the hashes never change at runtime.
pub static CSP_HEADER: LazyLock<String> = LazyLock::new(build_header);

fn build_header() -> String {
    // Every inline script is one bundle or one constant, so the hash input is
    // exactly the string Maud emits.
    let inline_scripts = [
        render::THEME_BOOTSTRAP.to_string(),
        render::SW_REGISTER.to_string(),
        render::DASHBOARD_JS.to_string(),
        render::SETTINGS_JS.to_string(),
    ];

    let hashes: Vec<String> = inline_scripts.iter().map(|s| hash(s)).collect();
    let style_hashes: Vec<String> = [render::DASHBOARD_CSS, render::SETTINGS_CSS]
        .iter()
        .map(|s| hash(s))
        .collect();

    format!(
        "default-src 'self'; \
         script-src 'self' {}; \
         style-src 'self' {}; \
         img-src 'self' data:; \
         connect-src 'self'; \
         manifest-src 'self'; \
         frame-ancestors 'none'; \
         base-uri 'none'; \
         form-action 'none'; \
         object-src 'none'",
        hashes.join(" "),
        style_hashes.join(" ")
    )
}

fn hash(value: &str) -> String {
    let digest = Sha256::digest(value.as_bytes());
    format!("'sha256-{}'", base64(digest.as_slice()))
}

const ALPHABET: &[u8; 64] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";

fn base64(data: &[u8]) -> String {
    let mut out = String::with_capacity(data.len().div_ceil(3) * 4);
    for chunk in data.chunks(3) {
        let b1 = u32::from(chunk[0]);
        let b2 = u32::from(chunk.get(1).copied().unwrap_or(0));
        let b3 = u32::from(chunk.get(2).copied().unwrap_or(0));
        let n = (b1 << 16) | (b2 << 8) | b3;

        out.push(ALPHABET[(n >> 18) as usize & 63] as char);
        out.push(ALPHABET[(n >> 12) as usize & 63] as char);
        out.push(if chunk.len() > 1 {
            ALPHABET[(n >> 6) as usize & 63] as char
        } else {
            '='
        });
        out.push(if chunk.len() > 2 {
            ALPHABET[n as usize & 63] as char
        } else {
            '='
        });
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn header_hashes_every_inline_script() {
        assert_eq!(CSP_HEADER.matches("'sha256-").count(), 6);
    }

    #[test]
    fn style_src_uses_hashes_without_unsafe_inline() {
        let style = CSP_HEADER
            .split(';')
            .find(|d| d.trim().starts_with("style-src"))
            .expect("style-src directive");
        assert!(!style.contains("'unsafe-inline'"));
        assert_eq!(style.matches("'sha256-").count(), 2);
    }
}
