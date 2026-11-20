//! Sound: mp3, flac, wav, aiff and Ogg Vorbis, each to any other; and what Audio also reads, tracker modules (MOD, XM,
//! S3M, IT), AU, VOC and WAVs in A-law or μ-law, to any of those five. The modules it uses are DotAny Audio's own, which
//! the two share in dotany-sound (`shared/sound`): what a file is (`formats`), the old sounds' readers (`oldsound`),
//! libopenmpt (`module`), the writers (`encode`, with libFLAC through `flac` and LAME through `lame`, the DLL beside the
//! program: dotany-sound-encode, `shared/sound-encode`), the rate changer (`rate`) and tags (`tags`). Each decoded block
//! goes straight to the writer instead of into a store, so a long recording never sits in memory. M4A is not read here:
//! Audio reads it through Windows, which this converter does not use.

use crate::out::Outputs;
use crate::{Options, Reason, Step, Tone};
use dotany_sound::{encode, flac, formats, module, oldsound, rate, tags};
use encode::{Encoder, Quality, shape_for};
use flac::Out;
use formats::{Kind, Sniff};
use std::io::BufWriter;
use std::path::Path;
use symphonia::core::audio::GenericAudioBufferRef;
use symphonia::core::codecs::audio::AudioDecoderOptions;
use symphonia::core::errors::Error as SymError;
use symphonia::core::formats::probe::Hint;
use symphonia::core::formats::{FormatOptions, FormatReader, TrackType};
use symphonia::core::io::{MediaSourceStream, ReadOnlySource};
use symphonia::core::meta::MetadataOptions;

// a decoder that has lost its place loses a packet or two; a file made of nothing but errors isn't audio
const MAX_BAD_PACKETS: u32 = 1000;

fn kind_of(target: &str) -> Kind {
    match target {
        "mp3" => Kind::Mp3,
        "flac" => Kind::Flac,
        "aiff" => Kind::Aiff,
        "ogg" => Kind::Ogg,
        _ => Kind::Wav,
    }
}

fn quality(t: Tone) -> Quality {
    match t {
        Tone::Normal => Quality::Std,
        Tone::High => Quality::Hi,
        Tone::Top => Quality::Top,
    }
}

/// What a file really is; M4A, Opus and the rest are refused whatever they are called.
fn sniff(p: &Path) -> Result<Kind, Reason> {
    match formats::sniff_file(p).map_err(|_| Reason::Unreadable)? {
        Sniff::Is(k) if k != Kind::M4a => Ok(k),
        // Opus, AAC, WMA and the rest, whatever the file is called; M4A is not read here
        Sniff::Refused | Sniff::Is(_) => Err(Reason::Refused(crate::Refusal::Unsupported)),
        Sniff::Damaged => Err(Reason::Damaged),
    }
}

fn reader(p: &Path, kind: Kind) -> Result<Box<dyn FormatReader>, Reason> {
    let mut hint = Hint::new();
    hint.with_extension(kind.ext());
    let file = crate::open(p)?;
    let mss = MediaSourceStream::new(Box::new(file), Default::default());
    match symphonia::default::get_probe().probe(&hint, mss, FormatOptions::default(), MetadataOptions::default()) {
        Ok(f) => Ok(f),
        // an AIFF cut off partway fails when the reader looks past the end of its sound; told it can't look ahead, it just reads
        Err(_) if kind == Kind::Aiff => {
            let file = crate::open(p)?;
            let mss = MediaSourceStream::new(Box::new(ReadOnlySource::new(file)), Default::default());
            symphonia::default::get_probe()
                .probe(&hint, mss, FormatOptions::default(), MetadataOptions::default())
                .map_err(|_| Reason::Damaged)
        }
        Err(_) => Err(Reason::Damaged),
    }
}

/// More channels than the format holds are folded into the ones it has.
fn mix_to(planar: &[Vec<f32>], n: usize, to: usize) -> Vec<Vec<f32>> {
    let from = planar.len();
    if from <= to {
        return planar.iter().map(|p| p[..n].to_vec()).collect();
    }
    let mut out = vec![vec![0f32; n]; to];
    let k = to as f32 / from as f32;
    for (c, p) in planar.iter().enumerate() {
        let o = &mut out[c % to];
        for f in 0..n {
            o[f] += p[f] * k;
        }
    }
    out
}

/// The writer, and what stands between it and the decoder: folding channels, changing the rate.
struct Pipe {
    enc: Encoder,
    rate: Option<rate::Rate>,
    channels: usize,
    written: u64,
    limit: u64,
}

