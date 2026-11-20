//! Sound that Windows decodes: M4A, through Media Foundation's source reader. No decoder of ours is involved, and nothing is ever written back.
//! Media Foundation is looked up when it's first needed, not linked, so a Windows without it (the N editions) still starts this program
//! and can say why an M4A won't open.

use std::ffi::c_void;
use std::path::Path;
use std::sync::OnceLock;
use windows::Win32::Media::MediaFoundation::{
    IMFByteStream, IMFMediaType, IMFSample, IMFSourceReader, MF_MT_AUDIO_BITS_PER_SAMPLE, MF_MT_AUDIO_NUM_CHANNELS,
    MF_MT_AUDIO_SAMPLES_PER_SECOND, MF_MT_MAJOR_TYPE, MF_MT_SUBTYPE, MF_SOURCE_READER_ALL_STREAMS,
    MF_SOURCE_READER_FIRST_AUDIO_STREAM, MF_SOURCE_READERF_CURRENTMEDIATYPECHANGED, MF_SOURCE_READERF_ENDOFSTREAM,
    MF_SOURCE_READERF_ERROR, MF_VERSION, MFAudioFormat_ALAC, MFAudioFormat_Float, MFMediaType_Audio,
};
use windows::Win32::System::Com::{COINIT_MULTITHREADED, CoInitializeEx, CoUninitialize};
use windows::core::{HRESULT, Interface, PCWSTR};
use windows_sys::Win32::System::LibraryLoader::{GetProcAddress, LOAD_LIBRARY_SEARCH_SYSTEM32, LoadLibraryExW};

#[derive(Debug, PartialEq)]
pub enum SystemError {
    /// This Windows can't decode it: Media Foundation or its AAC or ALAC decoder isn't there.
    NoDecoder,
    Damaged,
    /// The callback asked to stop.
    Stopped,
}

type Startup = unsafe extern "system" fn(u32, u32) -> HRESULT;
type Shutdown = unsafe extern "system" fn() -> HRESULT;
type CreateFile = unsafe extern "system" fn(i32, i32, i32, PCWSTR, *mut *mut c_void) -> HRESULT;
type CreateType = unsafe extern "system" fn(*mut *mut c_void) -> HRESULT;
type CreateReader = unsafe extern "system" fn(*mut c_void, *mut c_void, *mut *mut c_void) -> HRESULT;

struct Mf {
    startup: Startup,
    shutdown: Shutdown,
    create_file: CreateFile,
    create_type: CreateType,
    create_reader: CreateReader,
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
            create_type: symbol("mfplat.dll", "MFCreateMediaType")?,
            create_reader: symbol("mfreadwrite.dll", "MFCreateSourceReaderFromByteStream")?,
        })
    })
    .as_ref()
}

/// COM and Media Foundation for as long as this lives, on this thread.
struct Session {
    mf: &'static Mf,
    com: bool,
}

impl Session {
    fn start() -> Result<Session, SystemError> {
        // how a Windows without Media Foundation looks, for the tests
        if std::env::var_os("DOTANY_TEST_NO_MEDIA_FOUNDATION").is_some() {
            return Err(SystemError::NoDecoder);
        }
        let mf = mf().ok_or(SystemError::NoDecoder)?;
        let com = unsafe { CoInitializeEx(None, COINIT_MULTITHREADED) }.is_ok();
        // MFSTARTUP_LITE: no network sources
        if unsafe { (mf.startup)(MF_VERSION, 1) }.is_err() {
            if com {
                unsafe { CoUninitialize() };
            }
            return Err(SystemError::NoDecoder);
        }
        Ok(Session { mf, com })
    }
}

impl Drop for Session {
    fn drop(&mut self) {
        unsafe {
            let _ = (self.mf.shutdown)();
            if self.com {
                CoUninitialize();
            }
        }
    }
}

// what Windows 11's MPEG-4 source calls ALAC; older SDKs name the wave-format number instead
const ALAC: windows::core::GUID = windows::core::GUID::from_u128(0x616c6163_767a_494d_b478_f29d25dc9037);

const AUDIO: u32 = MF_SOURCE_READER_FIRST_AUDIO_STREAM.0 as u32;

