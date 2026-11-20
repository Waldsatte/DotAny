//! Films WebView2 won't play (AVI, WMV), played by Windows: Media Foundation's media engine reads the file, decodes it with the system's
//! decoders, plays the sound and keeps the pictures in step. Nothing of ours decodes; we only copy each finished picture into memory
//! the page shares, where it's drawn. Media Foundation is looked up when it's first needed, not linked, so a Windows without it
//! (the N editions) still starts this program, and such a film is refused with the usual words.
//!
//! The shared memory, as the page reads it (little-endian):
//! 0 seq u32 (pictures written so far) · 4 slot u32 (where the newest is) · 8 flags u32 (1 paused, 2 ended, 4 failed) · 12 seek u32 (the last seek done)
//! 16 time f64 (where playing is now) · 24 unused · 32 width u32 · 36 height u32 · from 64: `SLOTS` pictures, BGRA, `width * 4` bytes a row.

use std::ffi::c_void;
use std::path::Path;
use std::sync::atomic::{AtomicU32, Ordering, fence};
use std::sync::mpsc::{Receiver, RecvTimeoutError, Sender, channel};
use std::sync::{Arc, OnceLock};
use std::thread::JoinHandle;
use std::time::{Duration, Instant};
use windows::Win32::Foundation::RECT;
use windows::Win32::Graphics::Imaging::{
    CLSID_WICImagingFactory, GUID_WICPixelFormat32bppBGRA, IWICBitmap, IWICImagingFactory, WICBitmapCacheOnDemand,
};
use windows::Win32::Media::MediaFoundation::{
    CLSID_MFMediaEngineClassFactory, IMFAttributes, IMFByteStream, IMFMediaEngine, IMFMediaEngineClassFactory,
    IMFMediaEngineEx, IMFMediaEngineNotify, IMFMediaEngineNotify_Impl, MF_MEDIA_ENGINE_CALLBACK,
    MF_MEDIA_ENGINE_EVENT_ENDED, MF_MEDIA_ENGINE_EVENT_ERROR, MF_MEDIA_ENGINE_EVENT_LOADEDMETADATA,
    MF_MEDIA_ENGINE_VIDEO_OUTPUT_FORMAT, MF_VERSION,
};
use windows::Win32::System::Com::{
    CLSCTX_INPROC_SERVER, COINIT_MULTITHREADED, CoCreateInstance, CoInitializeEx, CoUninitialize,
};
use windows::core::{BSTR, HRESULT, Interface, PCWSTR, implement};
use windows_sys::Win32::System::LibraryLoader::{GetProcAddress, LOAD_LIBRARY_SEARCH_SYSTEM32, LoadLibraryExW};

pub const HEADER: usize = 64;
const SLOTS: usize = 3;
// DXGI_FORMAT_B8G8R8A8_UNORM: the order a page's VideoFrame takes as BGRA
const BGRA: u32 = 87;

#[derive(Debug, PartialEq)]
pub enum SystemError {
    /// This Windows has no Media Foundation, or no decoder for what's in the file.
    NoDecoder,
    /// The system couldn't play it, or it has no picture.
    Refused,
}

#[derive(Clone, Copy, Debug, serde::Serialize)]
pub struct SystemFacts {
    /// What the picture is shown as: the stored height, and the width its shape asks for.
    pub width: u32,
    pub height: u32,
    /// What's decoded: the stored size.
    pub frame_width: u32,
    pub frame_height: u32,
    pub duration: f64,
    pub audio: bool,
}

impl SystemFacts {
    pub fn surface_len(&self) -> usize {
        HEADER + SLOTS * self.frame_width as usize * self.frame_height as usize * 4
    }
}

type Startup = unsafe extern "system" fn(u32, u32) -> HRESULT;
type Shutdown = unsafe extern "system" fn() -> HRESULT;
type CreateFile = unsafe extern "system" fn(i32, i32, i32, PCWSTR, *mut *mut c_void) -> HRESULT;
type CreateAttributes = unsafe extern "system" fn(*mut *mut c_void, u32) -> HRESULT;

