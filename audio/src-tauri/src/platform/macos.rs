//! Sound the Mac decodes: M4A (AAC, HE-AAC, ALAC), through AudioToolbox's `ExtAudioFile`. No decoder of ours is involved,
//! and nothing is ever written back.
//!
//! Why `ExtAudioFile` and not AVFoundation's `AVAssetReader`: it is the same system decoder, but a plain C interface that
//! reads on whatever thread asks, with no Objective-C objects to keep alive; and it reads the file's own packet table
//! (the encoder's priming and padding, from the M4A's edit list or iTunes' gapless note), so what comes out is exactly
//! the song's frames, start to end, as Music plays them gaplessly. HE-AAC is decoded at its full rate (the top layer of
//! the file's format list) without being asked. The Mac always has these decoders, so there is no "missing decoder"
//! here: a file the system can't read is a damaged one.

use std::ffi::c_void;
use std::path::Path;

#[derive(Debug, PartialEq)]
pub enum SystemError {
    Damaged,
    /// The callback asked to stop.
    Stopped,
}

#[repr(C)]
#[derive(Default, Clone, Copy)]
struct Asbd {
    sample_rate: f64,
    format_id: u32,
    format_flags: u32,
    bytes_per_packet: u32,
    frames_per_packet: u32,
    bytes_per_frame: u32,
    channels_per_frame: u32,
    bits_per_channel: u32,
    reserved: u32,
}

#[repr(C)]
struct AudioBuffer {
    channels: u32,
    size: u32,
    data: *mut c_void,
}

#[repr(C)]
struct AudioBufferList {
    count: u32,
    buffers: [AudioBuffer; 1],
}

type ExtAudioFileRef = *mut c_void;
type OsStatus = i32;

const fn four(c: &[u8; 4]) -> u32 {
    u32::from_be_bytes(*c)
}
const FILE_DATA_FORMAT: u32 = four(b"ffmt");
const CLIENT_DATA_FORMAT: u32 = four(b"cfmt");
const LINEAR_PCM: u32 = four(b"lpcm");
const APPLE_LOSSLESS: u32 = four(b"alac");
const FLOAT: u32 = 1;
const PACKED: u32 = 8;

#[link(name = "AudioToolbox", kind = "framework")]
unsafe extern "C" {
    fn ExtAudioFileOpenURL(url: *const c_void, file: *mut ExtAudioFileRef) -> OsStatus;
    fn ExtAudioFileDispose(file: ExtAudioFileRef) -> OsStatus;
    fn ExtAudioFileGetProperty(file: ExtAudioFileRef, id: u32, size: *mut u32, data: *mut c_void) -> OsStatus;
    fn ExtAudioFileSetProperty(file: ExtAudioFileRef, id: u32, size: u32, data: *const c_void) -> OsStatus;
    fn ExtAudioFileRead(file: ExtAudioFileRef, frames: *mut u32, data: *mut AudioBufferList) -> OsStatus;
}

#[link(name = "CoreFoundation", kind = "framework")]
unsafe extern "C" {
    fn CFURLCreateFromFileSystemRepresentation(
        allocator: *const c_void,
        buffer: *const u8,
        length: isize,
        is_directory: u8,
    ) -> *const c_void;
    fn CFRelease(cf: *const c_void);
}

/// The open file, disposed of however the reading ends.
struct File(ExtAudioFileRef);

impl Drop for File {
    fn drop(&mut self) {
        unsafe { ExtAudioFileDispose(self.0) };
    }
}

fn open(path: &Path) -> Result<File, SystemError> {
    use std::os::unix::ffi::OsStrExt;
    let bytes = path.as_os_str().as_bytes();
    unsafe {
        let url = CFURLCreateFromFileSystemRepresentation(std::ptr::null(), bytes.as_ptr(), bytes.len() as isize, 0);
        if url.is_null() {
            return Err(SystemError::Damaged);
        }
        let mut file = std::ptr::null_mut();
        let status = ExtAudioFileOpenURL(url, &mut file);
        CFRelease(url);
        if status != 0 || file.is_null() {
            return Err(SystemError::Damaged);
        }
        Ok(File(file))
    }
}

/// ALAC says the bits of the sound it was made from in its format flags (`kAppleLosslessFormatFlag_…BitSourceData`).
fn alac_bits(flags: u32) -> Option<u32> {
    match flags {
        1 => Some(16),
        2 => Some(20),
        3 => Some(24),
        4 => Some(32),
        _ => None,
    }
}

/// Decodes the first sound track to 32-bit floats, handing each stretch to `each(channels, rate, samples)`; `each` returns false to stop.
/// Returns the source's bits per sample where it has such a thing (ALAC), and nothing for AAC. What comes out is the song
/// only: the encoder's priming and padding are already gone.
pub fn system_decode(
    path: &Path,
    mut each: impl FnMut(usize, u32, &[f32]) -> bool,
) -> Result<Option<u32>, SystemError> {
    let file = open(path)?;
    let mut source = Asbd::default();
    let mut size = std::mem::size_of::<Asbd>() as u32;
    if unsafe { ExtAudioFileGetProperty(file.0, FILE_DATA_FORMAT, &mut size, &mut source as *mut Asbd as *mut c_void) }
        != 0
    {
        return Err(SystemError::Damaged);
    }
    let channels = source.channels_per_frame as usize;
    let rate = source.sample_rate.round() as u32;
    if channels == 0 || rate == 0 {
        return Err(SystemError::Damaged);
    }
    let bits = if source.format_id == APPLE_LOSSLESS { alac_bits(source.format_flags) } else { None };
    // the file's own rate and channels, as floats side by side
    let client = Asbd {
        sample_rate: source.sample_rate,
        format_id: LINEAR_PCM,
        format_flags: FLOAT | PACKED,
        bytes_per_packet: 4 * channels as u32,
        frames_per_packet: 1,
        bytes_per_frame: 4 * channels as u32,
        channels_per_frame: channels as u32,
        bits_per_channel: 32,
        reserved: 0,
    };
    if unsafe {
        ExtAudioFileSetProperty(
            file.0,
            CLIENT_DATA_FORMAT,
            std::mem::size_of::<Asbd>() as u32,
            &client as *const Asbd as *const c_void,
        )
    } != 0
    {
        return Err(SystemError::Damaged);
    }
    const FRAMES: usize = 8192;
    let mut samples = vec![0f32; FRAMES * channels];
    loop {
        let mut list = AudioBufferList {
            count: 1,
            buffers: [AudioBuffer {
                channels: channels as u32,
                size: (samples.len() * 4) as u32,
                data: samples.as_mut_ptr() as *mut c_void,
            }],
        };
        let mut frames = FRAMES as u32;
        // a file that fails partway ends there; what came before is still sound
        if unsafe { ExtAudioFileRead(file.0, &mut frames, &mut list) } != 0 || frames == 0 {
            break;
        }
        let got = (frames as usize * channels).min(samples.len());
        if !each(channels, rate, &samples[..got]) {
            return Err(SystemError::Stopped);
        }
    }
    Ok(bits)
}
