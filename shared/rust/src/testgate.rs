//! Who may talk to the Mac's test driver (`testdrive.rs`), kept apart from it so that the rules are plain functions
//! that build and are tested on every system.
//!
//! The driver runs JavaScript in the page and drives the program, so it must answer only the test that started it:
//!
//! - It needs a secret: the test makes a random token for each start and passes it in `DOTANY_TEST_TOKEN`; every request
//!   carries it as the first segment of its path (`/<token>/json`, `/<token>/w/<label>`). Another user of the same Mac
//!   can't read this program's environment, and nothing the driver answers without the token gives it away. Without a
//!   token, or with one too short to be a secret, the driver doesn't start at all.
//! - It refuses any request with an `Origin` header. Browsers allow a web page to open a WebSocket to any address, but
//!   always say which page asks; the test's driver is a program, not a page, and sends none.
//! - It refuses a `Host` other than its own loopback address, so that a web address made to point at 127.0.0.1 is not
//!   taken for it.

use std::collections::HashMap;

/// The fewest characters a token may have: 32 characters of base64url, 192 bits.
const SHORTEST: usize = 32;

/// The driver's port and the token every request must carry.
pub struct Gate {
    pub port: u16,
    token: String,
}

impl Gate {
    /// From the environment: `DOTANY_TEST`, `DOTANY_TEST_PORT` and `DOTANY_TEST_TOKEN`, all three or none.
    // only the Mac's driver reads it; elsewhere this module is built for its tests alone
    #[cfg_attr(not(target_os = "macos"), allow(dead_code))]
    pub fn from_env() -> Option<Gate> {
        Gate::new(
            std::env::var_os("DOTANY_TEST").is_some(),
            std::env::var("DOTANY_TEST_PORT").ok().as_deref(),
            std::env::var("DOTANY_TEST_TOKEN").ok().as_deref(),
        )
    }

    /// A token is 32 to 256 characters of base64url (letters, digits, `-`, `_`), which a path carries as they are.
    pub fn new(test: bool, port: Option<&str>, token: Option<&str>) -> Option<Gate> {
        if !test {
            return None;
        }
        let port = port?.trim().parse::<u16>().ok().filter(|&p| p != 0)?;
        let token = token?;
        let fits = (SHORTEST..=256).contains(&token.len())
            && token.bytes().all(|b| b.is_ascii_alphanumeric() || b == b'-' || b == b'_');
        fits.then(|| Gate { port, token: token.to_string() })
    }

    /// What every path the driver hands out starts with.
    pub fn prefix(&self) -> String {
        format!("/{}", self.token)
    }

    /// The request's path past the token (`/json`, `/w/<label>`), or `None` when the request is refused. `headers` are
    /// keyed in lower case.
    pub fn admit<'a>(&self, path: &'a str, headers: &HashMap<String, String>) -> Option<&'a str> {
        if headers.contains_key("origin") {
            return None;
        }
        let host = headers.get("host")?.to_ascii_lowercase();
        let own = [format!("127.0.0.1:{}", self.port), format!("localhost:{}", self.port)];
        if !own.contains(&host) {
            return None;
        }
        let rest = path.strip_prefix('/')?;
        let (token, rest) = match rest.find(['/', '?']) {
            Some(i) => rest.split_at(i),
            None => (rest, ""),
        };
        if !same(token.as_bytes(), self.token.as_bytes()) || !rest.starts_with('/') {
            return None;
        }
        Some(rest)
    }
}

/// Equal, in a time that doesn't depend on where they first differ.
fn same(a: &[u8], b: &[u8]) -> bool {
    a.len() == b.len() && std::hint::black_box(a.iter().zip(b).fold(0u8, |d, (x, y)| d | (x ^ y))) == 0
}

#[cfg(test)]
mod tests {
    use super::*;

    const TOKEN: &str = "Zm9vYmFyYmF6cXV4cXV1eHF1dXhxdXV4";

    fn gate() -> Gate {
        Gate::new(true, Some("9444"), Some(TOKEN)).unwrap()
    }

    fn headers(pairs: &[(&str, &str)]) -> HashMap<String, String> {
        pairs.iter().map(|(k, v)| (k.to_string(), v.to_string())).collect()
    }