impl Pipe {
    fn push(&mut self, planar: &[Vec<f32>], n: usize) -> Result<(), String> {
        let mixed;
        let part: &[Vec<f32>] = if planar.len() > self.channels {
            mixed = mix_to(planar, n, self.channels);
            &mixed
        } else {
            planar
        };
        self.written += n as u64;
        if self.written > self.limit {
            return Err("too large".into());
        }
        let enc = &mut self.enc;
        match &mut self.rate {
            None => enc.write(part, n),
            Some(r) => r.push(part, n, &mut |o, m| enc.write(o, m)),
        }
    }
    fn finish(mut self) -> Result<(), String> {
        if let Some(r) = &mut self.rate {
            let enc = &mut self.enc;
            r.finish(&mut |o, m| enc.write(o, m))?;
        }
        self.enc.finish()
    }
}

/// Where the decoded blocks go, from whichever reader: the first block decides the writer's shape.
struct Feed<'a> {
    kind: Kind,
    target: &'a str,
    opts: &'a Options,
    bits: Option<u32>,
    total: u64,
    pipe: Option<Pipe>,
    shape_of_source: (usize, u32),
    planar: Vec<Vec<f32>>,
    frames: u64,
    bad: u32,
}

impl<'a> Feed<'a> {
    fn new(kind: Kind, target: &'a str, opts: &'a Options, bits: Option<u32>, total: u64) -> Feed<'a> {
        Feed {
            kind,
            target,
            opts,
            bits,
            total,
            pipe: None,
            shape_of_source: (0, 0),
            planar: Vec::new(),
            frames: 0,
            bad: 0,
        }
    }

    fn push(
        &mut self,
        out: &mut Outputs,
        channels: usize,
        rate: u32,
        interleaved: &[f32],
        step: Step,
    ) -> Result<(), Reason> {
        if channels == 0 || rate == 0 {
            return Err(Reason::Damaged);
        }
        let kind = self.kind;
        if self.pipe.is_none() {
            self.shape_of_source = (channels, rate);
            let shape = shape_for(kind, rate, channels, self.bits);
            let file = out.file(self.target)?;
            let sink: Box<dyn Out> = Box::new(BufWriter::with_capacity(1 << 20, file));
            let estimate = rate::frames_after(self.total, rate, shape.rate);
            let enc = Encoder::new(
                kind,
                sink,
                &shape,
                if kind == Kind::Mp3 { quality(self.opts.tone) } else { Quality::Hi },
                estimate,
            )
            .map_err(|e| {
                if e.contains("libmp3lame") {
                    Reason::Missing(dotany_sound::lame::FILE.into())
                } else {
                    Reason::TooLarge
                }
            })?;
            let resample = if shape.rate != rate {
                Some(rate::Rate::new(rate, shape.rate, shape.channels).map_err(|_| Reason::Damaged)?)
            } else {
                None
            };
            // WAV and AIFF count their bytes in 32 bits
            let limit = match kind {
                Kind::Wav | Kind::Aiff => {
                    (u32::MAX as u64 - 1024) / (shape.channels as u64 * shape.bits as u64 / 8) * rate as u64
                        / shape.rate.max(1) as u64
                }
                _ => u64::MAX,
            };
            self.pipe = Some(Pipe { enc, rate: resample, channels: shape.channels, written: 0, limit });
        }
        // a file that changes its shape partway keeps the shape it began with
        if self.shape_of_source != (channels, rate) {
            self.bad += 1;
            return Ok(());
        }
        let n = interleaved.len() / channels;
        self.planar.resize(channels, Vec::new());
        for (c, plane) in self.planar.iter_mut().enumerate() {
            plane.clear();
            plane.extend(interleaved.iter().skip(c).step_by(channels));
        }
        self.pipe.as_mut().unwrap().push(&self.planar, n).map_err(|_| Reason::TooLarge)?;
        self.frames += n as u64;
        if self.total > 0 {
            step(0.95 * (self.frames as f32 / self.total as f32).min(1.0));
        }
        Ok(())
    }

    fn finish(self) -> Result<(), Reason> {
        let Some(pipe) = self.pipe else { return Err(Reason::Damaged) };
        if self.frames == 0 {
            return Err(Reason::Damaged);
        }
        pipe.finish().map_err(|_| Reason::Unwritable)
    }
}

pub fn convert(p: &Path, target: &str, opts: &Options, out: &mut Outputs, step: Step) -> Result<(), Reason> {
    let source = sniff(p)?;
    let kind = kind_of(target);
    let carry = if source.module() {
        from_module(p, source, kind, target, opts, out, step)?
    } else if is_old(p, source) {
        from_old(p, source, kind, target, opts, out, step)?;
        tags::read(p).1
    } else {
        from_symphonia(p, source, kind, target, opts, out, step)?;
        tags::read(p).1
    };
    // the source's tags (title, artist, album, cover) go across
    if let (Some(tag), Some(temp)) = (carry.as_ref(), out.last_temp()) {
        let _ = tags::write(temp, kind, Some(tag), &tags::Edit::default(), None);
    }
    step(1.0);
    Ok(())
}

