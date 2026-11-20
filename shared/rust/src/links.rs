//! A web address a file links to (a PDF's link, a slide's link), handed to the system's browser. Only `http` and
//! `https`: whatever else a file names (file:, javascript:, mailto:, a program's own scheme) is never opened.

/// The address, trimmed, when it is one this family opens: `http://` or `https://`, with something after it, and nothing
/// in it that a shell or a command line could read as more than one argument.
pub fn web_address(url: &str) -> Option<&str> {
    let url = url.trim();
    let lower = url.get(..8).unwrap_or(url).to_ascii_lowercase();
    let rest = if lower.starts_with("https://") {
        &url[8..]
    } else if lower.starts_with("http://") {
        &url[7..]
    } else {
        return None;
    };
    if rest.is_empty() || rest.starts_with('/') || url.len() > 4096 {
        return None;
    }
    if url
        .chars()
        .any(|c| c.is_control() || c.is_whitespace() || c == '"' || c == '<' || c == '>' || c == '`' || c == '\\')
    {
        return None;
    }
    Some(url)
}

/// Opens a web address in the system's browser. False when it isn't one (`web_address`) or the system refused.
/// With `DOTANY_TEST_LINKS` set to a file, the address is written there on a line of its own instead, so that an
/// automated test sees what would have opened, and no browser starts.
pub fn open_web_link(url: &str) -> bool {
    let Some(url) = web_address(url) else { return false };
    if let Some(log) = std::env::var_os("DOTANY_TEST_LINKS") {
        use std::io::Write;
        return std::fs::OpenOptions::new()
            .create(true)
            .append(true)
            .open(log)
            .and_then(|mut f| writeln!(f, "{url}"))
            .is_ok();
    }
    crate::platform::open_url(url)
}

#[cfg(test)]
mod tests {
    use super::web_address;

    #[test]
    fn http_and_https_only() {
        assert_eq!(web_address("https://example.com/a?b=1#c"), Some("https://example.com/a?b=1#c"));
        assert_eq!(web_address("HTTP://Example.com"), Some("HTTP://Example.com"));
        assert_eq!(web_address("  https://example.com  "), Some("https://example.com"));
        for bad in [
            "file:///C:/Windows/System32/calc.exe",
            "javascript:alert(1)",
            "mailto:someone@example.com",
            "ms-settings:privacy",
            "ftp://example.com",
            "\\\\server\\share",
            "C:\\Windows\\notepad.exe",
            "https://",
            "http://",
            "  http://  ",
            "http:// example.com",
            "https:///etc",
            "http:example.com",
            "https://example.com/\" --new-window",
            "https://exa mple.com",
            "https://example.com/a b",
            "https://example.com/\ta",
            "https://example.com/\u{3000}a",
            "https://example.com/\u{0}x",
            "",
        ] {
            assert_eq!(web_address(bad), None, "{bad}");
        }
    }
}
