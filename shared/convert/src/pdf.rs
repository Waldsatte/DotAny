//! PDF pages to pictures ("Exports each page as an individual image."). DotAny PDF draws a page with PDF.js in its
//! page; the converter has no page, so it asks Windows' own PDF renderer (`Windows.Data.Pdf`, part of every Windows 10
//! and 11), the way Video leaves its films to Windows, and on the Mac Core Graphics (`CGPDFDocument`, Preview's own): nothing new is shipped.
//! A page is drawn on white at twice its size in points, as PDF's own PNG export draws it (`pagePicture(pg, 2)`), and
//! saved as PNG. One page is `name.png`; more are `name 1.png`,
//! `name 2.png` and so on. PDF to Word is not done here: README.md says why.

use crate::out::Outputs;
use crate::{Reason, Step};
use std::path::Path;

/// What a PDF can become here.
pub fn targets() -> Vec<&'static str> {
    vec!["png"]
}

#[cfg(windows)]
mod win {
    use crate::Reason;
    use std::path::Path;
    use windows::Data::Pdf::{PdfDocument, PdfPageRenderOptions};
    use windows::Graphics::Imaging::BitmapEncoder;
    use windows::Storage::StorageFile;
    use windows::Storage::Streams::{DataReader, InMemoryRandomAccessStream};
    use windows::UI::Color;
    use windows::core::HSTRING;

    /// Each page's PNG, handed over as it is drawn, two pages ahead at most so a long document never sits in memory.
    /// The drawing runs on a thread of its own in the multithreaded apartment, so the caller's apartment doesn't matter;
    /// `each` runs on the caller's thread.
    pub fn pages(p: &Path, each: &mut dyn FnMut(u32, u32, Vec<u8>) -> Result<(), Reason>) -> Result<(), Reason> {
        let path = std::path::absolute(p).map_err(|_| Reason::Unreadable)?;
        let (tx, rx) = std::sync::mpsc::sync_channel::<(u32, u32, Vec<u8>)>(2);
        std::thread::scope(|s| {
            let drawer =
                s.spawn(move || run(&path, &mut |i, n, png| tx.send((i, n, png)).map_err(|_| Reason::Damaged)));
            let mut failed = None;
            for (i, n, png) in rx.iter() {
                if let Err(e) = each(i, n, png) {
                    failed = Some(e);
                    break;
                }
            }
            // dropping the receiver stops the drawing at its next page
            drop(rx);
            let drawn = drawer.join().unwrap_or(Err(Reason::Damaged));
            match failed {
                Some(e) => Err(e),
                None => drawn,
            }
        })
    }

    fn run(path: &Path, each: &mut dyn FnMut(u32, u32, Vec<u8>) -> Result<(), Reason>) -> Result<(), Reason> {
        use windows::Win32::System::WinRT::{RO_INIT_MULTITHREADED, RoInitialize};
        // SAFETY: this thread is ours; MTA is what blocking on WinRT's async calls needs
        let _ = unsafe { RoInitialize(RO_INIT_MULTITHREADED) };
        let file = StorageFile::GetFileFromPathAsync(&HSTRING::from(path.as_os_str()))
            .and_then(|a| a.join())
            .map_err(|_| Reason::Unreadable)?;
        let doc = match PdfDocument::LoadFromFileAsync(&file).and_then(|a| a.join()) {
            Ok(d) => d,
            // one that needs a password to open says so in its trailer
            Err(_) if std::fs::read(path).is_ok_and(|b| b.windows(8).any(|w| w == b"/Encrypt")) => {
                return Err(Reason::Locked);
            }
            Err(_) => return Err(Reason::Damaged),
        };
        let count = doc.PageCount().map_err(|_| Reason::Damaged)?;
        if count == 0 {
            return Err(Reason::Damaged);
        }
        let png = BitmapEncoder::PngEncoderId().map_err(|_| Reason::Missing("Windows.Graphics.Imaging".into()))?;
        let mut ratio = 1.0f64;
        for i in 0..count {
            let page = doc.GetPage(i).map_err(|_| Reason::Damaged)?;
            let size = page.Size().map_err(|_| Reason::Damaged)?;
            // Size is in device-independent pixels (1/96 inch); twice the size in points is 1.5 times that
            let (w, h) =
                ((size.Width as f64 * 1.5).round().max(1.0) as u32, (size.Height as f64 * 1.5).round().max(1.0) as u32);
            if w as u64 * h as u64 > 400_000_000 {
                return Err(Reason::TooLarge);
            }
            // Windows scales the size it is given by the screen's own; the first page says by how much
            let mut bytes = draw(
                &page,
                ((w as f64) / ratio).round().max(1.0) as u32,
                ((h as f64) / ratio).round().max(1.0) as u32,
                png,
            )?;
            if let Some(got) = png_width(&bytes).filter(|g| g.abs_diff(w) > 1) {
                ratio *= got as f64 / w as f64;
                bytes = draw(
                    &page,
                    ((w as f64) / ratio).round().max(1.0) as u32,
                    ((h as f64) / ratio).round().max(1.0) as u32,
                    png,
                )?;
            }
            each(i, count, bytes)?;
        }
        Ok(())
    }

