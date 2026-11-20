//! Bytes to samples. The file is read once, start to end, into the store; after that the disk file is only needed to copy it.
//! M4A is decoded by the system (`platform::system_decode`: Media Foundation on Windows, AudioToolbox on the Mac); tracker modules are played by libopenmpt (`module`); AU and VOC
//! are read by our own `oldsound`; everything else by Symphonia, with only our formats compiled in.

use crate::formats::{self, Kind, Sniff};
use crate::store::{Builder, Pcm};
use std::path::Path;
use std::sync::atomic::{AtomicU64, Ordering};
use symphonia::core::audio::GenericAudioBufferRef;
use symphonia::core::codecs::audio::AudioDecoderOptions;
use symphonia::core::errors::Error as SymError;
use symphonia::core::formats::probe::Hint;
use symphonia::core::formats::{FormatOptions, TrackType};
use symphonia::core::io::{MediaSourceStream, ReadOnlySource};
use symphonia::core::meta::MetadataOptions;

#[derive(Debug, PartialEq)]
pub enum OpenError {
    Refused,
    Damaged,
    /// There isn't room for the samples.
    TooLarge,
    /// The system has no decoder for it: an M4A on a Windows without Media Foundation's.
    NoDecoder,
    Abandoned,
}

pub struct Decoded {
    pub kind: Kind,
    pub pcm: Pcm,
    /// Bits per sample of the source, where it has such a thing. Lossy files have none.
    pub bits: Option<u32>,
    /// Words the file holds in its own way rather than in a tag (a module's title): shown, and carried into a save.
    pub words: Option<lofty::tag::Tag>,
}

// a decoder that's lost its place loses a packet or two; a file made of nothing but errors isn't audio
const MAX_BAD_PACKETS: u32 = 1000;

fn copy_out(buf: &GenericAudioBufferRef<'_>, out: &mut Vec<f32>) {
    out.resize(buf.samples_interleaved(), 0.0);
    buf.copy_to_slice_interleaved(&mut out[..]);
}

/// Reads a whole file. `seq` and `current` let a newer open abandon this one.
pub fn read_file(path: &Path, temp_dir: &Path, seq: u64, current: &AtomicU64) -> Result<Decoded, OpenError> {
    let sniff = formats::sniff_file(path).map_err(|_| OpenError::Damaged)?;
    let kind = match sniff {
        Sniff::Is(k) => k,
        Sniff::Refused => return Err(OpenError::Refused),
        Sniff::Damaged => return Err(OpenError::Damaged),
    };
    if kind == Kind::M4a {
        #[cfg(any(windows, target_os = "macos"))]
        return read_by_system(path, temp_dir, seq, current);
        #[cfg(not(any(windows, target_os = "macos")))]
        return Err(OpenError::Refused);
    }
    if kind.module() {
        return read_module(path, kind, temp_dir, seq, current);
    }
    if kind == Kind::Au || kind == Kind::Voc || (kind == Kind::Wav && g711(path)) {
        return read_old(path, kind, temp_dir, seq, current);
    }
    let mut hint = Hint::new();
    hint.with_extension(kind.ext());
    let file = std::fs::File::open(path).map_err(|_| OpenError::Damaged)?;
    let mss = MediaSourceStream::new(Box::new(file), Default::default());
    let mut format =
        match symphonia::default::get_probe().probe(&hint, mss, FormatOptions::default(), MetadataOptions::default()) {
            Ok(f) => f,
            // an AIFF cut off partway fails when the reader looks past the end of its sound; told it can't look ahead, it just reads
            Err(_) if kind == Kind::Aiff => {
                let file = std::fs::File::open(path).map_err(|_| OpenError::Damaged)?;
                let mss = MediaSourceStream::new(Box::new(ReadOnlySource::new(file)), Default::default());
                symphonia::default::get_probe()
                    .probe(&hint, mss, FormatOptions::default(), MetadataOptions::default())
                    .map_err(|_| OpenError::Damaged)?
            }
            Err(_) => return Err(OpenError::Damaged),
        };
    let track = format.default_track(TrackType::Audio).ok_or(OpenError::Damaged)?;
    let track_id = track.id;
    let params = track.codec_params.as_ref().and_then(|c| c.audio()).ok_or(OpenError::Damaged)?.clone();
    let bits = if kind.lossy() { None } else { params.bits_per_sample.or(params.bits_per_coded_sample) };
    let mut decoder = symphonia::default::get_codecs()
        .make_audio_decoder(&params, &AudioDecoderOptions::default())
        .map_err(|_| OpenError::Damaged)?;

    let mut builder: Option<Builder> = None;
    let mut scratch: Vec<f32> = Vec::new();
    let mut bad = 0;
    let fail = |b: Option<Builder>, e: OpenError| {
        if let Some(b) = b {
            b.discard();
        }
        Err(e)
    };
    loop {
        if current.load(Ordering::Relaxed) != seq {
            return fail(builder, OpenError::Abandoned);
        }
        let packet = match format.next_packet() {
            Ok(Some(p)) => p,
            Ok(None) => break,
            // a cut-off or chained file ends here; what came before is still sound
            Err(_) => break,
        };
        if packet.track_id != track_id {
            continue;
        }
        match decoder.decode(&packet) {
            Ok(buf) => {
                let spec = buf.spec();
                let (channels, rate) = (spec.channels().count(), spec.rate());
                copy_out(&buf, &mut scratch);
                if channels == 0 || rate == 0 {
                    return fail(builder, OpenError::Damaged);
                }
                let b = builder.get_or_insert_with(|| Builder::new(channels, rate, temp_dir));
                // a file that changes its shape partway keeps the shape it began with
                if !b.fits(channels, rate) {
                    bad += 1;
                    continue;
                }
                if b.push(&scratch).is_err() {
                    return fail(builder, OpenError::TooLarge);
                }
            }
            Err(SymError::DecodeError(_)) | Err(SymError::IoError(_)) => {
                bad += 1;
                if bad > MAX_BAD_PACKETS {
                    return fail(builder, OpenError::Damaged);
                }
            }
            Err(_) => break,
        }
    }
    let Some(b) = builder else { return Err(OpenError::Damaged) };
    if b.frames() == 0 {
        b.discard();
        return Err(OpenError::Damaged);
    }
    let pcm = b.finish().map_err(|_| OpenError::TooLarge)?;
    Ok(Decoded { kind, pcm, bits, words: None })
}

