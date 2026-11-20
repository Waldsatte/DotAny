//! The Mac's stand-in for WebView2's DevTools port, so that an automated test can drive the program. The end-to-end
//! tests that use this interface are not part of this repository.
//!
//! WKWebView has no debugging port a test can reach, tauri-driver doesn't run on the Mac, and safaridriver only drives
//! Safari. So with `DOTANY_TEST`, `DOTANY_TEST_PORT` and `DOTANY_TEST_TOKEN` all set, and only then, the program
//! itself listens on 127.0.0.1 and answers the few DevTools messages a driver sends, the same way a browser would.
//! It answers only requests that carry the token and no `Origin` header (`testgate.rs`); every path below starts with
//! `/<token>`:
//!
//! - `GET /json`: the windows, each with a WebSocket address (`/<token>/w/<label>`).
//! - `Runtime.evaluate`: the page runs the expression (WKWebView's `callAsyncJavaScript`, which waits for a promise).
//! - `Input.dispatchMouseEvent`, `Input.dispatchKeyEvent`: real AppKit events (NSEvent) given to the window and the
//!   program, as a click or a key press would come; no Accessibility permission is needed, as nothing leaves the program.
//! - `Page.captureScreenshot`: the page as WKWebView draws it (`takeSnapshot`), at the screen's scale; no Screen Recording
//!   permission is needed. The system's own parts (traffic lights, menu bar) are not in it.
//! - `Runtime.exceptionThrown`, `Runtime.consoleAPICalled` (errors): the page reports them through an event (`TEST_HOOK`);
//!   `Network.requestWillBeSent`: what the page's resource timing lists, so a test's "no request left the program" looks
//!   at real loads on the Mac too (all but the program's own ipc, which the hook itself reports through). WKWebView lists
//!   nothing loaded through the program's own schemes there (tauri://, dotfont://: no entry at all);
//!   loads from the web are what it is for. A test that wants to see a font come from
//!   dotfont:// reads the font face instead.
//! - `DotAny.close`, `DotAny.windows`: the window's red button and the window list (here with each window's place, size
//!   and sheet and the file the family has open in it); `DotAny.quit`: the menu bar's
//!   Quit (⌘Q); `DotAny.terminate`: a quit from outside (the Dock icon's Quit, logging out).
//! - `DotAny.windowButtons`: where the system's red, yellow and green buttons are in the window (no picture shows them
//!   without the Screen Recording permission); `DotAny.resize`, `DotAny.fullScreen`, `DotAny.theme`: a window's size,
//!   the green button's full screen, and the window drawn light or dark.
//! - The Mac's own conventions: `DotAny.menuBar` (the whole menu
//!   bar: titles, keys, enabled) and `DotAny.menuItem` (choose one, by its titles), answered with no window open too;
//!   `DotAny.recents` (the system's recent documents); `DotAny.probeTitleBar` (what an AppKit title bar does with a
//!   double-click under the setting as it is now); `DotAny.killPage` (the window's web process ends);
//!   `DotAny.webview` (pinch and swipe); `DotAny.represented`; `DotAny.endSheet`; `DotAny.setSize`; `DotAny.restore`.
//! - `DotAny.closeButton` (the system's red button pressed, its own `performClose:`) and `DotAny.cancelSheet` (a sheet's
//!   own Cancel pressed, as a person ends the print panel).
//! - `DotAny.printTo` (the next Print… goes to a PDF file without the print panel, `mac_print.rs`) and `DotAny.printed`
//!   (whether that print has ended, and well): what is on the paper, measured.
//!
//! Everything else is answered with an empty result, so a test that asks for it carries on.

use std::collections::HashMap;
use std::io::{BufRead, BufReader, Read, Write};
use std::net::{TcpListener, TcpStream};
use std::sync::mpsc::{Sender, channel};
use std::sync::{Arc, Mutex};

use block2::RcBlock;
use objc2::MainThreadMarker;
use objc2::runtime::AnyObject;
use objc2_app_kit::{
    NSApplication, NSBitmapImageFileType, NSBitmapImageRep, NSEvent, NSEventModifierFlags, NSEventType, NSImage,
    NSWindow, NSWindowButton, NSWindowStyleMask,
};
use objc2_foundation::{NSDictionary, NSError, NSPoint, NSString};
use objc2_web_kit::{WKContentWorld, WKWebView};
use serde_json::{Value, json};
use tauri::{AppHandle, Listener, Manager, WebviewWindow};

use crate::testgate::Gate;

/// Added to every page in a test: errors and console errors are reported to the driver.
pub const TEST_HOOK: &str = r#"(() => {
  const send = (payload) => { try { window.__TAURI_INTERNALS__.invoke('plugin:event|emit', { event: 'dotany-test', payload }); } catch (e) {} };
  addEventListener('error', (e) => send({ method: 'Runtime.exceptionThrown', params: { exceptionDetails: { text: String(e.message), exception: { description: String(e.error && e.error.stack || e.message) } } } }));
  addEventListener('unhandledrejection', (e) => send({ method: 'Runtime.exceptionThrown', params: { exceptionDetails: { text: 'Uncaught (in promise)', exception: { description: String(e.reason && e.reason.stack || e.reason) } } } }));
  const error = console.error.bind(console);
  console.error = (...args) => { send({ method: 'Runtime.consoleAPICalled', params: { type: 'error', args: args.map((a) => ({ value: String(a) })) } }); error(...args); };
  try { new PerformanceObserver((l) => { for (const e of l.getEntries()) if (!/^(ipc:|http:\/\/ipc\.localhost)/.test(e.name)) send({ method: 'Network.requestWillBeSent', params: { request: { url: e.name } } }); }).observe({ type: 'resource', buffered: true }); } catch (e) {}
})();"#;

/// Whether this run is a test the driver reaches: then the port.
pub fn port() -> Option<u16> {
    Gate::from_env().map(|g| g.port)
}

type Clients = Arc<Mutex<Vec<Sender<String>>>>;

/// Starts listening, once, as the program starts.
pub fn start(app: &AppHandle) {
    let Some(gate) = Gate::from_env() else { return };
    let Ok(listener) = TcpListener::bind(("127.0.0.1", gate.port)) else { return };
    let gate = Arc::new(gate);
    let clients: Clients = Arc::default();
    {
        // the page's errors go to every driver attached
        let clients = clients.clone();
        app.listen_any("dotany-test", move |event| {
            let text = event.payload().to_string();
            crate::lock(&clients).retain(|c| c.send(text.clone()).is_ok());
        });
    }
    let app = app.clone();
    std::thread::spawn(move || {
        for stream in listener.incoming().flatten() {
            let (app, clients, gate) = (app.clone(), clients.clone(), gate.clone());
            std::thread::spawn(move || {
                let _ = serve(&app, stream, &clients, &gate);
            });
        }
    });
}

// ---------- HTTP and WebSocket, as little of them as the driver needs ----------

fn serve(app: &AppHandle, stream: TcpStream, clients: &Clients, gate: &Gate) -> std::io::Result<()> {
    let mut reader = BufReader::new(stream.try_clone()?);
    let mut first = String::new();
    reader.read_line(&mut first)?;
    let path = first.split_whitespace().nth(1).unwrap_or("/").to_string();
    let mut headers = HashMap::new();
    loop {
        let mut line = String::new();
        if reader.read_line(&mut line)? == 0 || line.trim().is_empty() {
            break;
        }
        if let Some((k, v)) = line.split_once(':') {
            headers.insert(k.trim().to_ascii_lowercase(), v.trim().to_string());
        }
    }
    let mut out = stream;
    // a web page, or anyone without the token: nothing is answered, not even the list of windows
    let Some(path) = gate.admit(&path, &headers) else {
        return write!(out, "HTTP/1.1 403 Forbidden\r\nContent-Length: 0\r\nConnection: close\r\n\r\n");
    };
    if let Some(key) = headers.get("sec-websocket-key") {
        let label = path.trim_start_matches("/w/").to_string();
        let accept = base64(&sha1(format!("{key}258EAFA5-E914-47DA-95CA-C5AB0DC85B11").as_bytes()));
        write!(
            out,
            "HTTP/1.1 101 Switching Protocols\r\nUpgrade: websocket\r\nConnection: Upgrade\r\nSec-WebSocket-Accept: {accept}\r\n\r\n"
        )?;
        return socket(app, reader, out, &label, clients);
    }
    let body = if path.starts_with("/json") { windows_json(app, gate).to_string() } else { "{}".into() };
    write!(
        out,
        "HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",
        body.len()
    )
}

