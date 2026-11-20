//! Fetching a file over https with Windows' own WinHTTP, and with the Mac's own NSURLSession there (`mac` below), each
//! under the same rules.
//!
//! Why WinHTTP and not ureq with rustls: it is part of Windows, so DotAny grows by a few KB instead of 1 to 1.5 MB; it
//! uses the system's proxy settings (WINHTTP_ACCESS_TYPE_AUTOMATIC_PROXY: the same proxy, PAC script and auto-detect as
//! Windows itself) and the system's certificate store, so it works in an office whose proxy inspects TLS with its own
//! root certificate, where rustls with its built-in roots would refuse. ureq also doesn't read Windows' proxy settings.
//!
//! Redirects are followed here, one at a time, never by WinHTTP: every address on the way is checked by `allowed`
//! before anything is asked of it. GitHub's `releases/latest/download/x` goes github.com → github.com → its file host.
//!
//! How long to wait is the caller's (`Patience`): a place with another one after it is given up quickly, so a site that
//! doesn't answer costs seconds before GitHub is asked (update.rs, `Source`).
//!
//! `post` sends one small body (the Contact page's message, feedback.rs) under the same rules, except that it follows
//! no redirect at all: the status comes back as it is.

#[cfg(windows)]
use std::ffi::c_void;
#[cfg(windows)]
use windows_sys::Win32::Foundation::GetLastError;
#[cfg(windows)]
use windows_sys::Win32::Networking::WinHttp::*;

#[derive(Debug, Clone, PartialEq)]
pub struct Url {
    pub secure: bool,
    pub host: String,
    pub port: u16,
    /// The path and query, from the first "/".
    pub path: String,
}

/// `https://host[:port]/path`, or `http://` (only ever let through for a test server on this computer).
pub fn parse(url: &str) -> Option<Url> {
    let (secure, rest) = if let Some(r) = url.strip_prefix("https://") {
        (true, r)
    } else if let Some(r) = url.strip_prefix("http://") {
        (false, r)
    } else {
        return None;
    };
    let (authority, path) = match rest.find('/') {
        Some(i) => (&rest[..i], &rest[i..]),
        None => (rest, "/"),
    };
    // no user:password@, no IPv6 brackets: neither is ever ours
    if authority.is_empty() || authority.contains('@') || authority.contains('[') {
        return None;
    }
    let (host, port) = match authority.rsplit_once(':') {
        Some((h, p)) => (h, p.parse().ok()?),
        None => (authority, if secure { 443 } else { 80 }),
    };
    if host.is_empty() || !host.chars().all(|c| c.is_ascii_alphanumeric() || c == '.' || c == '-') {
        return None;
    }
    Some(Url { secure, host: host.to_ascii_lowercase(), port, path: path.to_string() })
}

/// How long a place is waited for before it counts as not answering.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Patience {
    /// To find the name and to connect (Windows; the Mac counts these in `idle`).
    pub connect_ms: u32,
    /// Without a byte, once connected: to send the request, for the answer, between pieces.
    pub idle_ms: u32,
}

/// The last place there is to ask, and every download of the installer itself.
pub const PATIENT: Patience = Patience { connect_ms: 15_000, idle_ms: 30_000 };
/// A place with another one after it: a small file that hasn't started in 10 seconds isn't coming.
pub const QUICK: Patience = Patience { connect_ms: 5_000, idle_ms: 10_000 };

#[derive(Debug, Clone, PartialEq)]
pub enum Fail {
    /// No answer: no network, the name didn't resolve, the connection or TLS failed, it took too long.
    Offline(String),
    /// An answer, but not the file: an address we don't go to, a status other than 200, too big.
    Refused(String),
    /// The receiver stopped it (it wanted no more).
    Stopped,
}

#[cfg(windows)]
struct Handle(*mut c_void);
#[cfg(windows)]
impl Drop for Handle {
    fn drop(&mut self) {
        if !self.0.is_null() {
            unsafe { WinHttpCloseHandle(self.0) };
        }
    }
}

#[cfg(windows)]
fn wide(s: &str) -> Vec<u16> {
    s.encode_utf16().chain(Some(0)).collect()
}

