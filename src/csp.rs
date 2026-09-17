use std::sync::LazyLock;

use sha2::{Digest, Sha256};

use crate::render;

/// Built once at startup: every inline script is an `include_str!` constant, so
/// the hashes never change at runtime.
pub static CSP_HEADER: LazyLock<String> = LazyLock::new(build_header);

fn build_header() -> String {
    // Maud emits `script { (a) (b) }` with nothing between the two strings, so
    // the hash input is the plain concatenation.
    let inline_scripts = [
        render::THEME_BOOTSTRAP.to_string(),
        render::SW_REGISTER.to_string(),
        format!("{}{}", render::COMMON_JS, render::APP_JS),
        format!("{}{}", render::COMMON_JS, render::SETTINGS_JS),
    ];

    let hashes: Vec<String> = inline_scripts
        .iter()
        .map(|s| {
            let digest = Sha256::digest(s.as_bytes());
            format!("'sha256-{}'", base64(digest.as_slice()))
        })
        .collect();

    // style-src keeps 'unsafe-inline' and carries no hash on purpose: the
    // language-colour dot writes a style attribute, which no hash can cover,
    // and any hash here would make browsers ignore 'unsafe-inline'.
    format!(
        "default-src 'self'; \
         script-src 'self' {}; \
         style-src 'self' 'unsafe-inline'; \
         img-src 'self' data:; \
         connect-src 'self'; \
         manifest-src 'self'; \
         frame-ancestors 'none'; \
         base-uri 'none'; \
         form-action 'none'; \
         object-src 'none'",
        hashes.join(" ")
    )
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

    // RFC 4648 section 10 test vectors.
    #[test]
    fn base64_matches_known_vectors() {
        assert_eq!(base64(b""), "");
        assert_eq!(base64(b"f"), "Zg==");
        assert_eq!(base64(b"fo"), "Zm8=");
        assert_eq!(base64(b"foo"), "Zm9v");
        assert_eq!(base64(b"foob"), "Zm9vYg==");
        assert_eq!(base64(b"fooba"), "Zm9vYmE=");
        assert_eq!(base64(b"foobar"), "Zm9vYmFy");
    }

    #[test]
    fn header_hashes_every_inline_script() {
        assert_eq!(CSP_HEADER.matches("'sha256-").count(), 4);
    }

    // A hash in a directive makes browsers ignore 'unsafe-inline' in that same
    // directive. style-src needs 'unsafe-inline' for the per-repo colour
    // attribute, so it must carry no hash.
    #[test]
    fn style_src_has_no_hash() {
        let style = CSP_HEADER
            .split(';')
            .find(|d| d.trim().starts_with("style-src"))
            .expect("style-src directive");
        assert!(style.contains("'unsafe-inline'"));
        assert!(!style.contains("sha256-"));
    }
}