/// A tracker module, played once through by libopenmpt, as stereo floats.
fn read_module(path: &Path, kind: Kind, temp_dir: &Path, seq: u64, current: &AtomicU64) -> Result<Decoded, OpenError> {
    use crate::module::{MAX_FILE, ModError, Module, RATE};
    use lofty::tag::{Accessor, Tag, TagType};
    if std::fs::metadata(path).map_err(|_| OpenError::Damaged)?.len() > MAX_FILE {
        return Err(OpenError::Damaged);
    }
    let data = std::fs::read(path).map_err(|_| OpenError::Damaged)?;
    let mut module = Module::load(data, kind)
        .map_err(|e| if e == ModError::Refused { OpenError::Refused } else { OpenError::Damaged })?;
    let mut tag = Tag::new(TagType::Id3v2);
    let (title, artist) = (module.meta("title"), module.meta("artist"));
    if !title.is_empty() {
        tag.set_title(title);
    }
    if !artist.is_empty() {
        tag.set_artist(artist);
    }
    let mut b = Builder::new(2, RATE, temp_dir);
    let mut failed = None;
    let done = module.render(|samples| {
        if current.load(Ordering::Relaxed) != seq {
            failed = Some(OpenError::Abandoned);
            return false;
        }
        if b.push(samples).is_err() {
            failed = Some(OpenError::TooLarge);
            return false;
        }
        true
    });
    if done.is_err() || b.frames() == 0 {
        b.discard();
        return Err(failed.unwrap_or(OpenError::Damaged));
    }
    let pcm = b.finish().map_err(|_| OpenError::TooLarge)?;
    Ok(Decoded { kind, pcm, bits: None, words: (!lofty::tag::TagExt::is_empty(&tag)).then_some(tag) })
}

// a WAV in A-law or μ-law, by the start of the file
fn g711(path: &Path) -> bool {
    use std::io::Read;
    let mut head = Vec::new();
    std::fs::File::open(path).and_then(|f| f.take(256 * 1024).read_to_end(&mut head)).is_ok()
        && crate::oldsound::is_g711_wav(&head)
}