#[cfg(windows)]
fn offline(what: &str) -> Fail {
    Fail::Offline(format!("{what} ({})", unsafe { GetLastError() }))
}

#[cfg(windows)]
fn session(agent: &str, wait: Patience) -> Result<Handle, Fail> {
    let agent = wide(agent);
    unsafe {
        // the system's proxy (Windows 8.1 and later); the older default where that isn't known
        let mut s =
            WinHttpOpen(agent.as_ptr(), WINHTTP_ACCESS_TYPE_AUTOMATIC_PROXY, std::ptr::null(), std::ptr::null(), 0);
        if s.is_null() {
            s = WinHttpOpen(agent.as_ptr(), WINHTTP_ACCESS_TYPE_DEFAULT_PROXY, std::ptr::null(), std::ptr::null(), 0);
        }
        if s.is_null() {
            return Err(offline("WinHttpOpen"));
        }
        // name lookup, connect, send, each read
        let (c, i) = (wait.connect_ms as i32, wait.idle_ms as i32);
        WinHttpSetTimeouts(s, c, c, i, i);
        let tls: u32 = WINHTTP_FLAG_SECURE_PROTOCOL_TLS1_2 | WINHTTP_FLAG_SECURE_PROTOCOL_TLS1_3;
        // TLS 1.3 isn't known before Windows 11: then 1.2 alone
        if WinHttpSetOption(s, WINHTTP_OPTION_SECURE_PROTOCOLS, &tls as *const u32 as *const c_void, 4) == 0 {
            let tls: u32 = WINHTTP_FLAG_SECURE_PROTOCOL_TLS1_2;
            WinHttpSetOption(s, WINHTTP_OPTION_SECURE_PROTOCOLS, &tls as *const u32 as *const c_void, 4);
        }
        Ok(Handle(s))
    }
}

#[cfg(windows)]
fn header(req: &Handle, which: u32) -> Option<String> {
    let mut buf = vec![0u16; 4096];
    let mut len = (buf.len() * 2) as u32;
    let ok = unsafe {
        WinHttpQueryHeaders(
            req.0,
            which,
            std::ptr::null(),
            buf.as_mut_ptr() as *mut c_void,
            &mut len,
            std::ptr::null_mut(),
        )
    };
    if ok == 0 {
        return None;
    }
    Some(String::from_utf16_lossy(&buf[..(len as usize / 2).min(buf.len())]))
}

