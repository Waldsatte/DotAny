//! Contact Us › Feedback: when Send is pressed, what was typed goes to `https://dotany.app/feedback` as one small JSON
//! body: `{ "text", "version", "lang", "system" }`, where `system` is "windows" or "macos" and nothing more precise.
//! Nothing else is ever sent from this page, and nothing is sent without the press.
//!
//! Only https to that one host is let through, and no redirect is followed (`net::post`). It is sent once, with a short
//! wait (`net::QUICK`): a person is looking at the button.
//!
//! What the receiver answers, and what the page is told:
//!   200 with `{"result":"accepted"}`   sent (anything else that says 200 is not the receiver: refused)
//!   413                                too long (the page lets no more than `MAX_CHARS` be typed, so not from here)
//!   429                                too many from this address within a minute
//!   503                                the receiver could not keep it just now
//!   any other answer                   refused
//!   no answer                          offline
//!
//! Until it is sent, the text is kept in DotAny's folder as `feedback/draft.json`: a message that could not be sent is
//! there again the next time the page opens, and once one is sent the draft is removed.
//!
//! Tests: `DOTANY_FEEDBACK_URL` names a receiver on this computer (`http://127.0.0.1:…/…` or `localhost`); any other
//! address there is ignored.

use crate::net::{self, Fail, Url};
use std::path::{Path, PathBuf};

pub const ADDRESS: &str = "https://dotany.app/feedback";

/// The receiver keeps at most this many characters (Unicode scalar values). The page's box allows this many UTF-16
/// units, which is never more characters.
pub const MAX_CHARS: usize = 4000;

#[derive(Debug, Clone, Copy, PartialEq)]
pub enum Sent {
    Sent,
    Offline,
    TooLong,
    TooMany,
    Later,
    Refused,
}

impl Sent {
    /// The word the page gets.
    pub fn word(self) -> &'static str {
        match self {
            Sent::Sent => "sent",
            Sent::Offline => "offline",
            Sent::TooLong => "long",
            Sent::TooMany => "many",
            Sent::Later => "later",
            Sent::Refused => "refused",
        }
    }
}

/// Where a message goes: the real address, or a test receiver on this computer.
pub struct Place {
    pub url: String,
    pub local: bool,
}

impl Place {
    pub fn from_env() -> Place {
        match std::env::var("DOTANY_FEEDBACK_URL") {
            Ok(u) if net::parse(&u).is_some_and(|p| crate::update::is_local(&p.host)) => Place { url: u, local: true },
            _ => Place { url: ADDRESS.to_string(), local: false },
        }
    }

    /// https to dotany.app on its usual port; http only to a test receiver here.
    pub fn allowed(&self, u: &Url) -> bool {
        (u.secure && u.host == "dotany.app" && u.port == 443) || (self.local && crate::update::is_local(&u.host))
    }
}

pub fn system() -> &'static str {
    if cfg!(target_os = "macos") { "macos" } else { "windows" }
}

/// The body sent: the four fields, nothing else.
pub fn body(text: &str, version: &str, lang: &str) -> Vec<u8> {
    serde_json::json!({ "text": text, "version": version, "lang": lang, "system": system() }).to_string().into_bytes()
}

/// What an answer means.
pub fn outcome(status: u16, answer: &[u8]) -> Sent {
    match status {
        200 => {
            let said = serde_json::from_slice::<serde_json::Value>(answer).ok();
            if said.as_ref().and_then(|v| v.get("result")).and_then(|r| r.as_str()) == Some("accepted") {
                Sent::Sent
            } else {
                Sent::Refused
            }
        }
        413 => Sent::TooLong,
        429 => Sent::TooMany,
        503 => Sent::Later,
        _ => Sent::Refused,
    }
}

/// Sends one message and says how it went. An empty text is not sent; one over `MAX_CHARS` is not either.
pub fn send(place: &Place, agent: &str, text: &str, version: &str, lang: &str) -> Sent {
    if text.trim().is_empty() {
        return Sent::Refused;
    }
    if text.chars().count() > MAX_CHARS {
        return Sent::TooLong;
    }
    let allowed = |u: &Url| place.allowed(u);
    match net::post(agent, &place.url, &allowed, "application/json", &body(text, version, lang), net::QUICK) {
        Ok((status, answer)) => outcome(status, &answer),
        Err(Fail::Offline(_)) => Sent::Offline,
        Err(_) => Sent::Refused,
    }
}