fn from_symphonia(
    p: &Path,
    source: Kind,
    kind: Kind,
    target: &str,
    opts: &Options,
    out: &mut Outputs,
    step: Step,
) -> Result<(), Reason> {
    let mut format = reader(p, source)?;
    let track = format.default_track(TrackType::Audio).ok_or(Reason::Damaged)?;
    let track_id = track.id;
    let params = track.codec_params.as_ref().and_then(|c| c.audio()).ok_or(Reason::Damaged)?.clone();
    let total = track.num_frames.unwrap_or(0);
    let bits = if source.lossy() { None } else { params.bits_per_sample.or(params.bits_per_coded_sample) };
    let mut decoder = symphonia::default::get_codecs()
        .make_audio_decoder(&params, &AudioDecoderOptions::default())
        .map_err(|_| Reason::Damaged)?;
    let mut feed = Feed::new(kind, target, opts, bits, total);
    let mut scratch: Vec<f32> = Vec::new();
    loop {
        let packet = match format.next_packet() {
            Ok(Some(p)) => p,
            Ok(None) => break,
            // a cut-off file ends here; what came before is still sound
            Err(_) => break,
        };
        if packet.track_id != track_id {
            continue;
        }
        let buf = match decoder.decode(&packet) {
            Ok(buf) => buf,
            Err(SymError::DecodeError(_)) | Err(SymError::IoError(_)) => {
                feed.bad += 1;
                if feed.bad > MAX_BAD_PACKETS {
                    return Err(Reason::Damaged);
                }
                continue;
            }
            Err(_) => break,
        };
        let spec = buf.spec();
        let (channels, rate) = (spec.channels().count(), spec.rate());
        copy_out(&buf, &mut scratch);
        feed.push(out, channels, rate, &scratch, step)?;
    }
    feed.finish()
}

/// A tracker module, played once through by libopenmpt; its title and artist go across.
fn from_module(
    p: &Path,
    source: Kind,
    kind: Kind,
    target: &str,
    opts: &Options,
    out: &mut Outputs,
    step: Step,
) -> Result<Option<lofty::tag::Tag>, Reason> {
    use lofty::tag::{Accessor, Tag, TagType};
    use module::{MAX_FILE, MAX_SECONDS, ModError, Module, RATE};
    if std::fs::metadata(p).map_err(|_| Reason::Unreadable)?.len() > MAX_FILE {
        return Err(Reason::TooLarge);
    }
    let mut m = Module::load(crate::read_all(p)?, source).map_err(|e| {
        if e == ModError::Refused { Reason::Refused(crate::Refusal::Unsupported) } else { Reason::Damaged }
    })?;
    let mut tag = Tag::new(TagType::Id3v2);
    let (title, artist) = (m.meta("title"), m.meta("artist"));
    if !title.is_empty() {
        tag.set_title(title);
    }
    if !artist.is_empty() {
        tag.set_artist(artist);
    }
    let total = (m.seconds().min(MAX_SECONDS) * RATE as f64) as u64;
    let mut feed = Feed::new(kind, target, opts, None, total);
    let mut failed = None;
    let _ = m.render(|samples| match feed.push(out, 2, RATE, samples, step) {
        Ok(()) => true,
        Err(e) => {
            failed = Some(e);
            false
        }
    });
    if let Some(e) = failed {
        return Err(e);
    }
    feed.finish()?;
    Ok((!lofty::tag::TagExt::is_empty(&tag)).then_some(tag))
}

/// AU, VOC or a WAV in A-law or μ-law: the readers in `oldsound` take these.
fn is_old(p: &Path, source: Kind) -> bool {
    match source {
        Kind::Au | Kind::Voc => true,
        Kind::Wav => {
            use std::io::Read;
            let mut head = Vec::new();
            std::fs::File::open(p).and_then(|f| f.take(256 * 1024).read_to_end(&mut head)).is_ok()
                && oldsound::is_g711_wav(&head)
        }
        _ => false,
    }
}

fn old_parts(map: &[u8], source: Kind) -> Result<Vec<oldsound::Part>, Reason> {
    let bad = |e: oldsound::Bad| {
        if e == oldsound::Bad::Refused { Reason::Refused(crate::Refusal::Unsupported) } else { Reason::Damaged }
    };
    match source {
        Kind::Au => Ok(vec![oldsound::au(map, map.len()).map_err(bad)?]),
        Kind::Wav => Ok(vec![oldsound::wav(map).map_err(bad)?]),
        _ => oldsound::voc(map).map_err(bad),
    }
}

// how many frames the parts make, at the first part's rate
fn old_frames(parts: &[oldsound::Part], rate: u32) -> u64 {
    parts
        .iter()
        .map(|part| match part {
            oldsound::Part::Sound(s, r) => (r.len() / (s.enc.bytes() * s.channels)) as u64,
            oldsound::Part::Silence(secs) => (secs * rate as f64).round() as u64,
        })
        .sum()
}