/// GETs `url`, following up to five redirects, each one checked by `allowed`. `body` gets each piece as it comes, with
/// the whole length when the server says it, and answers whether to go on.
#[cfg(windows)]
pub fn get(
    agent: &str,
    url: &str,
    allowed: &dyn Fn(&Url) -> bool,
    max: u64,
    wait: Patience,
    body: &mut dyn FnMut(&[u8], Option<u64>) -> bool,
) -> Result<(), Fail> {
    let s = session(agent, wait)?;
    let mut at = parse(url).ok_or_else(|| Fail::Refused(format!("not an address: {url}")))?;
    for _ in 0..6 {
        if !allowed(&at) {
            return Err(Fail::Refused(format!(
                "not allowed: {}://{}",
                if at.secure { "https" } else { "http" },
                at.host
            )));
        }
        let host = wide(&at.host);
        let conn = Handle(unsafe { WinHttpConnect(s.0, host.as_ptr(), at.port, 0) });
        if conn.0.is_null() {
            return Err(offline("WinHttpConnect"));
        }
        let (verb, path) = (wide("GET"), wide(&at.path));
        let flags = if at.secure { WINHTTP_FLAG_SECURE } else { 0 };
        let req = Handle(unsafe {
            WinHttpOpenRequest(
                conn.0,
                verb.as_ptr(),
                path.as_ptr(),
                std::ptr::null(),
                std::ptr::null(),
                std::ptr::null(),
                flags,
            )
        });
        if req.0.is_null() {
            return Err(offline("WinHttpOpenRequest"));
        }
        let no_redirects: u32 = WINHTTP_DISABLE_REDIRECTS;
        unsafe {
            WinHttpSetOption(req.0, WINHTTP_OPTION_DISABLE_FEATURE, &no_redirects as *const u32 as *const c_void, 4);
            if WinHttpSendRequest(req.0, std::ptr::null(), 0, std::ptr::null(), 0, 0, 0) == 0 {
                return Err(offline("WinHttpSendRequest"));
            }
            if WinHttpReceiveResponse(req.0, std::ptr::null_mut()) == 0 {
                return Err(offline("WinHttpReceiveResponse"));
            }
        }
        let mut code = 0u32;
        let mut len = 4u32;
        if unsafe {
            WinHttpQueryHeaders(
                req.0,
                WINHTTP_QUERY_STATUS_CODE | WINHTTP_QUERY_FLAG_NUMBER,
                std::ptr::null(),
                &mut code as *mut u32 as *mut c_void,
                &mut len,
                std::ptr::null_mut(),
            )
        } == 0
        {
            return Err(offline("status"));
        }
        if matches!(code, 301 | 302 | 303 | 307 | 308) {
            let to = header(&req, WINHTTP_QUERY_LOCATION)
                .ok_or_else(|| Fail::Refused("a redirect without a Location".into()))?;
            at = if to.starts_with('/') {
                Url { path: to, ..at }
            } else {
                parse(&to).ok_or_else(|| Fail::Refused(format!("a redirect to {to}")))?
            };
            continue;
        }
        if code != 200 {
            return Err(Fail::Refused(format!("HTTP {code}")));
        }
        let total = header(&req, WINHTTP_QUERY_CONTENT_LENGTH).and_then(|v| v.trim().parse::<u64>().ok());
        if total.map_or(false, |t| t > max) {
            return Err(Fail::Refused(format!("{} bytes is more than {max}", total.unwrap())));
        }
        let mut buf = vec![0u8; 64 * 1024];
        let mut got = 0u64;
        loop {
            let mut read = 0u32;
            if unsafe { WinHttpReadData(req.0, buf.as_mut_ptr() as *mut c_void, buf.len() as u32, &mut read) } == 0 {
                return Err(offline("WinHttpReadData"));
            }
            if read == 0 {
                break;
            }
            got += read as u64;
            if got > max {
                return Err(Fail::Refused(format!("more than {max} bytes")));
            }
            if !body(&buf[..read as usize], total) {
                return Err(Fail::Stopped);
            }
        }
        if total.map_or(false, |t| t != got) {
            return Err(Fail::Offline(format!("{got} of {} bytes", total.unwrap())));
        }
        return Ok(());
    }
    Err(Fail::Refused("too many redirects".into()))
}