/// AU, VOC or a G.711 WAV, by our own reader. The file is mapped, never changed, and read a stretch at a time.
fn read_old(path: &Path, kind: Kind, temp_dir: &Path, seq: u64, current: &AtomicU64) -> Result<Decoded, OpenError> {
    use crate::oldsound::{self, Bad, Part};
    let file = std::fs::File::open(path).map_err(|_| OpenError::Damaged)?;
    // a map of an empty file fails; such a file is damaged anyway
    let map = unsafe { memmap2::Mmap::map(&file) }.map_err(|_| OpenError::Damaged)?;
    let bad = |e: Bad| if e == Bad::Refused { OpenError::Refused } else { OpenError::Damaged };
    let parts = match kind {
        Kind::Au => vec![oldsound::au(&map, map.len()).map_err(bad)?],
        Kind::Wav => vec![oldsound::wav(&map).map_err(bad)?],
        _ => oldsound::voc(&map).map_err(bad)?,
    };
    let first = oldsound::first_shape(&parts).ok_or(OpenError::Damaged)?;
    let mut b = Builder::new(first.channels, first.rate, temp_dir);
    let mut scratch = Vec::new();
    let fail = |b: Builder, e: OpenError| {
        b.discard();
        Err(e)
    };
    for part in &parts {
        match part {
            // a file that changes its shape partway keeps the shape it began with
            Part::Sound(shape, range) if b.fits(shape.channels, shape.rate) => {
                // whole frames, about a megabyte at a time
                let frame = shape.enc.bytes() * shape.channels;
                for chunk in map[range.clone()].chunks(((1 << 20) / frame).max(1) * frame) {
                    if current.load(Ordering::Relaxed) != seq {
                        return fail(b, OpenError::Abandoned);
                    }
                    oldsound::decode(shape, chunk, &mut scratch);
                    if b.push(&scratch).is_err() {
                        return fail(b, OpenError::TooLarge);
                    }
                }
            }
            Part::Sound(..) => {}
            Part::Silence(seconds) => {
                let frames = (seconds * first.rate as f64).round() as usize;
                scratch.clear();
                scratch.resize(frames * first.channels, 0.0);
                if b.push(&scratch).is_err() {
                    return fail(b, OpenError::TooLarge);
                }
            }
        }
    }
    if b.frames() == 0 {
        return fail(b, OpenError::Damaged);
    }
    let pcm = b.finish().map_err(|_| OpenError::TooLarge)?;
    Ok(Decoded { kind, pcm, bits: Some(first.enc.bits()), words: None })
}

#[cfg(any(windows, target_os = "macos"))]
fn read_by_system(path: &Path, temp_dir: &Path, seq: u64, current: &AtomicU64) -> Result<Decoded, OpenError> {
    use crate::platform::{SystemError, system_decode};
    let mut builder: Option<Builder> = None;
    let mut failed = None;
    // Windows hands over the encoder's priming and padding too, and the file says how much of it to drop; the Mac's
    // reader has dropped them already (it reads the same note itself)
    #[cfg(windows)]
    let heard = formats::heard_of(path);
    #[cfg(target_os = "macos")]
    let heard: Option<formats::Heard> = None;
    // frames still to drop at the start, and how many are the song; known once the rate is
    let (mut skip, mut left) = (0u64, u64::MAX);
    let done = system_decode(path, |channels, rate, samples| {
        if current.load(Ordering::Relaxed) != seq {
            failed = Some(OpenError::Abandoned);
            return false;
        }
        let b = builder.get_or_insert_with(|| {
            if let Some(h) = heard {
                let at = |t: u64, scale: u32| ((t as u128 * rate as u128 + scale as u128 / 2) / scale as u128) as u64;
                skip = at(h.skip, h.skip_scale);
                left = if h.length > 0 { at(h.length, h.length_scale) } else { u64::MAX };
            }
            Builder::new(channels, rate, temp_dir)
        });
        // a file that changes its shape partway keeps the shape it began with
        if !b.fits(channels, rate) {
            return true;
        }
        let frames = (samples.len() / channels) as u64;
        let drop = skip.min(frames);
        skip -= drop;
        let take = (frames - drop).min(left);
        left -= take;
        if take == 0 {
            return true;
        }
        if b.push(&samples[drop as usize * channels..(drop + take) as usize * channels]).is_err() {
            failed = Some(OpenError::TooLarge);
            return false;
        }
        true
    });
    let bits = match done {
        Ok(bits) => bits,
        Err(e) => {
            if let Some(b) = builder {
                b.discard();
            }
            return Err(match e {
                #[cfg(windows)]
                SystemError::NoDecoder => OpenError::NoDecoder,
                SystemError::Damaged => OpenError::Damaged,
                SystemError::Stopped => failed.unwrap_or(OpenError::Abandoned),
            });
        }
    };
    let Some(b) = builder else { return Err(OpenError::Damaged) };
    if b.frames() == 0 {
        b.discard();
        return Err(OpenError::Damaged);
    }
    let pcm = b.finish().map_err(|_| OpenError::TooLarge)?;
    Ok(Decoded { kind: Kind::M4a, pcm, bits, words: None })
}