fn reader(s: &Session, path: &Path) -> Result<IMFSourceReader, SystemError> {
    let name: Vec<u16> = path.as_os_str().to_string_lossy().encode_utf16().chain(std::iter::once(0)).collect();
    unsafe {
        let mut stream = std::ptr::null_mut();
        // read only, fail if it isn't there, no special flags
        (s.mf.create_file)(1, 0, 0, PCWSTR(name.as_ptr()), &mut stream).ok().map_err(|_| SystemError::Damaged)?;
        let stream = IMFByteStream::from_raw(stream);
        let mut reader = std::ptr::null_mut();
        (s.mf.create_reader)(stream.as_raw(), std::ptr::null_mut(), &mut reader)
            .ok()
            .map_err(|_| SystemError::Damaged)?;
        Ok(IMFSourceReader::from_raw(reader))
    }
}

fn shape(t: &IMFMediaType) -> Result<(usize, u32), SystemError> {
    unsafe {
        let channels = t.GetUINT32(&MF_MT_AUDIO_NUM_CHANNELS).map_err(|_| SystemError::Damaged)? as usize;
        let rate = t.GetUINT32(&MF_MT_AUDIO_SAMPLES_PER_SECOND).map_err(|_| SystemError::Damaged)?;
        if channels == 0 || rate == 0 {
            return Err(SystemError::Damaged);
        }
        Ok((channels, rate))
    }
}

/// Decodes the first sound track to 32-bit floats, handing each stretch to `each(channels, rate, samples)`; `each` returns false to stop.
/// Returns the source's bits per sample where it has such a thing (ALAC), and nothing for AAC.
pub fn system_decode(
    path: &Path,
    mut each: impl FnMut(usize, u32, &[f32]) -> bool,
) -> Result<Option<u32>, SystemError> {
    let session = Session::start()?;
    let reader = reader(&session, path)?;
    unsafe {
        let _ = reader.SetStreamSelection(MF_SOURCE_READER_ALL_STREAMS.0 as u32, false);
        reader.SetStreamSelection(AUDIO, true).map_err(|_| SystemError::Damaged)?;
        let native = reader.GetNativeMediaType(AUDIO, 0).map_err(|_| SystemError::Damaged)?;
        let bits = match native.GetGUID(&MF_MT_SUBTYPE) {
            Ok(g) if g == MFAudioFormat_ALAC || g == ALAC => native.GetUINT32(&MF_MT_AUDIO_BITS_PER_SAMPLE).ok(),
            _ => None,
        };
        let mut want = std::ptr::null_mut();
        (session.mf.create_type)(&mut want).ok().map_err(|_| SystemError::NoDecoder)?;
        let want = IMFMediaType::from_raw(want);
        want.SetGUID(&MF_MT_MAJOR_TYPE, &MFMediaType_Audio).map_err(|_| SystemError::NoDecoder)?;
        want.SetGUID(&MF_MT_SUBTYPE, &MFAudioFormat_Float).map_err(|_| SystemError::NoDecoder)?;
        // this is where a missing decoder shows: there's nothing to turn the file's sound into samples
        reader.SetCurrentMediaType(AUDIO, None, &want).map_err(|_| SystemError::NoDecoder)?;
        let (mut channels, mut rate) = shape(&reader.GetCurrentMediaType(AUDIO).map_err(|_| SystemError::Damaged)?)?;
        loop {
            let mut flags = 0u32;
            let mut sample: Option<IMFSample> = None;
            // a file that fails partway ends there; what came before is still sound
            if reader.ReadSample(AUDIO, 0, None, Some(&mut flags), None, Some(&mut sample)).is_err()
                || flags & MF_SOURCE_READERF_ERROR.0 as u32 != 0
            {
                break;
            }
            if flags & MF_SOURCE_READERF_CURRENTMEDIATYPECHANGED.0 as u32 != 0 {
                (channels, rate) = shape(&reader.GetCurrentMediaType(AUDIO).map_err(|_| SystemError::Damaged)?)?;
            }
            if let Some(sample) = sample {
                let buffer = sample.ConvertToContiguousBuffer().map_err(|_| SystemError::Damaged)?;
                let mut data = std::ptr::null_mut();
                let mut len = 0u32;
                buffer.Lock(&mut data, None, Some(&mut len)).map_err(|_| SystemError::Damaged)?;
                let floats = std::slice::from_raw_parts(data as *const f32, len as usize / 4);
                let go_on = each(channels, rate, floats);
                let _ = buffer.Unlock();
                if !go_on {
                    return Err(SystemError::Stopped);
                }
            }
            if flags & MF_SOURCE_READERF_ENDOFSTREAM.0 as u32 != 0 {
                break;
            }
        }
        Ok(bits)
    }
}