struct Mf {
    startup: Startup,
    shutdown: Shutdown,
    create_file: CreateFile,
    create_attributes: CreateAttributes,
}

fn wide(s: &str) -> Vec<u16> {
    s.encode_utf16().chain(std::iter::once(0)).collect()
}

fn symbol<T>(dll: &str, name: &str) -> Option<T> {
    unsafe {
        let module = LoadLibraryExW(wide(dll).as_ptr(), std::ptr::null_mut(), LOAD_LIBRARY_SEARCH_SYSTEM32);
        if module.is_null() {
            return None;
        }
        let name = std::ffi::CString::new(name).ok()?;
        let f = GetProcAddress(module, name.as_ptr() as *const u8)?;
        Some(std::mem::transmute_copy(&f))
    }
}

fn mf() -> Option<&'static Mf> {
    static MF: OnceLock<Option<Mf>> = OnceLock::new();
    MF.get_or_init(|| {
        Some(Mf {
            startup: symbol("mfplat.dll", "MFStartup")?,
            shutdown: symbol("mfplat.dll", "MFShutdown")?,
            create_file: symbol("mfplat.dll", "MFCreateFile")?,
            create_attributes: symbol("mfplat.dll", "MFCreateAttributes")?,
        })
    })
    .as_ref()
}

/// What the engine has told us, as bits: 1 its facts are in, 2 it failed, 4 it reached the end.
#[implement(IMFMediaEngineNotify)]
struct Notify(Arc<AtomicU32>);

impl IMFMediaEngineNotify_Impl for Notify_Impl {
    fn EventNotify(&self, event: u32, _a: usize, _b: u32) -> windows::core::Result<()> {
        let bit = match event as i32 {
            e if e == MF_MEDIA_ENGINE_EVENT_LOADEDMETADATA.0 => 1,
            e if e == MF_MEDIA_ENGINE_EVENT_ERROR.0 => 2,
            e if e == MF_MEDIA_ENGINE_EVENT_ENDED.0 => 4,
            _ => 0,
        };
        self.0.fetch_or(bit, Ordering::AcqRel);
        Ok(())
    }
}

pub enum Command {
    /// Where the pictures go. Only valid until `Close` has been answered.
    Surface(usize, usize),
    Play,
    Pause,
    Seek(f64, u32),
    Volume(f64),
    Rate(f64),
    Close,
}

/// A film the system is playing, on a thread of its own: every call to the engine is made there.
pub struct SystemFilm {
    pub facts: SystemFacts,
    tx: Sender<Command>,
    thread: Option<JoinHandle<()>>,
}

impl SystemFilm {
    pub fn open(path: &Path) -> Result<SystemFilm, SystemError> {
        if std::env::var_os("DOTANY_TEST_NO_MEDIA_FOUNDATION").is_some() {
            return Err(SystemError::NoDecoder);
        }
        let (tx, rx) = channel();
        let (answer, opened) = channel();
        let path = path.to_path_buf();
        let thread = std::thread::Builder::new()
            .name("system-film".into())
            .spawn(move || run(&path, rx, answer))
            .map_err(|_| SystemError::Refused)?;
        match opened.recv_timeout(Duration::from_secs(30)) {
            Ok(Ok(facts)) => Ok(SystemFilm { facts, tx, thread: Some(thread) }),
            Ok(Err(e)) => {
                let _ = thread.join();
                Err(e)
            }
            Err(_) => {
                let _ = tx.send(Command::Close);
                Err(SystemError::Refused)
            }
        }
    }
    pub fn send(&self, c: Command) {
        let _ = self.tx.send(c);
    }
    /// Stops it, and waits until nothing more will be written to its surface.
    pub fn close(mut self) {
        self.send(Command::Close);
        if let Some(t) = self.thread.take() {
            let _ = t.join();
        }
    }
}