/// POSTs `body` to `url` and answers the status code with the start of the answer's body (up to `ANSWER` bytes; the
/// rest is read and dropped). One address only, checked by `allowed` first; a redirect is never followed: it comes back
/// as its 3xx status, for the caller to refuse.
#[cfg(windows)]
pub fn post(
    agent: &str,
    url: &str,
    allowed: &dyn Fn(&Url) -> bool,
    content_type: &str,
    body: &[u8],
    wait: Patience,
) -> Result<(u16, Vec<u8>), Fail> {
    let at = parse(url).ok_or_else(|| Fail::Refused(format!("not an address: {url}")))?;
    if !allowed(&at) {
        return Err(Fail::Refused(format!("not allowed: {}://{}", if at.secure { "https" } else { "http" }, at.host)));
    }
    let len = u32::try_from(body.len()).map_err(|_| Fail::Refused("too big to send".into()))?;
    let s = session(agent, wait)?;
    let host = wide(&at.host);
    let conn = Handle(unsafe { WinHttpConnect(s.0, host.as_ptr(), at.port, 0) });
    if conn.0.is_null() {
        return Err(offline("WinHttpConnect"));
    }
    let (verb, path) = (wide("POST"), wide(&at.path));
    let flags = if at.secure { WINHTTP_FLAG_SECURE } else { 0 };
    let req = Handle(unsafe {
        WinHttpOpenRequest(
            conn.0,
            verb.as_ptr(),
            path.as_ptr(),
            std::ptr::null(),
            std::ptr::null(),
            std::ptr::null(),
            flags,
        )
    });
    if req.0.is_null() {
        return Err(offline("WinHttpOpenRequest"));
    }
    let headers = wide(&format!("Content-Type: {content_type}\r\n"));
    let no_redirects: u32 = WINHTTP_DISABLE_REDIRECTS;
    unsafe {
        WinHttpSetOption(req.0, WINHTTP_OPTION_DISABLE_FEATURE, &no_redirects as *const u32 as *const c_void, 4);
        // the body is sent with the request (it is small): WinHTTP keeps the pointer only for this call
        if WinHttpSendRequest(
            req.0,
            headers.as_ptr(),
            u32::MAX, // -1: the headers end at their NUL
            body.as_ptr() as *const c_void,
            len,
            len,
            0,
        ) == 0
        {
            return Err(offline("WinHttpSendRequest"));
        }
        if WinHttpReceiveResponse(req.0, std::ptr::null_mut()) == 0 {
            return Err(offline("WinHttpReceiveResponse"));
        }
    }
    let mut code = 0u32;
    let mut size = 4u32;
    if unsafe {
        WinHttpQueryHeaders(
            req.0,
            WINHTTP_QUERY_STATUS_CODE | WINHTTP_QUERY_FLAG_NUMBER,
            std::ptr::null(),
            &mut code as *mut u32 as *mut c_void,
            &mut size,
            std::ptr::null_mut(),
        )
    } == 0
    {
        return Err(offline("status"));
    }
    // the answer's body: its start is kept; read to its end (at most 64 KB) so the connection ends cleanly
    let (mut buf, mut answer, mut got) = ([0u8; 4096], Vec::new(), 0usize);
    while got < 64 * 1024 {
        let mut read = 0u32;
        if unsafe { WinHttpReadData(req.0, buf.as_mut_ptr() as *mut c_void, buf.len() as u32, &mut read) } == 0
            || read == 0
        {
            break;
        }
        got += read as usize;
        keep(&mut answer, &buf[..read as usize]);
    }
    Ok((code as u16, answer))
}

/// The Mac fetches with its own URL loading (`NSURLSession`), for the same reasons Windows uses WinHTTP: it is part of the
/// system, uses the system's proxy settings (System Settings › Network, a PAC script included) and its certificates
/// (the keychain's, so an office's own root works), and costs nothing to ship. Its rules are WinHTTP's here: no
/// cookies or cache, `Patience::idle_ms` without a byte (30 seconds, or 10 for a place with another after it) is a
/// failure, redirects are followed here one at a time, each address
/// checked by `allowed` before it is asked, at most five, a length over `max` refused before or as it comes.
#[cfg(target_os = "macos")]
mod mac {
    use super::{Fail, Patience, Url};
    use objc2::rc::Retained;
    use objc2::runtime::{NSObject, NSObjectProtocol, ProtocolObject};
    use objc2::{AllocAnyThread, DefinedClass, define_class, msg_send};
    use objc2_foundation::{
        NSData, NSError, NSHTTPURLResponse, NSString, NSURL, NSURLRequest, NSURLRequestCachePolicy, NSURLResponse,
        NSURLSession, NSURLSessionConfiguration, NSURLSessionDataDelegate, NSURLSessionDataTask, NSURLSessionDelegate,
        NSURLSessionResponseDisposition, NSURLSessionTask, NSURLSessionTaskDelegate,
    };
    use std::sync::mpsc::{Receiver, RecvTimeoutError, SyncSender, sync_channel};
    use std::time::Duration;

    /// What the session's delegate hears, passed to the thread that asked.
    enum Heard {
        Head { status: isize, location: Option<String>, length: Option<u64> },
        Data(Vec<u8>),
        End(Option<String>),
    }

    struct Ivars {
        tx: SyncSender<Heard>,
    }