// ---------- the draft ----------

fn draft_path(data_dir: &Path) -> PathBuf {
    data_dir.join("feedback").join("draft.json")
}

/// Keeps the text until it is sent; false when it could not be written.
pub fn keep_draft(data_dir: &Path, text: &str) -> bool {
    let path = draft_path(data_dir);
    let json = serde_json::to_vec_pretty(&serde_json::json!({ "text": text })).unwrap_or_default();
    std::fs::create_dir_all(path.parent().unwrap()).is_ok() && dotany_base::atomic::atomic_write(&path, &json).is_ok()
}

/// The text not sent yet, if there is one.
pub fn draft(data_dir: &Path) -> Option<String> {
    let bytes = std::fs::read(draft_path(data_dir)).ok()?;
    let v: serde_json::Value = serde_json::from_slice(&bytes).ok()?;
    v.get("text")?.as_str().filter(|t| !t.trim().is_empty()).map(str::to_string)
}

pub fn drop_draft(data_dir: &Path) {
    let _ = std::fs::remove_file(draft_path(data_dir));
}

/// The whole of Send: the draft first, then the message; the draft goes once the receiver has it.
pub fn send_and_keep(place: &Place, agent: &str, data_dir: &Path, text: &str, version: &str, lang: &str) -> Sent {
    let _ = keep_draft(data_dir, text);
    let sent = send(place, agent, text, version, lang);
    if sent == Sent::Sent {
        drop_draft(data_dir);
    }
    sent
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::{Read, Write};
    use std::sync::{Arc, Mutex};
    use std::time::Duration;

    #[test]
    fn only_dotany_app_over_https_or_a_test_receiver_here() {
        let real = Place { url: ADDRESS.into(), local: false };
        let ok = |p: &Place, u: &str| p.allowed(&net::parse(u).unwrap());
        assert!(ok(&real, "https://dotany.app/feedback"));
        assert!(!ok(&real, "http://dotany.app/feedback"));
        assert!(!ok(&real, "https://dotany.app:8443/feedback"));
        assert!(!ok(&real, "https://www.dotany.app/feedback"));
        assert!(!ok(&real, "https://dotany.app.example.com/feedback"));
        assert!(!ok(&real, "https://github.com/feedback"));
        assert!(!ok(&real, "http://127.0.0.1:9/feedback"));
        let test = Place { url: "http://127.0.0.1:9/feedback".into(), local: true };
        assert!(ok(&test, "http://127.0.0.1:9/feedback"));
        assert!(ok(&test, "http://localhost:9/feedback"));
        assert!(!ok(&test, "http://10.0.0.2:9/feedback"));
    }

    #[test]
    fn a_test_address_is_taken_only_on_this_computer() {
        // (one test touches the variable, so tests running side by side see no change)
        std::env::set_var("DOTANY_FEEDBACK_URL", "http://127.0.0.1:5/feedback");
        let p = Place::from_env();
        assert_eq!((p.url.as_str(), p.local), ("http://127.0.0.1:5/feedback", true));
        std::env::set_var("DOTANY_FEEDBACK_URL", "https://example.com/feedback");
        let p = Place::from_env();
        assert_eq!((p.url.as_str(), p.local), (ADDRESS, false));
        std::env::remove_var("DOTANY_FEEDBACK_URL");
        assert_eq!(Place::from_env().url, ADDRESS);
    }

    #[test]
    fn the_body_is_the_four_fields() {
        let v: serde_json::Value = serde_json::from_slice(&body("<b>好</b> 👍", "1.0.0", "zh")).unwrap();
        assert_eq!(
            v,
            serde_json::json!({ "text": "<b>好</b> 👍", "version": "1.0.0", "lang": "zh", "system": system() })
        );
        assert!(matches!(system(), "windows" | "macos"));
    }

    #[test]
    fn what_an_answer_means() {
        assert_eq!(outcome(200, br#"{"result":"accepted"}"#), Sent::Sent);
        assert_eq!(outcome(200, b"<html>a page</html>"), Sent::Refused);
        assert_eq!(outcome(200, b""), Sent::Refused);
        assert_eq!(outcome(413, b""), Sent::TooLong);
        assert_eq!(outcome(429, b""), Sent::TooMany);
        assert_eq!(outcome(503, b""), Sent::Later);
        for s in [301, 302, 308, 400, 403, 404, 405, 415, 500] {
            assert_eq!(outcome(s, br#"{"result":"accepted"}"#), Sent::Refused, "{s}");
        }
    }

    /// One request heard by a receiver on this computer: its first line, its headers and its body.
    #[derive(Clone, Debug, Default)]
    struct Heard {
        line: String,
        head: String,
        body: Vec<u8>,
    }

    /// A receiver answering each request with `status` and `answer` (or never, when `silent`).
    fn receiver(status: u16, answer: &'static str, silent: bool) -> (String, Arc<Mutex<Vec<Heard>>>) {
        let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
        let url = format!("http://127.0.0.1:{}/feedback", listener.local_addr().unwrap().port());
        let heard = Arc::new(Mutex::new(Vec::new()));
        let log = heard.clone();
        std::thread::spawn(move || {
            for conn in listener.incoming().flatten() {
                let log = log.clone();
                std::thread::spawn(move || {
                    let mut conn = conn;
                    let (mut got, mut buf) = (Vec::new(), [0u8; 4096]);
                    let end = loop {
                        if let Some(i) = got.windows(4).position(|w| w == b"\r\n\r\n") {
                            break i + 4;
                        }
                        match conn.read(&mut buf) {
                            Ok(0) | Err(_) => return,
                            Ok(n) => got.extend_from_slice(&buf[..n]),
                        }
                    };
                    let head = String::from_utf8_lossy(&got[..end]).into_owned();
                    let len = head
                        .lines()
                        .find_map(|l| {
                            l.to_ascii_lowercase().strip_prefix("content-length:").map(|v| v.trim().to_string())
                        })
                        .and_then(|v| v.parse::<usize>().ok())
                        .unwrap_or(0);
                    while got.len() < end + len {
                        match conn.read(&mut buf) {
                            Ok(0) | Err(_) => break,
                            Ok(n) => got.extend_from_slice(&buf[..n]),
                        }
                    }
                    log.lock().unwrap().push(Heard {
                        line: head.lines().next().unwrap_or("").to_string(),
                        head: head.clone(),
                        body: got[end..].to_vec(),
                    });
                    if silent {
                        std::thread::sleep(Duration::from_secs(20));
                        return;
                    }
                    let _ = write!(
                        conn,
                        "HTTP/1.1 {status} X\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{answer}",
                        answer.len()
                    );
                });
            }
        });
        (url, heard)
    }

    struct Scratch(PathBuf);
    impl Scratch {
        fn new(name: &str) -> Scratch {
            let d = std::env::temp_dir().join(format!("dotany-feedback-{name}-{}", std::process::id()));
            let _ = std::fs::remove_dir_all(&d);
            std::fs::create_dir_all(&d).unwrap();
            Scratch(d)
        }
    }
    impl Drop for Scratch {
        fn drop(&mut self) {
            let _ = std::fs::remove_dir_all(&self.0);
        }
    }

    const AGENT: &str = "DotAny/test";

    #[cfg(any(windows, target_os = "macos"))]
    #[test]
    fn sent_the_receiver_gets_the_four_fields_and_the_draft_goes() {
        let (url, heard) = receiver(200, r#"{"result":"accepted"}"#, false);
        let place = Place { url, local: true };
        let dir = Scratch::new("sent");
        let text = "<script>x</script> 很好用 👍🏽\n第二行";
        assert_eq!(send_and_keep(&place, AGENT, &dir.0, text, "1.0.0", "zh"), Sent::Sent);
        let h = heard.lock().unwrap().clone();
        assert_eq!(h.len(), 1);
        assert_eq!(h[0].line, "POST /feedback HTTP/1.1");
        assert!(h[0].head.to_ascii_lowercase().contains("content-type: application/json\r\n"), "{}", h[0].head);
        let v: serde_json::Value = serde_json::from_slice(&h[0].body).unwrap();
        assert_eq!(v, serde_json::json!({ "text": text, "version": "1.0.0", "lang": "zh", "system": system() }));
        assert_eq!(draft(&dir.0), None);
        assert!(!dir.0.join("feedback").join("draft.json").exists());
    }

    #[cfg(any(windows, target_os = "macos"))]
    #[test]
    fn refused_or_offline_the_text_stays_as_the_draft() {
        for (status, answer, want) in [
            (429, r#"{"result":"too_many"}"#, Sent::TooMany),
            (503, r#"{"result":"later"}"#, Sent::Later),
            (413, r#"{"result":"too_long"}"#, Sent::TooLong),
            (404, "not here", Sent::Refused),
            (200, "<html>a website</html>", Sent::Refused),
        ] {
            let (url, _) = receiver(status, answer, false);
            let dir = Scratch::new(&format!("refused-{status}"));
            let got = send_and_keep(&Place { url, local: true }, AGENT, &dir.0, "留着", "1.0.0", "zh");
            assert_eq!(got, want, "{status}");
            assert_eq!(draft(&dir.0).as_deref(), Some("留着"), "{status}");
        }
        // nobody there
        let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
        let url = format!("http://127.0.0.1:{}/feedback", listener.local_addr().unwrap().port());
        drop(listener);
        let dir = Scratch::new("offline");
        assert_eq!(send_and_keep(&Place { url, local: true }, AGENT, &dir.0, "留着", "1.0.0", "zh"), Sent::Offline);
        assert_eq!(draft(&dir.0).as_deref(), Some("留着"));
        // a later message that is sent takes the draft away
        let (url, _) = receiver(200, r#"{"result":"accepted"}"#, false);
        assert_eq!(send_and_keep(&Place { url, local: true }, AGENT, &dir.0, "留着", "1.0.0", "zh"), Sent::Sent);
        assert_eq!(draft(&dir.0), None);
    }

    #[cfg(any(windows, target_os = "macos"))]
    #[test]
    fn a_receiver_that_never_answers_costs_only_the_short_wait() {
        let (url, heard) = receiver(200, "", true);
        let started = std::time::Instant::now();
        let dir = Scratch::new("silent");
        assert_eq!(send_and_keep(&Place { url, local: true }, AGENT, &dir.0, "x", "1.0.0", "en"), Sent::Offline);
        assert_eq!(heard.lock().unwrap().len(), 1);
        assert!(started.elapsed() < Duration::from_secs(15), "{:?}", started.elapsed());
    }

    /// A receiver over https with the system's own network code and proxy, by hand:
    /// `DOTANY_FEEDBACK_HTTPS=https://<receiver>/feedback cargo test -p dotany-hub --lib -- --ignored over_https`.
    /// It sends one message.
    #[cfg(any(windows, target_os = "macos"))]
    #[test]
    #[ignore]
    fn a_real_receiver_takes_one_over_https() {
        let url = std::env::var("DOTANY_FEEDBACK_HTTPS").expect("DOTANY_FEEDBACK_HTTPS");
        let host = net::parse(&url).expect("an address").host;
        let allowed = |u: &Url| u.secure && u.host == host;
        let text = format!("hub unit test over https, {} <b>好</b> 👍🏽", system());
        let (status, answer) =
            net::post(AGENT, &url, &allowed, "application/json", &body(&text, "1.0.0", "en"), net::QUICK).unwrap();
        assert_eq!(outcome(status, &answer), Sent::Sent, "{status} {}", String::from_utf8_lossy(&answer));
    }

    #[test]
    fn nothing_leaves_for_another_address_or_an_empty_or_long_text() {
        // a test address that is not on this computer is refused before anything is asked
        let place = Place { url: "http://192.0.2.1:9/feedback".into(), local: true };
        assert_eq!(send(&place, AGENT, "x", "1.0.0", "en"), Sent::Refused);
        let (url, heard) = receiver(200, r#"{"result":"accepted"}"#, false);
        let place = Place { url, local: true };
        assert_eq!(send(&place, AGENT, "  \n ", "1.0.0", "en"), Sent::Refused);
        assert_eq!(send(&place, AGENT, &"字".repeat(MAX_CHARS + 1), "1.0.0", "en"), Sent::TooLong);
        assert!(heard.lock().unwrap().is_empty());
    }
}