impl Drop for SystemFilm {
    fn drop(&mut self) {
        let _ = self.tx.send(Command::Close);
        if let Some(t) = self.thread.take() {
            let _ = t.join();
        }
    }
}

struct Session(&'static Mf, bool);

impl Drop for Session {
    fn drop(&mut self) {
        unsafe {
            let _ = (self.0.shutdown)();
            if self.1 {
                CoUninitialize();
            }
        }
    }
}

fn engine_for(mf: &Mf, path: &Path, notify: Arc<AtomicU32>) -> Result<IMFMediaEngine, SystemError> {
    unsafe {
        let factory: IMFMediaEngineClassFactory =
            CoCreateInstance(&CLSID_MFMediaEngineClassFactory, None, CLSCTX_INPROC_SERVER)
                .map_err(|_| SystemError::NoDecoder)?;
        let mut attrs = std::ptr::null_mut();
        (mf.create_attributes)(&mut attrs, 2).ok().map_err(|_| SystemError::NoDecoder)?;
        let attrs = IMFAttributes::from_raw(attrs);
        let callback: IMFMediaEngineNotify = Notify(notify).into();
        attrs.SetUnknown(&MF_MEDIA_ENGINE_CALLBACK, &callback).map_err(|_| SystemError::NoDecoder)?;
        attrs.SetUINT32(&MF_MEDIA_ENGINE_VIDEO_OUTPUT_FORMAT, BGRA).map_err(|_| SystemError::NoDecoder)?;
        // no window and no graphics device: frame-server mode, decoded on the processor, each picture handed over on request
        let engine = factory.CreateInstance(0, &attrs).map_err(|_| SystemError::NoDecoder)?;
        // from the file itself rather than a web address, so no character in its name can be read as part of one
        let name = wide(&path.to_string_lossy());
        let mut stream = std::ptr::null_mut();
        (mf.create_file)(1, 0, 0, PCWSTR(name.as_ptr()), &mut stream).ok().map_err(|_| SystemError::Refused)?;
        let stream = IMFByteStream::from_raw(stream);
        let ex: IMFMediaEngineEx = engine.cast().map_err(|_| SystemError::NoDecoder)?;
        ex.SetSourceFromByteStream(&stream, &BSTR::from(path.to_string_lossy().as_ref()))
            .map_err(|_| SystemError::Refused)?;
        Ok(engine)
    }
}

fn run(path: &Path, rx: Receiver<Command>, answer: Sender<Result<SystemFacts, SystemError>>) {
    let Some(mf) = mf() else {
        let _ = answer.send(Err(SystemError::NoDecoder));
        return;
    };
    let com = unsafe { CoInitializeEx(None, COINIT_MULTITHREADED) }.is_ok();
    // MFSTARTUP_LITE: no network sources
    if unsafe { (mf.startup)(MF_VERSION, 1) }.is_err() {
        if com {
            unsafe { CoUninitialize() };
        }
        let _ = answer.send(Err(SystemError::NoDecoder));
        return;
    }
    let _session = Session(mf, com);
    let notify = Arc::new(AtomicU32::new(0));
    let engine = match engine_for(mf, path, notify.clone()) {
        Ok(e) => e,
        Err(e) => {
            let _ = answer.send(Err(e));
            return;
        }
    };
    // its facts, or its failure
    let start = Instant::now();
    while notify.load(Ordering::Acquire) & 3 == 0 {
        if start.elapsed() > Duration::from_secs(25) || matches!(rx.try_recv(), Ok(Command::Close)) {
            unsafe {
                let _ = engine.Shutdown();
            }
            let _ = answer.send(Err(SystemError::Refused));
            return;
        }
        std::thread::sleep(Duration::from_millis(5));
    }
    let facts = unsafe {
        let (mut w, mut h) = (0u32, 0u32);
        let picture = notify.load(Ordering::Acquire) & 2 == 0
            && engine.HasVideo().as_bool()
            && engine.GetNativeVideoSize(Some(&mut w), Some(&mut h)).is_ok()
            && w > 0
            && h > 0;
        if !picture {
            let _ = engine.Shutdown();
            let _ = answer.send(Err(SystemError::Refused));
            return;
        }
        let (mut ax, mut ay) = (0u32, 0u32);
        // the engine gives the picture's shape (16:9), so the width is the height stretched to it
        let shown = if engine.GetVideoAspectRatio(Some(&mut ax), Some(&mut ay)).is_ok() && ax > 0 && ay > 0 {
            ((h as u64 * ax as u64 + ay as u64 / 2) / ay as u64) as u32
        } else {
            w
        };
        let d = engine.GetDuration();
        SystemFacts {
            width: shown.max(1),
            height: h,
            frame_width: w,
            frame_height: h,
            duration: if d.is_finite() { d } else { 0.0 },
            audio: engine.HasAudio().as_bool(),
        }
    };
    let _ = answer.send(Ok(facts));
    play(&engine, &facts, &rx, &notify);
    unsafe {
        let _ = engine.Shutdown();
    }
}

struct Surface(*mut u8);

impl Surface {
    fn u32_at(&self, at: usize, v: u32) {
        unsafe { std::ptr::write_volatile(self.0.add(at) as *mut u32, v) }
    }
    fn f64_at(&self, at: usize, v: f64) {
        unsafe { std::ptr::write_volatile(self.0.add(at) as *mut f64, v) }
    }
}

fn play(engine: &IMFMediaEngine, facts: &SystemFacts, rx: &Receiver<Command>, notify: &AtomicU32) {
    let (w, h) = (facts.frame_width, facts.frame_height);
    let picture = w as usize * h as usize * 4;
    let Ok(bitmap): Result<IWICBitmap, _> = (unsafe {
        CoCreateInstance::<_, IWICImagingFactory>(&CLSID_WICImagingFactory, None, CLSCTX_INPROC_SERVER)
            .and_then(|f| f.CreateBitmap(w, h, &GUID_WICPixelFormat32bppBGRA, WICBitmapCacheOnDemand))
    }) else {
        return;
    };
    let rect = RECT { left: 0, top: 0, right: w as i32, bottom: h as i32 };
    let mut surface: Option<Surface> = None;
    let (mut seq, mut last_pts, mut seek_wanted, mut seek_done) = (0u32, -1i64, None::<u32>, 0u32);
    // a seek while paused: the engine has the new picture but may not say so, so it's taken anyway for a moment
    let mut fetch = 0u32;
    loop {
        let wait = match &surface {
            Some(_) if unsafe { !engine.IsPaused().as_bool() } => Duration::from_millis(4),
            _ => Duration::from_millis(15),
        };
        let command = match rx.recv_timeout(wait) {
            Ok(c) => Some(c),
            Err(RecvTimeoutError::Timeout) => None,
            Err(RecvTimeoutError::Disconnected) => return,
        };
        unsafe {
            match command {
                Some(Command::Close) => return,
                Some(Command::Surface(at, len)) if len >= HEADER + SLOTS * picture => {
                    let s = Surface(at as *mut u8);
                    s.u32_at(32, w);
                    s.u32_at(36, h);
                    surface = Some(s);
                    last_pts = -1;
                }
                Some(Command::Play) => {
                    let _ = engine.Play();
                }
                Some(Command::Pause) => {
                    let _ = engine.Pause();
                }
                Some(Command::Seek(t, id)) => {
                    let _ = engine.SetCurrentTime(t);
                    seek_wanted = Some(id);
                }
                Some(Command::Volume(v)) => {
                    let _ = engine.SetVolume(v.clamp(0.0, 1.0));
                }
                Some(Command::Rate(r)) => {
                    let _ = engine.SetDefaultPlaybackRate(r);
                    let _ = engine.SetPlaybackRate(r);
                }
                _ => {}
            }
            let Some(s) = &surface else { continue };
            if let Some(id) = seek_wanted {
                if !engine.IsSeeking().as_bool() {
                    seek_done = id;
                    seek_wanted = None;
                    fetch = 12;
                }
            }
            // a new picture, if there is one
            let tick = engine.OnVideoStreamTick().unwrap_or(-1);
            let take = (tick >= 0 && tick != last_pts) || fetch > 0;
            fetch = fetch.saturating_sub(1);
            if take {
                let pts = tick.max(0);
                if engine.TransferVideoFrame(&bitmap, None, &rect, None).is_ok() {
                    let slot = (seq as usize + 1) % SLOTS;
                    let to = std::slice::from_raw_parts_mut(s.0.add(HEADER + slot * picture), picture);
                    if bitmap.CopyPixels(std::ptr::null(), w * 4, to).is_ok() {
                        last_pts = pts;
                        s.u32_at(4, slot as u32);
                        fence(Ordering::Release);
                        seq = seq.wrapping_add(1);
                        s.u32_at(0, seq);
                    }
                }
            }
            let events = notify.load(Ordering::Acquire);
            let flags = engine.IsPaused().as_bool() as u32
                | (engine.IsEnded().as_bool() as u32) << 1
                | ((events & 2 != 0) as u32) << 2;
            s.f64_at(16, engine.GetCurrentTime());
            s.u32_at(12, seek_done);
            s.u32_at(8, flags);
        }
    }
}

// ---------- the memory the page shares ----------

use webview2_com::Microsoft::Web::WebView2::Win32::{
    COREWEBVIEW2_SHARED_BUFFER_ACCESS_READ_ONLY, ICoreWebView2_17, ICoreWebView2Environment12,
    ICoreWebView2SharedBuffer,
};

/// Memory this program writes and the page reads, made and given to the page on the window's own thread.
pub struct Shared(ICoreWebView2SharedBuffer);
// only its address is used elsewhere; the object itself is made and closed on the window's thread
unsafe impl Send for Shared {}

/// Makes `len` bytes the page receives as a `sharedbufferreceived` event whose data is `{"dotfilm": gen}`. Returns where they are.
pub fn share(window: &tauri::WebviewWindow, len: usize, gen: u64) -> Option<(Shared, usize)> {
    let (tx, rx) = channel();
    window
        .with_webview(move |w| {
            let made = (|| unsafe {
                let env: ICoreWebView2Environment12 = w.environment().cast().ok()?;
                let buffer = env.CreateSharedBuffer(len as u64).ok()?;
                let mut at = std::ptr::null_mut();
                buffer.Buffer(&mut at).ok()?;
                std::ptr::write_bytes(at, 0, HEADER);
                let core: ICoreWebView2_17 = w.controller().CoreWebView2().ok()?.cast().ok()?;
                let data = wide(&format!("{{\"dotfilm\":{gen}}}"));
                core.PostSharedBufferToScript(
                    &buffer,
                    COREWEBVIEW2_SHARED_BUFFER_ACCESS_READ_ONLY,
                    PCWSTR(data.as_ptr()),
                )
                .ok()?;
                Some((Shared(buffer), at as usize))
            })();
            let _ = tx.send(made);
        })
        .ok()?;
    rx.recv_timeout(Duration::from_secs(10)).ok().flatten()
}

/// Frees the memory. The film writing to it must be closed first.
pub fn unshare(window: &tauri::WebviewWindow, shared: Shared) {
    let _ = window.with_webview(move |_| {
        // the whole of it moves here, not just its inside
        let shared = shared;
        unsafe {
            let _ = shared.0.Close();
        }
    });
}