    fn node() -> HashMap<String, String> {
        // what Node's fetch and WebSocket send: no Origin
        headers(&[("host", "127.0.0.1:9444"), ("user-agent", "node"), ("sec-fetch-mode", "websocket")])
    }

    #[test]
    fn the_driver_starts_only_with_a_test_a_port_and_a_token() {
        assert!(Gate::new(false, Some("9444"), Some(TOKEN)).is_none());
        assert!(Gate::new(true, None, Some(TOKEN)).is_none());
        assert!(Gate::new(true, Some("9444"), None).is_none());
        assert!(Gate::new(true, Some("9444"), Some("")).is_none());
        assert!(Gate::new(true, Some("9444"), Some("short")).is_none());
        assert!(Gate::new(true, Some("0"), Some(TOKEN)).is_none());
        assert!(Gate::new(true, Some("port"), Some(TOKEN)).is_none());
        // a token a path can't carry as it is
        assert!(Gate::new(true, Some("9444"), Some(&format!("{TOKEN}/x"))).is_none());
        assert!(Gate::new(true, Some("9444"), Some(&format!("{TOKEN}%2F"))).is_none());
        assert!(Gate::new(true, Some("9444"), Some(&"a".repeat(257))).is_none());
        assert_eq!(Gate::new(true, Some(" 9444 "), Some(TOKEN)).map(|g| g.port), Some(9444));
        assert_eq!(gate().prefix(), format!("/{TOKEN}"));
    }

    #[test]
    fn the_test_with_its_token_is_let_in() {
        let g = gate();
        assert_eq!(g.admit(&format!("/{TOKEN}/json"), &node()), Some("/json"));
        assert_eq!(g.admit(&format!("/{TOKEN}/json/list"), &node()), Some("/json/list"));
        assert_eq!(g.admit(&format!("/{TOKEN}/w/main"), &node()), Some("/w/main"));
        assert_eq!(g.admit(&format!("/{TOKEN}/"), &node()), Some("/"));
        let local = headers(&[("host", "localhost:9444")]);
        assert_eq!(g.admit(&format!("/{TOKEN}/json"), &local), Some("/json"));
    }

    #[test]
    fn a_request_without_the_token_is_refused() {
        let g = gate();
        for path in [
            "/json".to_string(),
            "/w/main".to_string(),
            "/".to_string(),
            String::new(),
            format!("/{TOKEN}"),
            format!("/{TOKEN}?json"),
            format!("{TOKEN}/json"),
            format!("/{}/json", &TOKEN[1..]),
            format!("/{TOKEN}x/json"),
            format!("/x{TOKEN}/json"),
            format!("/{}/json", TOKEN.to_ascii_lowercase()),
            format!("//{TOKEN}/json"),
            format!("/json/{TOKEN}"),
        ] {
            assert_eq!(g.admit(&path, &node()), None, "{path}");
        }
    }

    #[test]
    fn a_web_page_is_refused_even_with_the_token() {
        let g = gate();
        let path = format!("/{TOKEN}/w/main");
        for origin in ["https://example.com", "http://127.0.0.1:9444", "null", ""] {
            let mut h = node();
            h.insert("origin".into(), origin.into());
            assert_eq!(g.admit(&path, &h), None, "{origin}");
        }
    }

    #[test]
    fn only_its_own_loopback_address_is_taken_for_it() {
        let g = gate();
        let path = format!("/{TOKEN}/json");
        assert_eq!(g.admit(&path, &headers(&[])), None);
        for host in
            ["example.com", "example.com:9444", "127.0.0.1", "127.0.0.1:9445", "203.0.113.2:9444", "0.0.0.0:9444", ""]
        {
            assert_eq!(g.admit(&path, &headers(&[("host", host)])), None, "{host}");
        }
        assert_eq!(g.admit(&path, &headers(&[("host", "LOCALHOST:9444")])), Some("/json"));
    }

    #[test]
    fn equal_is_byte_for_byte() {
        assert!(same(b"abc", b"abc"));
        assert!(same(b"", b""));
        assert!(!same(b"abc", b"abd"));
        assert!(!same(b"abc", b"ab"));
        assert!(!same(b"abc", b"abcd"));
    }
}
