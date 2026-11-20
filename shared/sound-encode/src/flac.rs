//! FLAC encoding through libFLAC's published functions. It's BSD and goes into the exe from vendor/flac; see build.rs.

use std::ffi::{c_int, c_uint, c_void};
use std::io::{Seek, SeekFrom, Write};

#[allow(non_camel_case_types)]
type Enc = c_void;
pub trait Out: Write + Seek + Send {}
impl<T: Write + Seek + Send> Out for T {}

type WriteCb = unsafe extern "C" fn(*const Enc, *const u8, usize, c_uint, c_uint, *mut c_void) -> c_int;
type SeekCb = unsafe extern "C" fn(*const Enc, u64, *mut c_void) -> c_int;
type TellCb = unsafe extern "C" fn(*const Enc, *mut u64, *mut c_void) -> c_int;
type MetaCb = unsafe extern "C" fn(*const Enc, *const c_void, *mut c_void);

extern "C" {
    fn FLAC__stream_encoder_new() -> *mut Enc;
    fn FLAC__stream_encoder_delete(e: *mut Enc);
    fn FLAC__stream_encoder_set_channels(e: *mut Enc, v: c_uint) -> c_int;
    fn FLAC__stream_encoder_set_bits_per_sample(e: *mut Enc, v: c_uint) -> c_int;
    fn FLAC__stream_encoder_set_sample_rate(e: *mut Enc, v: c_uint) -> c_int;
    fn FLAC__stream_encoder_set_compression_level(e: *mut Enc, v: c_uint) -> c_int;
    fn FLAC__stream_encoder_set_total_samples_estimate(e: *mut Enc, v: u64) -> c_int;
    fn FLAC__stream_encoder_set_metadata(e: *mut Enc, blocks: *mut *mut c_void, n: c_uint) -> c_int;
    fn FLAC__stream_encoder_init_stream(
        e: *mut Enc,
        w: Option<WriteCb>,
        s: Option<SeekCb>,
        t: Option<TellCb>,
        m: Option<MetaCb>,
        data: *mut c_void,
    ) -> c_int;
    fn FLAC__stream_encoder_process_interleaved(e: *mut Enc, buffer: *const i32, frames: c_uint) -> c_int;
    fn FLAC__stream_encoder_finish(e: *mut Enc) -> c_int;
    fn FLAC__metadata_object_new(kind: c_int) -> *mut c_void;
    fn FLAC__metadata_object_delete(o: *mut c_void);
    fn FLAC__metadata_object_seektable_template_append_spaced_points_by_samples(
        o: *mut c_void,
        samples: c_uint,
        total: u64,
    ) -> c_int;
    fn FLAC__metadata_object_seektable_template_sort(o: *mut c_void, compact: c_int) -> c_int;
}

const METADATA_SEEKTABLE: c_int = 3;

struct State {
    out: Box<dyn Out>,
    failed: bool,
}

unsafe extern "C" fn on_write(
    _e: *const Enc,
    buffer: *const u8,
    bytes: usize,
    _s: c_uint,
    _f: c_uint,
    data: *mut c_void,
) -> c_int {
    let st = &mut *(data as *mut State);
    match st.out.write_all(std::slice::from_raw_parts(buffer, bytes)) {
        Ok(()) => 0,
        Err(_) => {
            st.failed = true;
            1
        }
    }
}
unsafe extern "C" fn on_seek(_e: *const Enc, at: u64, data: *mut c_void) -> c_int {
    let st = &mut *(data as *mut State);
    if st.out.seek(SeekFrom::Start(at)).is_ok() { 0 } else { 1 }
}
unsafe extern "C" fn on_tell(_e: *const Enc, at: *mut u64, data: *mut c_void) -> c_int {
    let st = &mut *(data as *mut State);
    match st.out.stream_position() {
        Ok(p) => {
            *at = p;
            0
        }
        Err(_) => 1,
    }
}

pub struct FlacEncoder {
    enc: *mut Enc,
    state: Box<State>,
    seek: *mut c_void,
    channels: usize,
    buf: Vec<i32>,
    scale: f32,
    max: f32,
}

// the encoder is used from one thread at a time
unsafe impl Send for FlacEncoder {}

impl FlacEncoder {
    pub fn new(out: Box<dyn Out>, channels: usize, rate: u32, bits: u32, total: u64) -> Result<Self, String> {
        // SAFETY: each call follows libFLAC's documented order: new, set, init, process, finish, delete
        unsafe {
            let enc = FLAC__stream_encoder_new();
            if enc.is_null() {
                return Err("FLAC encoder".into());
            }
            FLAC__stream_encoder_set_channels(enc, channels as c_uint);
            FLAC__stream_encoder_set_bits_per_sample(enc, bits);
            FLAC__stream_encoder_set_sample_rate(enc, rate);
            FLAC__stream_encoder_set_compression_level(enc, 5);
            FLAC__stream_encoder_set_total_samples_estimate(enc, total);
            // a point every ten seconds, so other players can jump around
            let mut seek = FLAC__metadata_object_new(METADATA_SEEKTABLE);
            if !seek.is_null() && total > 0 {
                FLAC__metadata_object_seektable_template_append_spaced_points_by_samples(
                    seek,
                    rate.saturating_mul(10),
                    total,
                );
                FLAC__metadata_object_seektable_template_sort(seek, 1);
                FLAC__stream_encoder_set_metadata(enc, &mut seek, 1);
            }
            let mut state = Box::new(State { out, failed: false });
            let status = FLAC__stream_encoder_init_stream(
                enc,
                Some(on_write),
                Some(on_seek),
                Some(on_tell),
                None,
                &mut *state as *mut State as *mut c_void,
            );
            if status != 0 {
                FLAC__stream_encoder_delete(enc);
                if !seek.is_null() {
                    FLAC__metadata_object_delete(seek);
                }
                return Err(format!("FLAC encoder status {status}"));
            }
            let scale = (1u64 << (bits - 1)) as f32;
            Ok(FlacEncoder { enc, state, seek, channels, buf: Vec::new(), scale, max: scale - 1.0 })
        }
    }

    pub fn write(&mut self, planar: &[Vec<f32>], n: usize) -> Result<(), String> {
        self.buf.clear();
        self.buf.reserve(n * self.channels);
        for f in 0..n {
            for p in planar.iter().take(self.channels) {
                self.buf.push((p[f] * self.scale).round().clamp(-self.scale, self.max) as i32);
            }
        }
        // SAFETY: the buffer holds n frames of this many channels
        let ok = unsafe { FLAC__stream_encoder_process_interleaved(self.enc, self.buf.as_ptr(), n as c_uint) };
        if ok == 0 || self.state.failed {
            return Err("FLAC write".into());
        }
        Ok(())
    }

    pub fn finish(mut self) -> Result<(), String> {
        // SAFETY: finish closes the stream, writing the totals back through on_seek
        let ok = unsafe { FLAC__stream_encoder_finish(self.enc) };
        if ok == 0 || self.state.failed {
            return Err("FLAC finish".into());
        }
        self.state.out.flush().map_err(|e| e.to_string())
    }
}

impl Drop for FlacEncoder {
    fn drop(&mut self) {
        // SAFETY: delete finishes first if finish wasn't reached
        unsafe {
            FLAC__stream_encoder_delete(self.enc);
            if !self.seek.is_null() {
                FLAC__metadata_object_delete(self.seek);
            }
        }
    }
}