    define_class!(
        // SAFETY: NSObject has no rules for subclasses; the delegate keeps only a sender
        #[unsafe(super(NSObject))]
        #[name = "DotAnyFetchDelegate"]
        #[ivars = Ivars]
        struct Delegate;

        unsafe impl NSObjectProtocol for Delegate {}
        unsafe impl NSURLSessionDelegate for Delegate {}

        unsafe impl NSURLSessionTaskDelegate for Delegate {
            /// Never followed by the system: the redirect itself comes back as the answer, and `get` decides.
            #[unsafe(method(URLSession:task:willPerformHTTPRedirection:newRequest:completionHandler:))]
            fn redirect(
                &self,
                _session: &NSURLSession,
                _task: &NSURLSessionTask,
                _response: &NSHTTPURLResponse,
                _request: &NSURLRequest,
                handler: &block2::DynBlock<dyn Fn(*mut NSURLRequest)>,
            ) {
                handler.call((std::ptr::null_mut(),));
            }

            #[unsafe(method(URLSession:task:didCompleteWithError:))]
            fn done(&self, _session: &NSURLSession, _task: &NSURLSessionTask, error: Option<&NSError>) {
                let _ = self.ivars().tx.send(Heard::End(error.map(|e| e.localizedDescription().to_string())));
            }
        }

        unsafe impl NSURLSessionDataDelegate for Delegate {
            #[unsafe(method(URLSession:dataTask:didReceiveResponse:completionHandler:))]
            fn response(
                &self,
                _session: &NSURLSession,
                _task: &NSURLSessionDataTask,
                response: &NSURLResponse,
                handler: &block2::DynBlock<dyn Fn(NSURLSessionResponseDisposition)>,
            ) {
                let http = response.downcast_ref::<NSHTTPURLResponse>();
                let status = http.map_or(0, |h| h.statusCode());
                let location = http
                    .and_then(|h| h.valueForHTTPHeaderField(&NSString::from_str("Location")))
                    .map(|s| s.to_string());
                let length = u64::try_from(response.expectedContentLength()).ok();
                let _ = self.ivars().tx.send(Heard::Head { status, location, length });
                handler.call((NSURLSessionResponseDisposition::Allow,));
            }

            #[unsafe(method(URLSession:dataTask:didReceiveData:))]
            fn data(&self, _session: &NSURLSession, _task: &NSURLSessionDataTask, data: &NSData) {
                let _ = self.ivars().tx.send(Heard::Data(data.to_vec()));
            }
        }
    );

    impl Delegate {
        fn new(tx: SyncSender<Heard>) -> Retained<Self> {
            let this = Self::alloc().set_ivars(Ivars { tx });
            // SAFETY: NSObject's init on a freshly allocated object
            unsafe { msg_send![super(this), init] }
        }
    }

    /// One request, its answer as it comes. The session goes when this does.
    struct Asked {
        session: Retained<NSURLSession>,
        task: Retained<NSURLSessionDataTask>,
        rx: Receiver<Heard>,
    }

    impl Drop for Asked {
        fn drop(&mut self) {
            self.task.cancel();
            // the session holds its delegate until it is let go
            self.session.invalidateAndCancel();
        }
    }

    /// A GET, or with `send` (its content type and bytes) a POST.
    fn ask(agent: &str, at: &Url, wait: Patience, send: Option<(&str, &[u8])>) -> Result<Asked, Fail> {
        let address = format!("{}://{}:{}{}", if at.secure { "https" } else { "http" }, at.host, at.port, at.path);
        let url = NSURL::URLWithString(&NSString::from_str(&address))
            .ok_or_else(|| Fail::Refused(format!("not an address: {address}")))?;
        let (tx, rx) = sync_channel::<Heard>(32);
        let delegate = Delegate::new(tx);
        let config = NSURLSessionConfiguration::ephemeralSessionConfiguration();
        // so long without a byte (finding the name and connecting included), as WinHTTP's receive; the whole may take
        // as long as it takes
        let idle = f64::from(wait.idle_ms) / 1000.0;
        config.setTimeoutIntervalForRequest(idle);
        // SAFETY: no queue given, so the session makes its own serial one for the delegate
        let session = unsafe {
            NSURLSession::sessionWithConfiguration_delegate_delegateQueue(
                &config,
                Some(ProtocolObject::from_ref(&*delegate)),
                None,
            )
        };
        let request = objc2_foundation::NSMutableURLRequest::requestWithURL_cachePolicy_timeoutInterval(
            &url,
            NSURLRequestCachePolicy::ReloadIgnoringLocalCacheData,
            idle,
        );
        request.setValue_forHTTPHeaderField(Some(&NSString::from_str(agent)), &NSString::from_str("User-Agent"));
        if let Some((kind, bytes)) = send {
            request.setHTTPMethod(&NSString::from_str("POST"));
            request.setValue_forHTTPHeaderField(Some(&NSString::from_str(kind)), &NSString::from_str("Content-Type"));
            request.setHTTPBody(Some(&NSData::with_bytes(bytes)));
        }
        let task = session.dataTaskWithRequest(&request);
        task.resume();
        Ok(Asked { session, task, rx })
    }