fn windows_json(app: &AppHandle, gate: &Gate) -> Value {
    let (port, prefix) = (gate.port, gate.prefix());
    let mut list: Vec<(String, WebviewWindow)> = app.webview_windows().into_iter().collect();
    // the driver attaches to the newest window unless told otherwise: oldest first, as they were made
    let state = app.state::<crate::Base>();
    let files = crate::lock(&state.files);
    list.sort_by_key(|(l, _)| (files.born(l), l.clone()));
    drop(files);
    Value::Array(
        list.into_iter()
            .map(|(label, w)| {
                json!({ "id": label, "type": "page", "title": w.title().unwrap_or_default(), "url": format!("tauri://localhost/{label}"),
                        "webSocketDebuggerUrl": format!("ws://127.0.0.1:{port}{prefix}/w/{label}") })
            })
            .collect(),
    )
}

/// One driver attached to one window: messages in, answers and events out (a thread writes, so both can).
fn socket(
    app: &AppHandle,
    mut reader: BufReader<TcpStream>,
    out: TcpStream,
    label: &str,
    clients: &Clients,
) -> std::io::Result<()> {
    let (tx, rx) = channel::<String>();
    crate::lock(&clients).push(tx.clone());
    let mut writer = out;
    std::thread::spawn(move || {
        for text in rx {
            if write_frame(&mut writer, text.as_bytes()).is_err() {
                break;
            }
        }
    });
    while let Some(text) = read_frame(&mut reader)? {
        let Ok(msg) = serde_json::from_str::<Value>(&text) else { continue };
        let id = msg["id"].clone();
        let method = msg["method"].as_str().unwrap_or("").to_string();
        let params = msg["params"].clone();
        let (app, label, tx) = (app.clone(), label.to_string(), tx.clone());
        // each message on its own thread: an evaluation that waits doesn't hold up the next
        std::thread::spawn(move || {
            let result = handle(&app, &label, &method, &params);
            let _ = tx.send(json!({ "id": id, "result": result }).to_string());
        });
    }
    Ok(())
}

fn read_frame(r: &mut impl Read) -> std::io::Result<Option<String>> {
    let mut message = Vec::new();
    loop {
        let mut head = [0u8; 2];
        if r.read_exact(&mut head).is_err() {
            return Ok(None);
        }
        let (fin, op) = (head[0] & 0x80 != 0, head[0] & 0x0f);
        let mut len = (head[1] & 0x7f) as u64;
        if len == 126 {
            let mut b = [0u8; 2];
            r.read_exact(&mut b)?;
            len = u16::from_be_bytes(b) as u64;
        } else if len == 127 {
            let mut b = [0u8; 8];
            r.read_exact(&mut b)?;
            len = u64::from_be_bytes(b);
        }
        let mut mask = [0u8; 4];
        if head[1] & 0x80 != 0 {
            r.read_exact(&mut mask)?;
        }
        let mut data = vec![0u8; len as usize];
        r.read_exact(&mut data)?;
        data.iter_mut().enumerate().for_each(|(i, b)| *b ^= mask[i % 4]);
        match op {
            0x8 => return Ok(None),
            0x9 | 0xA => continue,
            _ => message.extend(data),
        }
        if fin {
            return Ok(Some(String::from_utf8_lossy(&message).into_owned()));
        }
    }
}

fn write_frame(w: &mut impl Write, data: &[u8]) -> std::io::Result<()> {
    let mut frame = vec![0x81u8];
    match data.len() {
        n if n < 126 => frame.push(n as u8),
        n if n < 65536 => {
            frame.push(126);
            frame.extend((n as u16).to_be_bytes());
        }
        n => {
            frame.push(127);
            frame.extend((n as u64).to_be_bytes());
        }
    }
    frame.extend_from_slice(data);
    w.write_all(&frame)
}

fn sha1(data: &[u8]) -> [u8; 20] {
    let mut h: [u32; 5] = [0x67452301, 0xEFCDAB89, 0x98BADCFE, 0x10325476, 0xC3D2E1F0];
    let mut m = data.to_vec();
    let bits = (data.len() as u64) * 8;
    m.push(0x80);
    while m.len() % 64 != 56 {
        m.push(0);
    }
    m.extend(bits.to_be_bytes());
    for chunk in m.chunks(64) {
        let mut w = [0u32; 80];
        for i in 0..16 {
            w[i] = u32::from_be_bytes([chunk[i * 4], chunk[i * 4 + 1], chunk[i * 4 + 2], chunk[i * 4 + 3]]);
        }
        for i in 16..80 {
            w[i] = (w[i - 3] ^ w[i - 8] ^ w[i - 14] ^ w[i - 16]).rotate_left(1);
        }
        let [mut a, mut b, mut c, mut d, mut e] = h;
        for (i, wi) in w.iter().enumerate() {
            let (f, k) = match i {
                0..=19 => ((b & c) | (!b & d), 0x5A827999),
                20..=39 => (b ^ c ^ d, 0x6ED9EBA1),
                40..=59 => ((b & c) | (b & d) | (c & d), 0x8F1BBCDC),
                _ => (b ^ c ^ d, 0xCA62C1D6),
            };
            let t = a.rotate_left(5).wrapping_add(f).wrapping_add(e).wrapping_add(k).wrapping_add(*wi);
            e = d;
            d = c;
            c = b.rotate_left(30);
            b = a;
            a = t;
        }
        for (x, y) in h.iter_mut().zip([a, b, c, d, e]) {
            *x = x.wrapping_add(y);
        }
    }
    let mut out = [0u8; 20];
    for (i, x) in h.iter().enumerate() {
        out[i * 4..i * 4 + 4].copy_from_slice(&x.to_be_bytes());
    }
    out
}

fn base64(data: &[u8]) -> String {
    const A: &[u8] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";
    let mut s = String::new();
    for c in data.chunks(3) {
        let n = (c[0] as u32) << 16 | (*c.get(1).unwrap_or(&0) as u32) << 8 | *c.get(2).unwrap_or(&0) as u32;
        for i in 0..4 {
            if i <= c.len() {
                s.push(A[(n >> (18 - 6 * i) & 63) as usize] as char);
            } else {
                s.push('=');
            }
        }
    }
    s
}

// ---------- the messages ----------