    fn draw(page: &windows::Data::Pdf::PdfPage, w: u32, h: u32, png: windows::core::GUID) -> Result<Vec<u8>, Reason> {
        let opts = PdfPageRenderOptions::new().map_err(|_| Reason::Damaged)?;
        opts.SetDestinationWidth(w).map_err(|_| Reason::Damaged)?;
        opts.SetDestinationHeight(h).map_err(|_| Reason::Damaged)?;
        opts.SetBackgroundColor(Color { A: 255, R: 255, G: 255, B: 255 }).map_err(|_| Reason::Damaged)?;
        opts.SetBitmapEncoderId(png).map_err(|_| Reason::Damaged)?;
        let stream = InMemoryRandomAccessStream::new().map_err(|_| Reason::Damaged)?;
        page.RenderWithOptionsToStreamAsync(&stream, &opts).and_then(|a| a.join()).map_err(|_| Reason::Damaged)?;
        let n = stream.Size().map_err(|_| Reason::Damaged)? as u32;
        stream.Seek(0).map_err(|_| Reason::Damaged)?;
        let reader = DataReader::CreateDataReader(&stream).map_err(|_| Reason::Damaged)?;
        reader.LoadAsync(n).and_then(|a| a.join()).map_err(|_| Reason::Damaged)?;
        let mut bytes = vec![0u8; n as usize];
        reader.ReadBytes(&mut bytes).map_err(|_| Reason::Damaged)?;
        Ok(bytes)
    }

    /// The width a PNG says it has, from its header.
    fn png_width(b: &[u8]) -> Option<u32> {
        (b.len() > 24 && &b[12..16] == b"IHDR").then(|| u32::from_be_bytes(b[16..20].try_into().unwrap()))
    }

    /// Whether this Windows has its PDF renderer (Windows 10 and later; Server Core may not).
    pub fn available() -> bool {
        windows::Data::Pdf::PdfPageRenderOptions::new().is_ok()
    }

    /// Each page's size in device-independent pixels, without drawing any.
    pub fn sizes(p: &Path) -> Option<Vec<(f64, f64)>> {
        let path = std::path::absolute(p).ok()?;
        std::thread::scope(|s| {
            s.spawn(move || {
                use windows::Win32::System::WinRT::{RO_INIT_MULTITHREADED, RoInitialize};
                // SAFETY: as in `run`
                let _ = unsafe { RoInitialize(RO_INIT_MULTITHREADED) };
                let file = StorageFile::GetFileFromPathAsync(&HSTRING::from(path.as_os_str())).ok()?.join().ok()?;
                let doc = PdfDocument::LoadFromFileAsync(&file).ok()?.join().ok()?;
                (0..doc.PageCount().ok()?)
                    .map(|i| doc.GetPage(i).ok()?.Size().ok().map(|s| (s.Width as f64, s.Height as f64)))
                    .collect()
            })
            .join()
            .ok()
            .flatten()
        })
    }
}