    fn hear(asked: &Asked, wait: Patience) -> Result<Heard, Fail> {
        // the system's own timeout says so first; this is only for a session that never answers at all
        match asked.rx.recv_timeout(Duration::from_millis(u64::from(wait.idle_ms) * 3)) {
            Ok(h) => Ok(h),
            Err(RecvTimeoutError::Timeout) => Err(Fail::Offline("no answer".into())),
            Err(RecvTimeoutError::Disconnected) => Err(Fail::Offline("the session went".into())),
        }
    }

    pub fn get(
        agent: &str,
        url: &str,
        allowed: &dyn Fn(&Url) -> bool,
        max: u64,
        wait: Patience,
        body: &mut dyn FnMut(&[u8], Option<u64>) -> bool,
    ) -> Result<(), Fail> {
        let mut at = super::parse(url).ok_or_else(|| Fail::Refused(format!("not an address: {url}")))?;
        for _ in 0..6 {
            if !allowed(&at) {
                return Err(Fail::Refused(format!(
                    "not allowed: {}://{}",
                    if at.secure { "https" } else { "http" },
                    at.host
                )));
            }
            let asked = ask(agent, &at, wait, None)?;
            let (status, location, total) = match hear(&asked, wait)? {
                Heard::Head { status, location, length } => (status, location, length),
                Heard::End(e) => return Err(Fail::Offline(e.unwrap_or_else(|| "no answer".into()))),
                Heard::Data(_) => return Err(Fail::Offline("data before an answer".into())),
            };
            if matches!(status, 301 | 302 | 303 | 307 | 308) {
                let to = location.ok_or_else(|| Fail::Refused("a redirect without a Location".into()))?;
                at = if to.starts_with('/') {
                    Url { path: to, ..at }
                } else {
                    super::parse(&to).ok_or_else(|| Fail::Refused(format!("a redirect to {to}")))?
                };
                continue;
            }
            if status != 200 {
                return Err(Fail::Refused(format!("HTTP {status}")));
            }
            if total.is_some_and(|t| t > max) {
                return Err(Fail::Refused(format!("{} bytes is more than {max}", total.unwrap())));
            }
            let mut got = 0u64;
            loop {
                match hear(&asked, wait)? {
                    Heard::Data(piece) => {
                        got += piece.len() as u64;
                        if got > max {
                            return Err(Fail::Refused(format!("more than {max} bytes")));
                        }
                        if !body(&piece, total) {
                            return Err(Fail::Stopped);
                        }
                    }
                    Heard::End(None) => break,
                    Heard::End(Some(e)) => return Err(Fail::Offline(e)),
                    Heard::Head { .. } => return Err(Fail::Offline("a second answer".into())),
                }
            }
            if total.is_some_and(|t| t != got) {
                return Err(Fail::Offline(format!("{got} of {} bytes", total.unwrap())));
            }
            return Ok(());
        }
        Err(Fail::Refused("too many redirects".into()))
    }