fn handle(app: &AppHandle, label: &str, method: &str, params: &Value) -> Value {
    // what is about the program, not one window: answered even after the driver's own window has gone (the File menu
    // with no window open)
    if let Some(answer) = program(app, method, params) {
        return answer;
    }
    let Some(window) = app.get_webview_window(label) else { return json!({}) };
    match method {
        "Runtime.evaluate" => evaluate(&window, params["expression"].as_str().unwrap_or("undefined")),
        "Page.captureScreenshot" => match screenshot(&window) {
            Some(png) => json!({ "data": base64(&png) }),
            None => json!({}),
        },
        // (the answer says, for a press, whether the program and the window were in front before it and whether it acted)
        "Input.dispatchMouseEvent" => on_main(&window, {
            let p = params.clone();
            move |w, mtm| mouse(w, mtm, &p)
        })
        .unwrap_or_else(|| json!({})),
        // the window's number for the system (`screencapture -l`)
        "DotAny.windowNumber" => json!({ "number": on_main(&window, |w, _| w.windowNumber()) }),
        // the window as AppKit has it: in the system's own full screen (a space of its own), and whether a sheet (the
        // print panel, a save dialog) hangs from it
        "DotAny.frame" => {
            let facts = on_main(&window, |w, _| {
                (w.styleMask().contains(objc2_app_kit::NSWindowStyleMask::FullScreen), w.attachedSheet().is_some())
            });
            json!({ "nativeFull": facts.map(|f| f.0), "sheet": facts.map(|f| f.1) })
        }
        // the titles the system's Window menu lists, the program's open windows among them
        "DotAny.windowsMenu" => {
            let found = on_main(&window, |_, mtm| {
                let app = NSApplication::sharedApplication(mtm);
                let titles =
                    |m: &objc2_app_kit::NSMenu| m.itemArray().iter().map(|i| i.title().to_string()).collect::<Vec<_>>();
                let windows = app.windowsMenu().map(|m| titles(&m));
                // the menu bar's own menus, to say where the Window menu went when the system has none set
                let bar = app.mainMenu().map(|m| titles(&m)).unwrap_or_default();
                json!({ "titles": windows.clone().unwrap_or_default(), "set": windows.is_some(), "bar": bar })
            });
            found.unwrap_or_else(|| json!({ "titles": [] }))
        }
        "Input.dispatchKeyEvent" | "Input.insertText" => {
            let p = params.clone();
            let insert = method == "Input.insertText";
            on_main(&window, move |w, mtm| key(w, mtm, &p, insert));
            json!({})
        }
        // the system's red, yellow and green buttons: each one's frame in the window, in points from the window's top-left
        // (as the page counts), whether it shows, and the window's size; the page's picture never has them
        "DotAny.windowButtons" => on_main(&window, |w, _| window_buttons(w)).unwrap_or_else(|| json!({})),
        // the buttons sampled through something AppKit lays the title bar out for (`action`: zoom, live resize, an
        // animated resize, a new title, minimise and back, full screen and back): on every display frame and at every
        // step of the window's resize, how far they were from their place
        "DotAny.sampleButtons" => sample_buttons(&window, params["action"].as_str().unwrap_or("zoom")),
        // the window's size in points, as dragging its corner would make it
        "DotAny.resize" => {
            let size =
                tauri::LogicalSize::new(params["w"].as_f64().unwrap_or(900.0), params["h"].as_f64().unwrap_or(600.0));
            let _ = window.set_size(size);
            json!({})
        }
        // the window drawn light or dark (`dark`: true, false, or null to follow the system again)
        "DotAny.theme" => {
            let theme = params["dark"].as_bool().map(|d| if d { tauri::Theme::Dark } else { tauri::Theme::Light });
            let _ = window.set_theme(theme);
            json!({})
        }
        // into or out of the system's full screen, as the green button does
        "DotAny.fullScreen" => {
            let on = params["on"].as_bool().unwrap_or(true);
            on_main(&window, move |w, _| {
                if w.styleMask().contains(NSWindowStyleMask::FullScreen) != on {
                    w.toggleFullScreen(None);
                }
            });
            json!({})
        }
        "DotAny.close" => {
            let _ = window.close();
            json!({})
        }
        // the system's own red button pressed, as a click on it ends (its action, `performClose:`), not Tauri's close
        "DotAny.closeButton" => {
            let found = on_main(&window, |w, _| {
                let b = w.standardWindowButton(NSWindowButton::CloseButton)?;
                // SAFETY: on the main thread, with no sender, as AppKit's own click on the button sends it
                unsafe { b.performClick(None) };
                Some(())
            });
            json!({ "pressed": found.flatten().is_some() })
        }
        // Quit, as the menu bar's ⌘Q does it (`platform::quit`)
        "DotAny.quit" => {
            let app = app.clone();
            let _ = window.run_on_main_thread(move || crate::platform::quit(&app));
            json!({})
        }
        // a quit from outside the program, as the Dock icon's Quit or logging out asks for it: AppKit's own terminate
        // (from the run loop, as the system's quit event arrives: not from inside one of Tauri's own callbacks, which
        // would hold every other event of the program while AppKit waits for its answer)
        "DotAny.terminate" => {
            let _ = window.run_on_main_thread(|| {
                if let Some(mtm) = MainThreadMarker::new() {
                    let app = NSApplication::sharedApplication(mtm);
                    unsafe {
                        let _: () = objc2::msg_send![&*app, performSelector: objc2::sel!(terminate:), withObject: std::ptr::null::<AnyObject>(), afterDelay: 0.0f64];
                    }
                }
            });
            json!({})
        }
        // the window's page ends, as when WebKit's web process for it crashes (its process is killed): the window should
        // come back by itself with its file
        "DotAny.killPage" => {
            let pid = on_webkit(&window, |web| unsafe { objc2::msg_send![web, _webProcessIdentifier] }).unwrap_or(0i32);
            if pid > 0 {
                unsafe { libc::kill(pid, libc::SIGKILL) };
            }
            json!({ "pid": pid })
        }
        // what the window's WKWebView allows: page-wide pinch magnification and swipe navigation (both off)
        "DotAny.webview" => on_webkit(&window, |web| unsafe {
            json!({ "magnification": web.allowsMagnification(), "swipe": web.allowsBackForwardNavigationGestures(), "scale": web.magnification() })
        })
        .unwrap_or_else(|| json!({})),
        // the file the window stands for, for the system (`representedURL`)
        "DotAny.represented" => {
            let path = on_main(&window, |w, _| w.representedURL().and_then(|u| u.path()).map(|p| p.to_string()));
            json!({ "path": path.flatten() })
        }
        // the window made this size (its content, in points), its top-left corner where it is, as a person drags its corner
        "DotAny.setSize" => {
            let (w, h) = (params["w"].as_f64().unwrap_or(800.0), params["h"].as_f64().unwrap_or(600.0));
            on_main(&window, move |ns, _| {
                let frame = ns.frame();
                let content = ns.contentRectForFrameRect(frame);
                let (dw, dh) = (frame.size.width - content.size.width, frame.size.height - content.size.height);
                let top = frame.origin.y + frame.size.height;
                let size = objc2_foundation::NSSize::new(w + dw, h + dh);
                let at = NSPoint::new(frame.origin.x, top - size.height);
                ns.setFrame_display(objc2_foundation::NSRect::new(at, size), true);
            });
            json!({})
        }
        // the window back from the Dock, and unzoomed
        "DotAny.restore" => {
            on_main(&window, |ns, _| {
                if ns.isMiniaturized() {
                    ns.deminiaturize(None);
                }
                if ns.isZoomed() {
                    ns.zoom(None);
                }
            });
            json!({})
        }
        // the next Print… goes to this PDF without the panel (`mac_print`); `DotAny.printed` says when it has ended
        "DotAny.printTo" => {
            crate::mac_print::print_next_to(params["path"].as_str().unwrap_or_default().into());
            json!({})
        }
        "DotAny.printed" => json!({ "ended": crate::mac_print::ended() }),
        // a sheet on the window (an open or save panel) answered Cancel
        "DotAny.endSheet" => {
            on_main(&window, |w, _| {
                if let Some(sheet) = w.attachedSheet() {
                    w.endSheet(&sheet);
                }
            });
            json!({})
        }
        // the sheet's own Cancel pressed (its button with the Escape key), as a person ends it: the print panel ended
        // from outside with endSheet while it was still making its preview held the program's main thread for good
        "DotAny.cancelSheet" => {
            let how = on_main(&window, |w, _| {
                let sheet = w.attachedSheet()?;
                if let Some(b) = sheet.contentView().and_then(|v| cancel_button(&v)) {
                    // SAFETY: on the main thread, with no sender, as AppKit's own click on the button sends it
                    unsafe { b.performClick(None) };
                    return Some("button");
                }
                // no such button: ended as Cancel answers (NSModalResponseCancel)
                w.endSheet_returnCode(&sheet, 0);
                Some("ended")
            });
            json!({ "cancelled": how.flatten() })
        }
        // a trackpad's scroll, pinch, rotate or double tap, made as the system makes it (`trackpad`, at the end)
        "DotAny.trackpad" => on_main(&window, {
            let p = params.clone();
            move |w, mtm| trackpad(w, mtm, &p)
        })
        .unwrap_or_else(|| json!({})),
        _ => json!({}),
    }
}