fn from_old(
    p: &Path,
    source: Kind,
    kind: Kind,
    target: &str,
    opts: &Options,
    out: &mut Outputs,
    step: Step,
) -> Result<(), Reason> {
    use oldsound::Part;
    let file = crate::open(p)?;
    // the file is mapped, never changed, and read a stretch at a time
    let map = unsafe { memmap2::Mmap::map(&file) }.map_err(|_| Reason::Damaged)?;
    let parts = old_parts(&map, source)?;
    let first = oldsound::first_shape(&parts).ok_or(Reason::Damaged)?;
    let mut feed = Feed::new(kind, target, opts, Some(first.enc.bits()), old_frames(&parts, first.rate));
    let mut scratch = Vec::new();
    for part in &parts {
        match part {
            Part::Sound(shape, range) => {
                let frame = shape.enc.bytes() * shape.channels;
                for chunk in map[range.clone()].chunks(((1 << 20) / frame).max(1) * frame) {
                    oldsound::decode(shape, chunk, &mut scratch);
                    feed.push(out, shape.channels, shape.rate, &scratch, step)?;
                }
            }
            Part::Silence(seconds) => {
                let mut left = (seconds * first.rate as f64).round() as usize;
                while left > 0 {
                    let n = left.min(1 << 16);
                    scratch.clear();
                    scratch.resize(n * first.channels, 0.0);
                    feed.push(out, first.channels, first.rate, &scratch, step)?;
                    left -= n;
                }
            }
        }
    }
    feed.finish()
}

fn copy_out(buf: &GenericAudioBufferRef<'_>, out: &mut Vec<f32>) {
    out.resize(buf.samples_interleaved(), 0.0);
    buf.copy_to_slice_interleaved(&mut out[..]);
}

/// What the sound is, from the file's header: length, rate, channels, bits. Quick: nothing is decoded.
struct Heard {
    secs: f64,
    rate: u32,
    channels: usize,
    bits: Option<u32>,
}

fn heard(p: &Path) -> Option<Heard> {
    let source = sniff(p).ok()?;
    if source.module() {
        // the whole module is read to know its length; it's small, and only the sound is long
        use module::{MAX_FILE, MAX_SECONDS, Module, RATE};
        if std::fs::metadata(p).ok()?.len() > MAX_FILE {
            return None;
        }
        let m = Module::load(std::fs::read(p).ok()?, source).ok()?;
        return Some(Heard { secs: m.seconds().min(MAX_SECONDS), rate: RATE, channels: 2, bits: None });
    }
    if is_old(p, source) {
        let file = std::fs::File::open(p).ok()?;
        let map = unsafe { memmap2::Mmap::map(&file) }.ok()?;
        let parts = old_parts(&map, source).ok()?;
        let first = oldsound::first_shape(&parts)?;
        let secs = old_frames(&parts, first.rate) as f64 / first.rate as f64;
        return Some(Heard { secs, rate: first.rate, channels: first.channels, bits: Some(first.enc.bits()) });
    }
    let format = reader(p, source).ok()?;
    let track = format.default_track(TrackType::Audio)?;
    let params = track.codec_params.as_ref()?.audio()?.clone();
    let rate = params.sample_rate?;
    let channels = params.channels.as_ref().map(|c| c.count()).unwrap_or(2);
    let bits = if source.lossy() { None } else { params.bits_per_sample.or(params.bits_per_coded_sample) };
    let secs = match track.num_frames {
        Some(n) if n > 0 => n as f64 / rate as f64,
        _ => tags::duration_ms(p)? as f64 / 1000.0,
    };
    Some(Heard { secs, rate, channels, bits })
}

pub fn estimate(p: &Path, size: u64, target: &str, opts: &Options) -> u64 {
    let Some(h) = heard(p) else { return size };
    let kind = kind_of(target);
    let shape = shape_for(kind, h.rate, h.channels, h.bits);
    let pcm = h.secs * shape.rate as f64 * shape.channels as f64 * (shape.bits / 8) as f64;
    // the tags and cover go across as they are
    let tags = 4096.0;
    let bytes = match kind {
        Kind::Wav => 44.0 + pcm,
        Kind::Aiff => 54.0 + pcm,
        // lossless packing of music lands near 55–60 % of the plain samples
        Kind::Flac => pcm * 0.58 + tags,
        Kind::Mp3 => encode::kbps(Kind::Mp3, quality(opts.tone)) as f64 * 125.0 * h.secs + tags,
        _ => encode::kbps(Kind::Ogg, Quality::Hi) as f64 * 125.0 * h.secs + tags,
    };
    bytes as u64
}