    /// As Windows' `post`: one address, checked first; a redirect comes back as its status (the delegate never follows
    /// one); the answer's own body is read and dropped.
    pub fn post(
        agent: &str,
        url: &str,
        allowed: &dyn Fn(&Url) -> bool,
        content_type: &str,
        body: &[u8],
        wait: Patience,
    ) -> Result<(u16, Vec<u8>), Fail> {
        let at = super::parse(url).ok_or_else(|| Fail::Refused(format!("not an address: {url}")))?;
        if !allowed(&at) {
            return Err(Fail::Refused(format!(
                "not allowed: {}://{}",
                if at.secure { "https" } else { "http" },
                at.host
            )));
        }
        let asked = ask(agent, &at, wait, Some((content_type, body)))?;
        let status = match hear(&asked, wait)? {
            Heard::Head { status, .. } => status,
            Heard::End(e) => return Err(Fail::Offline(e.unwrap_or_else(|| "no answer".into()))),
            Heard::Data(_) => return Err(Fail::Offline("data before an answer".into())),
        };
        let (mut answer, mut got) = (Vec::new(), 0usize);
        while got < 64 * 1024 {
            match hear(&asked, wait)? {
                Heard::Data(piece) => {
                    got += piece.len();
                    super::keep(&mut answer, &piece);
                }
                Heard::End(_) | Heard::Head { .. } => break,
            }
        }
        let status = u16::try_from(status).map_err(|_| Fail::Offline(format!("status {status}")))?;
        Ok((status, answer))
    }
}

#[cfg(target_os = "macos")]
pub use mac::{get, post};

/// Elsewhere nothing is fetched: asked anyway, it answers that it got no file, so a check says the release couldn't be
/// read ("Couldn't read the update information.") and never that it found anything.
#[cfg(not(any(windows, target_os = "macos")))]
pub fn get(
    _agent: &str,
    url: &str,
    _allowed: &dyn Fn(&Url) -> bool,
    _max: u64,
    _wait: Patience,
    _body: &mut dyn FnMut(&[u8], Option<u64>) -> bool,
) -> Result<(), Fail> {
    Err(Fail::Refused(format!("not fetched on this system yet: {url}")))
}

/// Elsewhere nothing is sent either.
#[cfg(not(any(windows, target_os = "macos")))]
pub fn post(
    _agent: &str,
    url: &str,
    _allowed: &dyn Fn(&Url) -> bool,
    _content_type: &str,
    _body: &[u8],
    _wait: Patience,
) -> Result<(u16, Vec<u8>), Fail> {
    Err(Fail::Refused(format!("not sent on this system yet: {url}")))
}

/// How much of an answer's body `post` keeps.
pub const ANSWER: usize = 4096;

#[cfg(any(windows, target_os = "macos"))]
fn keep(answer: &mut Vec<u8>, piece: &[u8]) {
    let room = ANSWER.saturating_sub(answer.len());
    answer.extend_from_slice(&piece[..piece.len().min(room)]);
}

/// The whole file in memory, for the small ones (manifest.json, SHA256SUMS.txt and its signature).
pub fn get_small(agent: &str, url: &str, allowed: &dyn Fn(&Url) -> bool, wait: Patience) -> Result<Vec<u8>, Fail> {
    let mut out = Vec::new();
    get(agent, url, allowed, 1 << 20, wait, &mut |piece, _| {
        out.extend_from_slice(piece);
        true
    })?;
    Ok(out)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn addresses() {
        let u = parse("https://github.com/Waldsatte/DotAny/releases/latest/download/manifest.json").unwrap();
        assert_eq!(
            (u.secure, u.host.as_str(), u.port, u.path.as_str()),
            (true, "github.com", 443, "/Waldsatte/DotAny/releases/latest/download/manifest.json")
        );
        let u = parse("http://127.0.0.1:8123/rel/x").unwrap();
        assert_eq!((u.secure, u.host.as_str(), u.port), (false, "127.0.0.1", 8123));
        assert_eq!(parse("https://Objects.GitHubUserContent.com").unwrap().path, "/");
        for bad in [
            "ftp://github.com/x",
            "https://user:pw@github.com/x",
            "https://[::1]/x",
            "https:///x",
            "github.com/x",
            "https://git hub.com/",
        ] {
            assert_eq!(parse(bad), None, "{bad}");
        }
    }
}