/// What the driver asks of the program as a whole.
fn program(app: &AppHandle, method: &str, params: &Value) -> Option<Value> {
    Some(match method {
        // every window of the program, oldest first: where it is (the top-left corner, in points), the file the family
        // has open in it, and the sheet on it if any (an open or save panel)
        "DotAny.windows" => {
            let state = app.state::<crate::Base>();
            let sheets: HashMap<String, Value> = on_app(app, |mtm| {
                let app = NSApplication::sharedApplication(mtm);
                app.windows().iter().map(|w| (w.windowNumber().to_string(), sheet_of(&w))).collect()
            })
            .unwrap_or_default();
            let mut list: Vec<(u64, Value)> = app
                .webview_windows()
                .values()
                .map(|w| {
                    let scale = w.scale_factor().unwrap_or(1.0);
                    let at = w.outer_position().map(|p| (p.x as f64 / scale, p.y as f64 / scale)).unwrap_or_default();
                    let size =
                        w.outer_size().map(|s| (s.width as f64 / scale, s.height as f64 / scale)).unwrap_or_default();
                    let inner =
                        w.inner_size().map(|s| (s.width as f64 / scale, s.height as f64 / scale)).unwrap_or_default();
                    let born = crate::lock(&state.files).born(w.label());
                    let number = number_of(w);
                    let sheet = number.and_then(|n| sheets.get(&n.to_string()).cloned()).unwrap_or(Value::Null);
                    (born, json!({ "label": w.label(), "title": w.title().unwrap_or_default(), "visible": w.is_visible().unwrap_or(false), "minimized": w.is_minimized().unwrap_or(false), "full": w.is_fullscreen().unwrap_or(false), "zoomed": w.is_maximized().unwrap_or(false), "focused": w.is_focused().unwrap_or(false), "x": at.0, "y": at.1, "w": size.0, "h": size.1, "inner": [inner.0, inner.1], "file": state.current_in(w.label()), "sheet": sheet }))
                })
                .collect();
            list.sort_by_key(|(born, _)| *born);
            // the screen the windows are on, in points: all of it, and the part windows go in (below the menu bar, beside
            // the Dock)
            let monitor = app
                .webview_windows()
                .values()
                .next()
                .and_then(|w| w.current_monitor().ok().flatten())
                .or_else(|| app.primary_monitor().ok().flatten());
            let screen = monitor.map(|m| {
                let s = m.scale_factor();
                let (size, area) = (m.size(), m.work_area());
                json!({ "w": size.width as f64 / s, "h": size.height as f64 / s,
                        "work": [area.position.x as f64 / s, area.position.y as f64 / s, area.size.width as f64 / s, area.size.height as f64 / s] })
            });
            json!({ "windows": list.into_iter().map(|(_, w)| w).collect::<Vec<_>>(), "pid": std::process::id(), "screen": screen })
        }
        // the whole menu bar as the system shows it now (after AppKit has added its own items and checked which are
        // enabled): each menu's title and items, each item's title, key, modifier keys, whether it is enabled, its action
        "DotAny.menuBar" => on_app(app, |mtm| {
            let app = NSApplication::sharedApplication(mtm);
            app.mainMenu().map(|m| menu_json(&m)).unwrap_or(Value::Null)
        })
        .unwrap_or(Value::Null),
        // chooses an item of the menu bar, by the titles from the bar down (["File", "Open…"], or ["文件", "打开…"]
        // with the menu in Chinese), as a click on it does
        "DotAny.menuItem" => {
            let titles: Vec<String> = params["titles"]
                .as_array()
                .map(|a| a.iter().filter_map(|t| t.as_str().map(String::from)).collect())
                .unwrap_or_default();
            on_app(app, move |mtm| {
                let app = NSApplication::sharedApplication(mtm);
                let Some(mut menu) = app.mainMenu() else { return false };
                for (i, title) in titles.iter().enumerate() {
                    menu.update();
                    let Some(at) = menu.itemArray().iter().position(|it| it.title().to_string() == *title) else {
                        return false;
                    };
                    if i + 1 == titles.len() {
                        menu.performActionForItemAtIndex(at as isize);
                        return true;
                    }
                    let Some(sub) = menu.itemAtIndex(at as isize).and_then(|it| it.submenu()) else { return false };
                    menu = sub;
                }
                false
            })
            .map(|done| json!({ "done": done }))
            .unwrap_or_else(|| json!({ "done": false }))
        }
        // the system's list of this program's recent documents (what Open Recent and the Dock icon's menu show)
        "DotAny.recents" => on_app(app, |mtm| {
            let paths: Vec<String> = objc2_app_kit::NSDocumentController::sharedDocumentController(mtm)
                .recentDocumentURLs()
                .iter()
                .filter_map(|u| u.path().map(|p| p.to_string()))
                .collect();
            json!({ "paths": paths })
        })
        .unwrap_or_else(|| json!({ "paths": [] })),
        // what a double-click on a plain AppKit title bar does with the system's setting as it is now: a titled window of
        // AppKit's own, double-clicked in its title bar (the events posted as the window server would), then looked at
        "DotAny.probeTitleBar" => probe_title_bar(app),
        _ => return None,
    })
}

/// The enabled button a sheet answers Escape with (its Cancel), anywhere inside `view`.
fn cancel_button(view: &objc2_app_kit::NSView) -> Option<objc2::rc::Retained<objc2_app_kit::NSButton>> {
    use objc2::Message;
    for sub in view.subviews().iter() {
        let any: &AnyObject = &sub;
        if let Some(b) = any.downcast_ref::<objc2_app_kit::NSButton>() {
            if b.keyEquivalent().to_string() == "\u{1b}" && b.isEnabled() {
                return Some(b.retain());
            }
        }
        if let Some(b) = cancel_button(&sub) {
            return Some(b);
        }
    }
    None
}

/// The window's number for the system.
fn number_of(w: &WebviewWindow) -> Option<isize> {
    let ptr = w.ns_window().ok()? as *const NSWindow;
    // windowNumber is safe to read from any thread
    unsafe { ptr.as_ref() }.map(|ns| ns.windowNumber())
}

/// The sheet on a window, if any: its kind (NSOpenPanel, NSSavePanel), and for an open panel what it picks (a file,
/// several, a folder).
fn sheet_of(w: &NSWindow) -> Value {
    match w.attachedSheet() {
        Some(s) => {
            let kind = s.class().name().to_string_lossy().into_owned();
            match s.downcast_ref::<objc2_app_kit::NSOpenPanel>() {
                Some(p) if p.canChooseDirectories() => json!(format!("{kind} folder")),
                Some(p) if p.allowsMultipleSelection() => json!(format!("{kind} files")),
                Some(_) => json!(format!("{kind} file")),
                None => json!(kind),
            }
        }
        None => Value::Null,
    }
}

/// A menu as the driver reads it.
fn menu_json(menu: &objc2_app_kit::NSMenu) -> Value {
    menu.update();
    let items: Vec<Value> = menu
        .itemArray()
        .iter()
        .map(|it| {
            if it.isSeparatorItem() {
                return json!({ "separator": true });
            }
            let mods = it.keyEquivalentModifierMask();
            let mut keys = String::new();
            for (flag, sign) in [
                (NSEventModifierFlags::Control, '⌃'),
                (NSEventModifierFlags::Option, '⌥'),
                (NSEventModifierFlags::Shift, '⇧'),
                (NSEventModifierFlags::Command, '⌘'),
            ] {
                if mods.contains(flag) && !it.keyEquivalent().to_string().is_empty() {
                    keys.push(sign);
                }
            }
            let key = it.keyEquivalent().to_string();
            json!({
                "title": it.title().to_string(),
                "key": if key.is_empty() { String::new() } else { format!("{keys}{}", key.to_uppercase()) },
                "enabled": it.isEnabled(),
                "hidden": it.isHidden(),
                "action": it.action().map(|a| a.name().to_string_lossy().into_owned()),
                "submenu": it.submenu().map(|s| menu_json(&s)),
            })
        })
        .collect();
    json!({ "title": menu.title().to_string(), "items": items })
}

/// Runs `f` on the main thread, and waits for it.
fn on_app<T: Send + 'static>(app: &AppHandle, f: impl FnOnce(MainThreadMarker) -> T + Send + 'static) -> Option<T> {
    let (tx, rx) = channel();
    app.run_on_main_thread(move || {
        let _ = tx.send(f(MainThreadMarker::new().expect("the main thread")));
    })
    .ok()?;
    rx.recv_timeout(std::time::Duration::from_secs(20)).ok()
}

/// Runs `f` with the window's WKWebView on the main thread, and waits for it.
fn on_webkit<T: Send + 'static>(window: &WebviewWindow, f: impl FnOnce(&WKWebView) -> T + Send + 'static) -> Option<T> {
    let (tx, rx) = channel();
    with_webkit(window, move |web| {
        let _ = tx.send(f(web));
    });
    rx.recv_timeout(std::time::Duration::from_secs(20)).ok()
}

thread_local! {
    /// The probe's own window, on the main thread, while it is being looked at.
    static PROBE: std::cell::RefCell<Option<objc2::rc::Retained<NSWindow>>> = const { std::cell::RefCell::new(None) };
}

fn probe_title_bar(app: &AppHandle) -> Value {
    use objc2_app_kit::{NSBackingStoreType, NSWindowStyleMask};
    use objc2_foundation::{NSRect, NSSize};
    let made = on_app(app, |mtm| {
        let style = NSWindowStyleMask::Titled
            | NSWindowStyleMask::Closable
            | NSWindowStyleMask::Miniaturizable
            | NSWindowStyleMask::Resizable;
        let w = unsafe {
            NSWindow::initWithContentRect_styleMask_backing_defer(
                mtm.alloc(),
                NSRect::new(NSPoint::new(160.0, 160.0), NSSize::new(420.0, 260.0)),
                style,
                NSBackingStoreType::Buffered,
                false,
            )
        };
        unsafe { w.setReleasedWhenClosed(false) };
        w.makeKeyAndOrderFront(None);
        let before = w.frame();
        PROBE.with(|p| *p.borrow_mut() = Some(w.clone()));
        // a point in its title bar, in the window's own coordinates (from the bottom)
        let at = NSPoint::new(before.size.width / 2.0, before.size.height - 10.0);
        let app = NSApplication::sharedApplication(mtm);
        let event = |kind: NSEventType, clicks: isize| {
            NSEvent::mouseEventWithType_location_modifierFlags_timestamp_windowNumber_context_eventNumber_clickCount_pressure(
                kind, at, NSEventModifierFlags::empty(), 0.0, w.windowNumber(), None, 0, clicks,
                if kind == NSEventType::LeftMouseDown { 1.0 } else { 0.0 },
            )
        };
        // the second press of a double-click, given to the window as the system gives it; its release waits in the
        // queue, for a title bar that follows the mouse until it comes up
        if let Some(up) = event(NSEventType::LeftMouseUp, 2) {
            app.postEvent_atStart(&up, true);
        }
        if let Some(down) = event(NSEventType::LeftMouseDown, 2) {
            w.sendEvent(&down);
        }
        [before.origin.x, before.origin.y, before.size.width, before.size.height]
    });
    let Some([bx, by, bw, bh]) = made else { return json!({}) };
    let before = NSRect::new(NSPoint::new(bx, by), NSSize::new(bw, bh));
    // what happened, once the events have gone through and any animation has ended
    std::thread::sleep(std::time::Duration::from_millis(1500));
    on_app(app, move |_| {
        let Some(w) = PROBE.with(|p| p.borrow_mut().take()) else { return json!({}) };
        let after = w.frame();
        let seen = json!({
            "zoomed": w.isZoomed(),
            "minimized": w.isMiniaturized(),
            "moved": before.origin.x != after.origin.x || before.origin.y != after.origin.y,
            "resized": before.size.width != after.size.width || before.size.height != after.size.height,
            "before": [before.origin.x, before.origin.y, before.size.width, before.size.height],
            "after": [after.origin.x, after.origin.y, after.size.width, after.size.height],
        });
        w.close();
        seen
    })
    .unwrap_or_else(|| json!({}))
}