/// The Mac draws the pages itself with Core Graphics (`CGPDFDocument`, part of every Mac), as Preview does: each page's
/// crop box, turned as the page says, at twice its size in points on white, so a page is the same number of pixels as
/// Windows makes it (Windows' size in its pixels times 1.5 is the same twice the points). Saved as PNG by the `image`
/// crate the converter writes every other PNG with.
#[cfg(target_os = "macos")]
mod mac {
    use crate::Reason;
    use std::ffi::{CString, c_char, c_void};
    use std::os::unix::ffi::OsStrExt;
    use std::path::Path;

    #[repr(C)]
    #[derive(Clone, Copy)]
    struct Point {
        x: f64,
        y: f64,
    }
    #[repr(C)]
    #[derive(Clone, Copy)]
    struct Size {
        w: f64,
        h: f64,
    }
    #[repr(C)]
    #[derive(Clone, Copy)]
    struct Rect {
        origin: Point,
        size: Size,
    }
    #[repr(C)]
    #[derive(Clone, Copy)]
    struct Transform {
        a: f64,
        b: f64,
        c: f64,
        d: f64,
        tx: f64,
        ty: f64,
    }

    type Ref = *mut c_void;
    /// kCGPDFCropBox: what a viewer shows of the page
    const CROP_BOX: i32 = 1;
    /// kCGImageAlphaPremultipliedLast, bytes in the order R G B A
    const RGBA: u32 = 1;

    #[link(name = "CoreGraphics", kind = "framework")]
    extern "C" {
        static kCGColorSpaceSRGB: *const c_void;
        fn CGDataProviderCreateWithFilename(name: *const c_char) -> Ref;
        fn CGDataProviderRelease(p: Ref);
        fn CGPDFDocumentCreateWithProvider(p: Ref) -> Ref;
        fn CGPDFDocumentRelease(d: Ref);
        fn CGPDFDocumentIsEncrypted(d: Ref) -> bool;
        fn CGPDFDocumentIsUnlocked(d: Ref) -> bool;
        fn CGPDFDocumentUnlockWithPassword(d: Ref, password: *const c_char) -> bool;
        fn CGPDFDocumentGetNumberOfPages(d: Ref) -> usize;
        fn CGPDFDocumentGetPage(d: Ref, n: usize) -> Ref;
        fn CGPDFPageGetBoxRect(p: Ref, b: i32) -> Rect;
        fn CGPDFPageGetRotationAngle(p: Ref) -> i32;
        fn CGPDFPageGetDrawingTransform(p: Ref, b: i32, r: Rect, rotate: i32, keep: bool) -> Transform;
        fn CGColorSpaceCreateWithName(name: *const c_void) -> Ref;
        fn CGColorSpaceRelease(c: Ref);
        fn CGBitmapContextCreate(
            data: *mut c_void,
            w: usize,
            h: usize,
            bits: usize,
            row: usize,
            cs: Ref,
            info: u32,
        ) -> Ref;
        fn CGContextRelease(c: Ref);
        fn CGContextSetRGBFillColor(c: Ref, r: f64, g: f64, b: f64, a: f64);
        fn CGContextFillRect(c: Ref, r: Rect);
        fn CGContextScaleCTM(c: Ref, sx: f64, sy: f64);
        fn CGContextConcatCTM(c: Ref, t: Transform);
        fn CGContextSetInterpolationQuality(c: Ref, q: i32);
        fn CGContextDrawPDFPage(c: Ref, p: Ref);
    }

    /// An open PDF, released when dropped.
    struct Doc(Ref);
    impl Drop for Doc {
        fn drop(&mut self) {
            // SAFETY: a document this side created
            unsafe { CGPDFDocumentRelease(self.0) }
        }
    }