thread_local! {
    /// What `sample_buttons` saw: (how, deviation), and what it set up to see it.
    static SAMPLES: std::cell::RefCell<Vec<(&'static str, f64)>> = const { std::cell::RefCell::new(Vec::new()) };
    static TIMERS: std::cell::RefCell<Vec<objc2::rc::Retained<objc2_foundation::NSTimer>>> = const { std::cell::RefCell::new(Vec::new()) };
    static WATCHES: std::cell::RefCell<Vec<objc2::rc::Retained<objc2::runtime::ProtocolObject<dyn objc2_foundation::NSObjectProtocol>>>> = const { std::cell::RefCell::new(Vec::new()) };
}

fn sample_buttons(window: &WebviewWindow, action: &str) -> Value {
    use objc2_app_kit::{NSWindowDidResizeNotification, NSWindowStyleMask};
    use objc2_foundation::{NSNotification, NSNotificationCenter, NSRunLoop, NSRunLoopCommonModes, NSTimer};
    let record = |how: &'static str, w: &NSWindow| {
        if let Some(d) = crate::platform::buttons::deviation(w) {
            SAMPLES.with(|s| s.borrow_mut().push((how, d)));
        }
    };
    // set up: every display frame (a timer at 240 per second in every run loop mode, AppKit's animation loops too) and
    // every step of the window's resize
    let started = on_main(window, move |w, _| {
        SAMPLES.with(|s| s.borrow_mut().clear());
        let me = objc2::Message::retain(w);
        let tick = {
            let me = me.clone();
            block2::RcBlock::new(move |_: std::ptr::NonNull<NSTimer>| record("frame", &me))
        };
        let timer = unsafe { NSTimer::timerWithTimeInterval_repeats_block(1.0 / 240.0, true, &tick) };
        unsafe { NSRunLoop::mainRunLoop().addTimer_forMode(&timer, NSRunLoopCommonModes) };
        let step = {
            let me = me.clone();
            block2::RcBlock::new(move |_: std::ptr::NonNull<NSNotification>| record("resize", &me))
        };
        let token = unsafe {
            NSNotificationCenter::defaultCenter().addObserverForName_object_queue_usingBlock(
                Some(NSWindowDidResizeNotification),
                Some(&me),
                None,
                &step,
            )
        };
        TIMERS.with(|t| t.borrow_mut().push(timer));
        WATCHES.with(|t| t.borrow_mut().push(token));
        w.frame()
    });
    let Some(start) = started else { return json!({ "error": "no window" }) };
    let wait = |ms| std::thread::sleep(std::time::Duration::from_millis(ms));
    let act = |f: Box<dyn FnOnce(&NSWindow, MainThreadMarker) + Send>| {
        on_main(window, f);
    };
    match action {
        "zoom" => {
            act(Box::new(|w, _| w.zoom(None)));
            wait(1500);
            act(Box::new(|w, _| w.zoom(None)));
            wait(1500);
        }
        // the window's corner dragged, as a hand does: AppKit's own live resize, with the drag waiting in the queue
        "resize" => {
            act(Box::new(move |w, mtm| {
                let app = NSApplication::sharedApplication(mtm);
                let size = w.frame().size;
                let corner = NSPoint::new(size.width - 2.0, 2.0);
                let ev = |kind: NSEventType, p: NSPoint| {
                    NSEvent::mouseEventWithType_location_modifierFlags_timestamp_windowNumber_context_eventNumber_clickCount_pressure(
                        kind, p, NSEventModifierFlags::empty(), 0.0, w.windowNumber(), None, 0, 1,
                        if kind == NSEventType::LeftMouseUp { 0.0 } else { 1.0 })
                };
                for i in 1..=40 {
                    let d = i as f64 * 4.0;
                    if let Some(e) = ev(NSEventType::LeftMouseDragged, NSPoint::new(corner.x + d, corner.y - d)) {
                        app.postEvent_atStart(&e, false);
                    }
                }
                if let Some(e) = ev(NSEventType::LeftMouseUp, NSPoint::new(corner.x + 160.0, corner.y - 160.0)) {
                    app.postEvent_atStart(&e, false);
                }
                if let Some(e) = ev(NSEventType::LeftMouseDown, corner) {
                    w.sendEvent(&e);
                }
            }));
            wait(1500);
            act(Box::new(move |w, _| w.setFrame_display_animate(start, true, false)));
            wait(600);
        }
        "animate" => {
            act(Box::new(move |w, _| {
                let mut r = start;
                r.size.width -= 240.0;
                r.size.height -= 160.0;
                w.setFrame_display_animate(r, true, true);
            }));
            wait(1200);
            act(Box::new(move |w, _| w.setFrame_display_animate(start, true, true)));
            wait(1200);
        }
        "title" => {
            act(Box::new(|w, _| {
                let t = w.title();
                w.setTitle(&NSString::from_str("another title"));
                w.setTitle(&t);
            }));
            wait(800);
        }
        "minimise" => {
            act(Box::new(|w, _| w.miniaturize(None)));
            wait(1500);
            act(Box::new(|w, _| w.deminiaturize(None)));
            wait(1500);
        }
        "full" => {
            act(Box::new(|w, _| w.toggleFullScreen(None)));
            wait(2500);
            act(Box::new(|w, _| {
                if w.styleMask().contains(NSWindowStyleMask::FullScreen) {
                    w.toggleFullScreen(None)
                }
            }));
            wait(2500);
        }
        _ => {}
    }
    on_main(window, |_, _| {
        TIMERS.with(|t| t.borrow_mut().drain(..).for_each(|t| t.invalidate()));
        WATCHES.with(|t| {
            for o in t.borrow_mut().drain(..) {
                unsafe { NSNotificationCenter::defaultCenter().removeObserver(o.as_ref()) };
            }
        });
        SAMPLES.with(|s| {
            let s = s.borrow();
            let worst = |how| s.iter().filter(|(h, _)| *h == how).map(|(_, d)| *d).fold(0f64, f64::max);
            let count = |how| s.iter().filter(|(h, _)| *h == how).count();
            // a display frame is what a person sees; a resize step is AppKit's notice of the new size, sent before it
            // has laid the title bar out for it (a container not yet moved shows there, never on screen)
            json!({ "frames": count("frame"), "max": worst("frame"),
                    "resizeSteps": count("resize"), "maxAtResizeStep": worst("resize") })
        })
    })
    .unwrap_or_else(|| json!({ "error": "no answer" }))
}

/// Runs `f` on the main thread with the window's NSWindow, and waits for it.
fn on_main<T: Send + 'static>(
    window: &WebviewWindow,
    f: impl FnOnce(&NSWindow, MainThreadMarker) -> T + Send + 'static,
) -> Option<T> {
    let (tx, rx) = channel();
    let w = window.clone();
    window
        .run_on_main_thread(move || {
            let mtm = MainThreadMarker::new().expect("the main thread");
            let ns = w.ns_window().ok().and_then(|p| unsafe { (p as *const NSWindow).as_ref() });
            let _ = tx.send(ns.map(|ns| f(ns, mtm)));
        })
        .ok()?;
    rx.recv_timeout(std::time::Duration::from_secs(20)).ok().flatten()
}

/// The three window buttons as the system has placed them. AppKit counts from the window's bottom-left; the answer
/// counts from its top-left, as the page does, so a button's frame and the page's top bar can be compared directly.
fn window_buttons(w: &NSWindow) -> Value {
    let size = w.frame().size;
    let buttons: Vec<Value> = [
        ("close", NSWindowButton::CloseButton),
        ("minimize", NSWindowButton::MiniaturizeButton),
        ("zoom", NSWindowButton::ZoomButton),
    ]
    .into_iter()
    .filter_map(|(name, kind)| {
        let b = w.standardWindowButton(kind)?;
        // in the window's own coordinates (nil: the window), wherever AppKit has nested the button
        let r = b.convertRect_toView(b.bounds(), None);
        // the views it is inside, nearest first: their kind, where they are, and which one (by address)
        let mut holders = Vec::new();
        let mut up = unsafe { b.superview() };
        while let Some(v) = up {
            if holders.len() == 3 {
                break;
            }
            let f = v.frame();
            holders.push(format!(
                "{} {:p} ({}, {}, {}, {})",
                v.class().name().to_string_lossy(),
                objc2::rc::Retained::as_ptr(&v),
                f.origin.x,
                f.origin.y,
                f.size.width,
                f.size.height
            ));
            up = unsafe { v.superview() };
        }
        Some(json!({ "name": name, "x": r.origin.x, "y": size.height - r.origin.y - r.size.height,
                     "w": r.size.width, "h": r.size.height, "hidden": b.isHiddenOrHasHiddenAncestor(), "alpha": b.alphaValue(), "in": holders }))
    })
    .collect();
    json!({ "buttons": buttons, "window": { "w": size.width, "h": size.height },
            "full": w.styleMask().contains(NSWindowStyleMask::FullScreen), "edited": w.isDocumentEdited(),
            "moves": crate::platform::buttons::MOVES.load(std::sync::atomic::Ordering::Relaxed),
            "systemSpacing": crate::platform::buttons::SPACING.get(),
            "title": w.title().to_string() })
}

/// The WKWebView of the window, on the main thread.
fn with_webkit(window: &WebviewWindow, f: impl FnOnce(&WKWebView) + Send + 'static) {
    let _ = window.with_webview(move |wv| {
        if let Some(web) = unsafe { (wv.inner() as *const WKWebView).as_ref() } {
            f(web);
        }
    });
}

fn evaluate(window: &WebviewWindow, expression: &str) -> Value {
    // the value comes back as JSON text, so any value the page has crosses unchanged (and `json` keeps it as that text:
    // `value` is parsed here, which puts an object's keys in alphabetical order)
    // DevTools takes statements too (`window.x = 1; 0`), which can't stand in brackets: they run as they are, with no
    // value back (Image's e2e hands a large picture over that way). Known by their `;` before they are run: a body that
    // doesn't parse is also reported to the page's own error event (as "Script error."), which the test then counts
    // among the page's errors
    let statements = is_statements(expression);
    let mut answer = if statements {
        run_script(window, format!("{expression}\n; return 'null';"))
    } else {
        run_script(
            window,
            format!("const v = await ({expression}\n); return JSON.stringify(v === undefined ? null : v);"),
        )
    };
    // what the look at the `;` missed (statements without one)
    if !statements && matches!(&answer, Ok(Err(m)) if m.contains("SyntaxError")) {
        answer = run_script(window, format!("{expression}\n; return 'null';"));
    }
    match answer {
        Ok(Ok(text)) => {
            json!({ "result": { "type": "object", "value": serde_json::from_str::<Value>(&text).unwrap_or(Value::Null), "json": text } })
        }
        Ok(Err(message)) => {
            json!({ "result": { "type": "undefined" }, "exceptionDetails": { "text": message.clone(), "exception": { "description": message } } })
        }
        Err(_) => {
            json!({ "result": { "type": "undefined" }, "exceptionDetails": { "text": "no answer from the page" } })
        }
    }
}

/// Whether a DevTools expression is really a list of statements: a `;` outside every string, comment and bracket.
fn is_statements(code: &str) -> bool {
    let b = code.as_bytes();
    let (mut depth, mut i) = (0i32, 0usize);
    while i < b.len() {
        match b[i] {
            q @ (b'\'' | b'"' | b'`') => {
                i += 1;
                while i < b.len() && b[i] != q {
                    i += if b[i] == b'\\' { 2 } else { 1 };
                }
            }
            b'/' if b.get(i + 1) == Some(&b'/') => {
                while i < b.len() && b[i] != b'\n' {
                    i += 1;
                }
            }
            b'/' if b.get(i + 1) == Some(&b'*') => {
                i += 2;
                while i + 1 < b.len() && !(b[i] == b'*' && b[i + 1] == b'/') {
                    i += 1;
                }
                i += 1;
            }
            b'(' | b'[' | b'{' => depth += 1,
            b')' | b']' | b'}' => depth -= 1,
            b';' if depth == 0 => return true,
            _ => {}
        }
        i += 1;
    }
    false
}

/// A function body run in the page (WKWebView's `callAsyncJavaScript`): its returned text, or the script's error.
fn run_script(
    window: &WebviewWindow,
    body: String,
) -> Result<Result<String, String>, std::sync::mpsc::RecvTimeoutError> {
    let (tx, rx) = channel::<Result<String, String>>();
    with_webkit(window, move |web| {
        let tx = Mutex::new(Some(tx));
        let done = RcBlock::new(move |value: *mut AnyObject, error: *mut NSError| {
            let Some(tx) = crate::lock(&tx).take() else { return };
            if let Some(e) = unsafe { error.as_ref() } {
                // WebKit puts the script's own message in the error's details
                let detail = e
                    .userInfo()
                    .objectForKey(&NSString::from_str("WKJavaScriptExceptionMessage"))
                    .and_then(|m| m.downcast::<NSString>().ok())
                    .map(|m| m.to_string());
                let _ = tx.send(Err(detail.unwrap_or_else(|| e.localizedDescription().to_string())));
                return;
            }
            let text =
                unsafe { (value as *const NSString).as_ref() }.map(|s| s.to_string()).unwrap_or_else(|| "null".into());
            let _ = tx.send(Ok(text));
        });
        unsafe {
            web.callAsyncJavaScript_arguments_inFrame_inContentWorld_completionHandler(
                &NSString::from_str(&body),
                None::<&NSDictionary<NSString, AnyObject>>,
                None,
                &WKContentWorld::pageWorld(MainThreadMarker::new().expect("the main thread")),
                Some(&done),
            );
        }
    });
    rx.recv_timeout(std::time::Duration::from_secs(20))
}

fn screenshot(window: &WebviewWindow) -> Option<Vec<u8>> {
    let (tx, rx) = channel::<Option<Vec<u8>>>();
    with_webkit(window, move |web| {
        let tx = Mutex::new(Some(tx));
        let done = RcBlock::new(move |image: *mut NSImage, _error: *mut NSError| {
            let Some(tx) = crate::lock(&tx).take() else { return };
            let png = unsafe { image.as_ref() }
                .and_then(|i| i.TIFFRepresentation())
                .and_then(|tiff| NSBitmapImageRep::imageRepWithData(&tiff))
                .and_then(|rep| unsafe {
                    rep.representationUsingType_properties(NSBitmapImageFileType::PNG, &NSDictionary::new())
                })
                .map(|data| data.to_vec());
            let _ = tx.send(png);
        });
        unsafe { web.takeSnapshotWithConfiguration_completionHandler(None, &done) };
    });
    rx.recv_timeout(std::time::Duration::from_secs(20)).ok().flatten()
}

/// DevTools' modifier bits (1 Alt, 2 Ctrl, 4 Meta, 8 Shift) as AppKit's. Ctrl is the shortcut key a driver sends on
/// Windows, so here it is ⌘ as well, which keeps one driver working on both systems; the Mac's Control key is `control`.
fn flags(m: u64) -> NSEventModifierFlags {
    let mut f = NSEventModifierFlags::empty();
    if m & 1 != 0 {
        f |= NSEventModifierFlags::Option;
    }
    if m & 6 != 0 {
        f |= NSEventModifierFlags::Command;
    }
    if m & 8 != 0 {
        f |= NSEventModifierFlags::Shift;
    }
    f
}

/// Where a point of the page is in the window: AppKit counts from the bottom, in points, as the page counts CSS pixels.
fn at(w: &NSWindow, p: &Value) -> NSPoint {
    let height = w.contentView().map(|v| v.frame().size.height).unwrap_or(0.0);
    NSPoint::new(p["x"].as_f64().unwrap_or(0.0), height - p["y"].as_f64().unwrap_or(0.0))
}

/// A press that only brought its window forward (`mouse`): its release goes nowhere either.
static SWALLOWED: std::sync::atomic::AtomicBool = std::sync::atomic::AtomicBool::new(false);