    fn open(p: &Path) -> Result<Doc, Reason> {
        let path = std::path::absolute(p).map_err(|_| Reason::Unreadable)?;
        let c = CString::new(path.as_os_str().as_bytes()).map_err(|_| Reason::Unreadable)?;
        // SAFETY: a NUL-terminated path; the provider is released once the document holds it
        let doc = unsafe {
            let provider = CGDataProviderCreateWithFilename(c.as_ptr());
            if provider.is_null() {
                return Err(Reason::Unreadable);
            }
            let doc = CGPDFDocumentCreateWithProvider(provider);
            CGDataProviderRelease(provider);
            doc
        };
        if doc.is_null() {
            return Err(Reason::Damaged);
        }
        let doc = Doc(doc);
        // SAFETY: a live document; an empty password is what opens a file locked only against changes (or AES with
        // no user password), as Preview and Windows open it
        unsafe {
            if CGPDFDocumentIsEncrypted(doc.0)
                && !CGPDFDocumentIsUnlocked(doc.0)
                && !CGPDFDocumentUnlockWithPassword(doc.0, c"".as_ptr())
            {
                return Err(Reason::Locked);
            }
        }
        // SAFETY: a live document
        if unsafe { CGPDFDocumentGetNumberOfPages(doc.0) } == 0 {
            return Err(Reason::Damaged);
        }
        Ok(doc)
    }

    /// A page's size in points as it is shown: its crop box, turned a quarter or three when the page says so.
    fn shown_size(page: Ref) -> (f64, f64) {
        // SAFETY: a page of a live document
        let (r, turn) = unsafe { (CGPDFPageGetBoxRect(page, CROP_BOX), CGPDFPageGetRotationAngle(page)) };
        let (w, h) = (r.size.w.abs(), r.size.h.abs());
        if turn.rem_euclid(180) == 90 { (h, w) } else { (w, h) }
    }

    fn page(doc: &Doc, i: usize) -> Result<Ref, Reason> {
        // SAFETY: pages count from 1; the page belongs to the document and lives as long as it
        let p = unsafe { CGPDFDocumentGetPage(doc.0, i + 1) };
        if p.is_null() { Err(Reason::Damaged) } else { Ok(p) }
    }

    /// One page drawn at twice its size in points on white, as RGBA rows from the top.
    fn draw(page: Ref) -> Result<(u32, u32, Vec<u8>), Reason> {
        let (pw, ph) = shown_size(page);
        let (w, h) = ((pw * 2.0).round().max(1.0) as usize, (ph * 2.0).round().max(1.0) as usize);
        if w as u64 * h as u64 > 400_000_000 {
            return Err(Reason::TooLarge);
        }
        let mut pixels = Vec::new();
        pixels.try_reserve_exact(w * h * 4).map_err(|_| Reason::TooLarge)?;
        pixels.resize(w * h * 4, 0u8);
        // SAFETY: the buffer is w × h × 4 bytes and outlives the context, released here before it is read
        unsafe {
            let cs = CGColorSpaceCreateWithName(kCGColorSpaceSRGB);
            let ctx = CGBitmapContextCreate(pixels.as_mut_ptr() as *mut c_void, w, h, 8, w * 4, cs, RGBA);
            CGColorSpaceRelease(cs);
            if ctx.is_null() {
                return Err(Reason::TooLarge);
            }
            let all = Rect { origin: Point { x: 0.0, y: 0.0 }, size: Size { w: w as f64, h: h as f64 } };
            CGContextSetRGBFillColor(ctx, 1.0, 1.0, 1.0, 1.0);
            CGContextFillRect(ctx, all);
            // twice the size: the page is then laid into a rectangle of its own size in points, which the drawing
            // transform only moves and turns (it never scales up)
            CGContextScaleCTM(ctx, w as f64 / pw, h as f64 / ph);
            let box_ = Rect { origin: Point { x: 0.0, y: 0.0 }, size: Size { w: pw, h: ph } };
            CGContextConcatCTM(ctx, CGPDFPageGetDrawingTransform(page, CROP_BOX, box_, 0, true));
            // kCGInterpolationHigh: pictures inside the page drawn smooth, as Preview draws them
            CGContextSetInterpolationQuality(ctx, 3);
            CGContextDrawPDFPage(ctx, page);
            CGContextRelease(ctx);
        }
        Ok((w as u32, h as u32, pixels))
    }