fn mouse(w: &NSWindow, mtm: MainThreadMarker, p: &Value) -> Value {
    let right = p["button"].as_str() == Some("right");
    // whether the window was the one in front, taking the keys, before this press
    let mut was_key = true;
    let mut seen = json!({});
    if p["type"].as_str() == Some("mousePressed") {
        // a press goes where a real one would: the program comes to the front, and the window clicked becomes its key
        // window, as key() brings it for a key
        let app = NSApplication::sharedApplication(mtm);
        was_key = app.isActive() && w.isKeyWindow();
        seen = json!({ "active": app.isActive(), "key": w.isKeyWindow(),
                       "keyWindow": app.keyWindow().map(|k| k.title().to_string()) });
        #[allow(deprecated)]
        app.activateIgnoringOtherApps(true);
        w.makeKeyAndOrderFront(None);
    } else if p["type"].as_str() == Some("mouseReleased") && SWALLOWED.swap(false, std::sync::atomic::Ordering::Relaxed)
    {
        return json!({ "acted": false });
    }
    if p["type"].as_str() == Some("mouseMoved") {
        // whether the window was in front: only that one's page hears the pointer (`mouse_event`)
        seen = json!({ "key": w.isKeyWindow(), "active": NSApplication::sharedApplication(mtm).isActive() });
    }
    let r = mouse_event(w, p, right, was_key);
    if let (Some(o), true) = (seen.as_object_mut(), p["type"].as_str() == Some("mousePressed")) {
        o.insert("acted".into(), json!(r));
    }
    seen
}

/// The event itself: sent to the window, unless it is a press into a window that was not in front and the view under it
/// takes no first click. Whether it was sent.
fn mouse_event(w: &NSWindow, p: &Value, right: bool, was_key: bool) -> bool {
    let kind = match p["type"].as_str().unwrap_or("") {
        "mousePressed" if right => NSEventType::RightMouseDown,
        "mousePressed" => NSEventType::LeftMouseDown,
        "mouseReleased" if right => NSEventType::RightMouseUp,
        "mouseReleased" => NSEventType::LeftMouseUp,
        "mouseMoved" if p["buttons"].as_u64().unwrap_or(0) & 1 != 0 => NSEventType::LeftMouseDragged,
        "mouseMoved" => NSEventType::MouseMoved,
        _ => return false,
    };
    w.setAcceptsMouseMovedEvents(true);
    let clicks = p["clickCount"].as_i64().unwrap_or(if kind == NSEventType::MouseMoved { 0 } else { 1 }) as isize;
    // a move carries the time a real one would (seconds since the Mac started, as NSEvent counts)
    let now = {
        let mut ts = libc::timespec { tv_sec: 0, tv_nsec: 0 };
        // SAFETY: a plain clock read into a local
        unsafe { libc::clock_gettime(libc::CLOCK_UPTIME_RAW, &mut ts) };
        ts.tv_sec as f64 + ts.tv_nsec as f64 / 1e9
    };
    let event = NSEvent::mouseEventWithType_location_modifierFlags_timestamp_windowNumber_context_eventNumber_clickCount_pressure(
        kind,
        at(w, p),
        flags(p["modifiers"].as_u64().unwrap_or(0)) | control(p),
        if kind == NSEventType::MouseMoved { now } else { 0.0 },
        w.windowNumber(),
        None,
        0,
        clicks,
        if matches!(kind, NSEventType::LeftMouseDown | NSEventType::RightMouseDown | NSEventType::LeftMouseDragged) { 1.0 } else { 0.0 },
    );
    let Some(e) = event else { return false };
    if !was_key {
        // a press in a window that wasn't the one in front: as AppKit does it for a real click, the view under it says
        // whether the click also acts or only brings the window forward (`acceptsFirstMouse`). Sending it to the window
        // as if it had been in front made the answer depend on how soon the program came to the front: a click acted
        // when it did at once and was lost when it didn't
        let view = w.contentView().and_then(|v| v.hitTest(at(w, p)));
        if !view.is_some_and(|v| v.acceptsFirstMouse(Some(&e))) {
            SWALLOWED.store(true, std::sync::atomic::Ordering::Relaxed);
            return false;
        }
    }
    if kind == NSEventType::MouseMoved {
        // The pointer's moves reach a WebView through its own tracking area, which AppKit feeds from the window server's
        // record of the real pointer, never from an event handed to the window: such a move reached no page (whether given
        // to the window, to the program's queue or to the view itself). WebKit's
        // `_simulateMouseMove:` (its own testing entry) goes on from where the tracking area hands a move over, so the
        // page hears it as it hears a hand's, and as for a hand's only in the window in front (WebKit tracks the pointer
        // in the key window only, with the Mac's usual overlay scroll bars); a move elsewhere goes to the window
        let Some(view) = w.contentView().and_then(|v| v.hitTest(at(w, p))) else { return false };
        let simulate = objc2::sel!(_simulateMouseMove:);
        // SAFETY: asking a view whether it answers a selector, then sending it with the event it takes
        if unsafe { objc2::msg_send![&*view, respondsToSelector: simulate] } {
            let _: () = unsafe { objc2::msg_send![&*view, _simulateMouseMove: &*e] };
            return true;
        }
    }
    w.sendEvent(&e);
    true
}

/// The Mac's key code for a DevTools `code`, and what the key types (AppKit's function-key characters for the rest).
fn key_code(code: &str, key: &str) -> (u16, String) {
    const LETTERS: [(char, u16); 26] = [
        ('a', 0x00),
        ('b', 0x0B),
        ('c', 0x08),
        ('d', 0x02),
        ('e', 0x0E),
        ('f', 0x03),
        ('g', 0x05),
        ('h', 0x04),
        ('i', 0x22),
        ('j', 0x26),
        ('k', 0x28),
        ('l', 0x25),
        ('m', 0x2E),
        ('n', 0x2D),
        ('o', 0x1F),
        ('p', 0x23),
        ('q', 0x0C),
        ('r', 0x0F),
        ('s', 0x01),
        ('t', 0x11),
        ('u', 0x20),
        ('v', 0x09),
        ('w', 0x0D),
        ('x', 0x07),
        ('y', 0x10),
        ('z', 0x06),
    ];
    const DIGITS: [u16; 10] = [0x1D, 0x12, 0x13, 0x14, 0x15, 0x17, 0x16, 0x1A, 0x1C, 0x19];
    let ch = |c: u32| char::from_u32(c).map(String::from).unwrap_or_default();
    if let Some(l) = code.strip_prefix("Key").and_then(|l| l.chars().next()) {
        let l = l.to_ascii_lowercase();
        let k = LETTERS.iter().find(|(c, _)| *c == l).map_or(0, |(_, k)| *k);
        return (k, if key.chars().count() == 1 { key.to_string() } else { l.to_string() });
    }
    if let Some(d) = code.strip_prefix("Digit").and_then(|d| d.parse::<usize>().ok()) {
        return (DIGITS[d.min(9)], d.to_string());
    }
    if let Some(n) = code.strip_prefix('F').and_then(|n| n.parse::<u32>().ok()) {
        const F: [u16; 12] = [0x7A, 0x78, 0x63, 0x76, 0x60, 0x61, 0x62, 0x64, 0x65, 0x6D, 0x67, 0x6F];
        return (F[(n.clamp(1, 12) - 1) as usize], ch(0xF704 + n - 1));
    }
    match if code.is_empty() { key } else { code } {
        "Enter" | "NumpadEnter" => (0x24, "\r".into()),
        "Escape" => (0x35, "\u{1b}".into()),
        "Backspace" => (0x33, "\u{7f}".into()),
        "Delete" => (0x75, ch(0xF728)),
        "Tab" => (0x30, "\t".into()),
        "Space" | " " => (0x31, " ".into()),
        "ArrowLeft" => (0x7B, ch(0xF702)),
        "ArrowRight" => (0x7C, ch(0xF703)),
        "ArrowDown" => (0x7D, ch(0xF701)),
        "ArrowUp" => (0x7E, ch(0xF700)),
        "Home" => (0x73, ch(0xF729)),
        "End" => (0x77, ch(0xF72B)),
        "PageUp" => (0x74, ch(0xF72C)),
        "PageDown" => (0x79, ch(0xF72D)),
        "Minus" => (0x1B, "-".into()),
        "Equal" => (0x18, "=".into()),
        "Comma" => (0x2B, ",".into()),
        "Period" => (0x2F, ".".into()),
        "Slash" => (0x2C, "/".into()),
        _ => (0, key.to_string()),
    }
}

fn key(w: &NSWindow, mtm: MainThreadMarker, p: &Value, insert: bool) {
    let app = NSApplication::sharedApplication(mtm);
    // keys go where a real key would: the program in front, its key window
    #[allow(deprecated)]
    app.activateIgnoringOtherApps(true);
    w.makeKeyAndOrderFront(None);
    let mods = flags(p["modifiers"].as_u64().unwrap_or(0)) | control(p);
    let send = |kind: NSEventType, code: u16, chars: &str, plain: &str| {
        let event = NSEvent::keyEventWithType_location_modifierFlags_timestamp_windowNumber_context_characters_charactersIgnoringModifiers_isARepeat_keyCode(
            kind,
            NSPoint::new(0.0, 0.0),
            mods,
            0.0,
            w.windowNumber(),
            None,
            &NSString::from_str(chars),
            &NSString::from_str(plain),
            false,
            code,
        );
        // through the program, so ⌘ keys reach the menu bar as they would (after the page has had them)
        if let Some(e) = event {
            app.sendEvent(&e);
        }
    };
    if insert {
        // typed text that isn't one key: each character as its own key press
        for c in p["text"].as_str().unwrap_or("").chars() {
            let s = c.to_string();
            send(NSEventType::KeyDown, 0, &s, &s);
            send(NSEventType::KeyUp, 0, &s, &s);
        }
        return;
    }
    let (kind, text) = match p["type"].as_str().unwrap_or("") {
        "keyDown" | "rawKeyDown" => (NSEventType::KeyDown, None),
        "keyUp" => (NSEventType::KeyUp, None),
        "char" => (NSEventType::KeyDown, p["text"].as_str().map(str::to_string)),
        _ => return,
    };
    let (code, chars) = key_code(p["code"].as_str().unwrap_or(""), p["key"].as_str().unwrap_or(""));
    match text {
        // a character typed: a key press and its release, as the system would send them
        Some(t) => {
            let code = if t.chars().count() == 1 { key_code("", &t).0 } else { 0 };
            send(NSEventType::KeyDown, code, &t, &t);
            send(NSEventType::KeyUp, code, &t, &t);
        }
        None => send(kind, code, &chars, &chars),
    }
}

/// The real Control key, for the Mac's own keys (⌃A in a field, Control-click): `"control": true` in a key or mouse
/// message. `modifiers` keeps meaning ⌘ for Ctrl, as on Windows.
fn control(p: &Value) -> NSEventModifierFlags {
    if p["control"].as_bool() == Some(true) { NSEventModifierFlags::Control } else { NSEventModifierFlags::empty() }
}

// ---------- the trackpad ----------
// A trackpad's events can't be made with NSEvent's constructors: they are made here as the window server sends them,
// as Quartz events, and AppKit turns them into NSEvents (`eventWithCGEvent:`), which go through the program as a real
// one does (`sendEvent:`, so event monitors see them too). The message says what to make, in Quartz's own field
// numbers (the test holds them): `scroll` {units 0 pixels / 1 lines, dy, dx} or a bare event of `type`, then `ints`
// and `doubles` {field: value}, at the page's point x, y. The answer is what AppKit made of it.

#[derive(Clone, Copy)]
#[repr(transparent)]
struct CgEvent(*mut std::ffi::c_void);
// SAFETY: a CGEventRef is a pointer to the opaque struct __CGEvent, as AppKit declares it
unsafe impl objc2::Encode for CgEvent {
    const ENCODING: objc2::Encoding = objc2::Encoding::Pointer(&objc2::Encoding::Struct("__CGEvent", &[]));
}
#[repr(C)]
#[derive(Clone, Copy)]
struct CgPoint {
    x: f64,
    y: f64,
}
#[repr(C)]
#[derive(Clone, Copy)]
struct CgRect {
    origin: CgPoint,
    size: CgPoint,
}
#[link(name = "CoreGraphics", kind = "framework")]
extern "C" {
    fn CGEventCreate(source: *const std::ffi::c_void) -> *mut std::ffi::c_void;
    fn CGEventCreateScrollWheelEvent2(
        source: *const std::ffi::c_void,
        units: u32,
        count: u32,
        w1: i32,
        w2: i32,
        w3: i32,
    ) -> *mut std::ffi::c_void;
    fn CGEventSetType(e: *mut std::ffi::c_void, t: u32);
    fn CGEventSetLocation(e: *mut std::ffi::c_void, p: CgPoint);
    fn CGEventSetFlags(e: *mut std::ffi::c_void, f: u64);
    fn CGEventSetTimestamp(e: *mut std::ffi::c_void, t: u64);
    fn CGEventSetIntegerValueField(e: *mut std::ffi::c_void, field: u32, v: i64);
    fn CGEventSetDoubleValueField(e: *mut std::ffi::c_void, field: u32, v: f64);
    fn CGMainDisplayID() -> u32;
    fn CGDisplayBounds(display: u32) -> CgRect;
    fn CFRelease(p: *const std::ffi::c_void);
}
extern "C" {
    // the system's (CLOCK_UPTIME_RAW is 8), not in the libc crate's list
    fn clock_gettime_nsec_np(clock: u32) -> u64;
}

fn trackpad(w: &NSWindow, mtm: MainThreadMarker, p: &Value) -> Value {
    let app = NSApplication::sharedApplication(mtm);
    #[allow(deprecated)]
    app.activateIgnoringOtherApps(true);
    w.makeKeyAndOrderFront(None);
    // the point on the screen, in Quartz's coordinates (from the main screen's top left)
    let on_screen = w.convertPointToScreen(at(w, p));
    let top = unsafe { CGDisplayBounds(CGMainDisplayID()) }.size.y;
    let num = |o: Option<&Value>| o.and_then(Value::as_f64).unwrap_or(0.0);
    let cg = unsafe {
        match p["scroll"].as_object() {
            Some(s) => CGEventCreateScrollWheelEvent2(
                std::ptr::null(),
                num(s.get("units")) as u32,
                2,
                num(s.get("dy")) as i32,
                num(s.get("dx")) as i32,
                0,
            ),
            None => CGEventCreate(std::ptr::null()),
        }
    };
    if cg.is_null() {
        return json!({ "error": "no event" });
    }
    unsafe {
        if let Some(t) = p["type"].as_u64() {
            CGEventSetType(cg, t as u32);
        }
        CGEventSetLocation(cg, CgPoint { x: on_screen.x, y: top - on_screen.y });
        // the point in the window too: the window server writes it as it routes an event, and AppKit's
        // `locationInWindow` reads it from there (not from the location on the screen)
        let set_window_location = libc::dlsym(libc::RTLD_DEFAULT, c"CGEventSetWindowLocation".as_ptr());
        if !set_window_location.is_null() {
            let f: extern "C" fn(*mut std::ffi::c_void, CgPoint) = std::mem::transmute(set_window_location);
            let height = w.frame().size.height;
            let inside = at(w, p);
            f(cg, CgPoint { x: inside.x, y: height - inside.y });
        }
        // now, as the system stamps its events (since the Mac started)
        CGEventSetTimestamp(cg, clock_gettime_nsec_np(8));
        let mods = flags(p["modifiers"].as_u64().unwrap_or(0)) | control(p);
        CGEventSetFlags(cg, mods.0 as u64);
        // the window the event is for, as the window server names it (51), and the window under the pointer (91, 92)
        for field in [51, 91, 92] {
            CGEventSetIntegerValueField(cg, field, w.windowNumber() as i64);
        }
        for (k, v) in p["ints"].as_object().into_iter().flatten() {
            if let (Ok(f), Some(v)) = (k.parse::<u32>(), v.as_i64()) {
                CGEventSetIntegerValueField(cg, f, v);
            }
        }
        for (k, v) in p["doubles"].as_object().into_iter().flatten() {
            if let (Ok(f), Some(v)) = (k.parse::<u32>(), v.as_f64()) {
                CGEventSetDoubleValueField(cg, f, v);
            }
        }
    }
    let event: Option<objc2::rc::Retained<NSEvent>> =
        unsafe { objc2::msg_send![objc2::class!(NSEvent), eventWithCGEvent: CgEvent(cg)] };
    unsafe { CFRelease(cg) };
    let Some(e) = event else { return json!({ "error": "AppKit made no event of it" }) };
    let kind = e.r#type();
    let at_window = e.locationInWindow();
    // the view AppKit finds under the event's point
    let view =
        w.contentView().and_then(|v| v.hitTest(at_window)).map(|v| v.class().name().to_string_lossy().into_owned());
    let mut made = json!({ "type": kind.0, "window": e.windowNumber(), "x": at_window.x, "y": at_window.y, "view": view, "time": e.timestamp() });
    if kind == NSEventType::ScrollWheel {
        made["phase"] = json!(e.phase().0);
        made["momentum"] = json!(e.momentumPhase().0);
        made["precise"] = json!(e.hasPreciseScrollingDeltas());
        made["dy"] = json!(e.scrollingDeltaY());
        made["dx"] = json!(e.scrollingDeltaX());
    }
    // (each asked only of its own kind: AppKit throws when a magnify is asked its rotation)
    if kind == NSEventType::Magnify {
        made["phase"] = json!(e.phase().0);
        made["magnification"] = json!(e.magnification());
    }
    if kind == NSEventType::Rotate {
        made["phase"] = json!(e.phase().0);
        made["rotation"] = json!(e.rotation());
    }
    // into the program's queue, as the window server's events come: the run loop takes it from there (a gesture can
    // start a tracking loop of AppKit's own, so nothing here waits for it)
    match p["via"].as_str() {
        Some("window") => w.sendEvent(&e),
        Some("app") => app.sendEvent(&e),
        Some("none") => {}
        _ => app.postEvent_atStart(&e, false),
    }
    made
}