    fn png(w: u32, h: u32, rgba: &[u8]) -> Result<Vec<u8>, Reason> {
        use image::ImageEncoder;
        let mut out = Vec::new();
        image::codecs::png::PngEncoder::new(&mut out)
            .write_image(rgba, w, h, image::ExtendedColorType::Rgba8)
            .map_err(|_| Reason::Damaged)?;
        Ok(out)
    }

    /// Each page's PNG, handed over as it is drawn: one page in memory at a time.
    pub fn pages(p: &Path, each: &mut dyn FnMut(u32, u32, Vec<u8>) -> Result<(), Reason>) -> Result<(), Reason> {
        let doc = open(p)?;
        // SAFETY: a live document
        let count = unsafe { CGPDFDocumentGetNumberOfPages(doc.0) };
        for i in 0..count {
            let (w, h, rgba) = draw(page(&doc, i)?)?;
            let bytes = png(w, h, &rgba)?;
            drop(rgba);
            each(i as u32, count as u32, bytes)?;
        }
        Ok(())
    }

    /// Each page's size in points as shown, without drawing any.
    pub fn sizes(p: &Path) -> Option<Vec<(f64, f64)>> {
        let doc = open(p).ok()?;
        // SAFETY: a live document
        let count = unsafe { CGPDFDocumentGetNumberOfPages(doc.0) };
        (0..count).map(|i| page(&doc, i).ok().map(shown_size)).collect()
    }
}

#[cfg(windows)]
pub fn available() -> bool {
    win::available()
}

/// Core Graphics is part of every Mac.
#[cfg(target_os = "macos")]
pub fn available() -> bool {
    true
}

#[cfg(not(any(windows, target_os = "macos")))]
pub fn available() -> bool {
    false
}

#[cfg(target_os = "macos")]
use mac as drawer;
/// Who draws the pages: Windows' renderer, or the Mac's Core Graphics.
#[cfg(windows)]
use win as drawer;

#[cfg(any(windows, target_os = "macos"))]
pub fn convert(p: &Path, _target: &str, out: &mut Outputs, step: Step) -> Result<(), Reason> {
    crate::open(p)?;
    // a file named .pdf that isn't one is damaged, not a reason to ask the system
    let mut head = [0u8; 1024];
    let n = std::io::Read::read(&mut crate::open(p)?, &mut head).map_err(|_| Reason::Unreadable)?;
    if !head[..n].windows(5).any(|w| w == b"%PDF-") {
        return Err(Reason::Damaged);
    }
    let stem = out.stem().to_string();
    drawer::pages(p, &mut |i, count, png| {
        let name = if count == 1 { stem.clone() } else { format!("{stem} {}", i + 1) };
        out.write_named(&name, "png", &png)?;
        step((i + 1) as f32 / count as f32);
        Ok(())
    })
}

/// Elsewhere no pages are drawn: `available` is false, so a PDF is never offered, and asked anyway it is refused the way
/// any file the converter doesn't take is.
#[cfg(not(any(windows, target_os = "macos")))]
pub fn convert(_p: &Path, _target: &str, _out: &mut Outputs, _step: Step) -> Result<(), Reason> {
    Err(Reason::Refused(crate::Refusal::Unsupported))
}

pub fn estimate(p: &Path, size: u64, _target: &str) -> u64 {
    // elsewhere no page is drawn (a PDF isn't offered there), and the file's own size stands
    #[cfg(not(any(windows, target_os = "macos")))]
    let _ = p;
    #[cfg(any(windows, target_os = "macos"))]
    if let Some(pages) = drawer::sizes(p) {
        // the first page really drawn, and the rest taken to pack the way it does, page area for page area
        let mut first = None;
        let _ = drawer::pages(p, &mut |_, _, png| {
            first = Some(png.len() as f64);
            Err(Reason::Damaged)
        });
        let area: f64 = pages.iter().map(|(w, h)| w * h).sum();
        if let (Some(bytes), Some((w, h))) = (first, pages.first()) {
            return (bytes * area / (w * h).max(1.0)) as u64;
        }
    }
    size
}
