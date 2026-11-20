//! Changing the sample rate, a block at a time, for the speakers and for formats that can't hold the source's rate.

use audioadapter_buffers::direct::SequentialSliceOfVecs;
use rubato::{Fft, FixedSync, Resampler};

const CHUNK: usize = 1024;

pub struct Rate {
    inner: Fft<f32>,
    channels: usize,
    waiting: Vec<Vec<f32>>,
    output: Vec<Vec<f32>>,
    skip: usize,
    fresh_skip: usize,
    ratio: f64,
    taken: u64,
    given: u64,
}

impl Rate {
    pub fn new(from: u32, to: u32, channels: usize) -> Result<Self, String> {
        let inner = Fft::<f32>::new(from as usize, to as usize, CHUNK, channels, FixedSync::Input)
            .map_err(|e| e.to_string())?;
        let skip = inner.output_delay();
        let room = inner.output_frames_max();
        Ok(Rate {
            inner,
            channels,
            waiting: vec![Vec::new(); channels],
            output: vec![vec![0.0; room]; channels],
            skip,
            fresh_skip: skip,
            ratio: to as f64 / from as f64,
            taken: 0,
            given: 0,
        })
    }

    /// Forget everything, as after jumping to another place in the song.
    pub fn reset(&mut self) {
        self.inner.reset();
        self.skip = self.fresh_skip;
        self.waiting.iter_mut().for_each(Vec::clear);
        self.taken = 0;
        self.given = 0;
    }

    fn run(&mut self, emit: &mut dyn FnMut(&[Vec<f32>], usize) -> Result<(), String>) -> Result<(), String> {
        loop {
            let need = self.inner.input_frames_next();
            if self.waiting[0].len() < need {
                return Ok(());
            }
            let chunk: Vec<Vec<f32>> = self.waiting.iter().map(|w| w[..need].to_vec()).collect();
            let frames_out = self.inner.output_frames_next();
            let (_, wrote) = {
                let input = SequentialSliceOfVecs::new(&chunk, self.channels, need).map_err(|e| e.to_string())?;
                let room = frames_out.max(self.output[0].len());
                let mut output =
                    SequentialSliceOfVecs::new_mut(&mut self.output, self.channels, room).map_err(|e| e.to_string())?;
                self.inner.process_into_buffer(&input, &mut output, None).map_err(|e| e.to_string())?
            };
            self.waiting.iter_mut().for_each(|w| {
                w.drain(..need);
            });
            let drop = self.skip.min(wrote);
            self.skip -= drop;
            if wrote > drop {
                let part: Vec<Vec<f32>> = self.output.iter().map(|o| o[drop..wrote].to_vec()).collect();
                self.given += (wrote - drop) as u64;
                emit(&part, wrote - drop)?;
            }
        }
    }

    pub fn push(
        &mut self,
        planar: &[Vec<f32>],
        n: usize,
        emit: &mut dyn FnMut(&[Vec<f32>], usize) -> Result<(), String>,
    ) -> Result<(), String> {
        for (w, p) in self.waiting.iter_mut().zip(planar) {
            w.extend_from_slice(&p[..n]);
        }
        self.taken += n as u64;
        self.run(emit)
    }

    /// The end of the sound: pushes out what's held back, and exactly as many frames as the length calls for.
    pub fn finish(&mut self, emit: &mut dyn FnMut(&[Vec<f32>], usize) -> Result<(), String>) -> Result<(), String> {
        let target = (self.taken as f64 * self.ratio).round() as u64;
        while self.given < target {
            let pad = self.inner.input_frames_next();
            let zeros = vec![vec![0.0; pad]; self.channels];
            let (mut given, target_now) = (self.given, target);
            let mut limited = |p: &[Vec<f32>], n: usize| -> Result<(), String> {
                let left = (target_now - given).min(n as u64) as usize;
                given += left as u64;
                if left > 0 {
                    emit(p, left)?;
                }
                Ok(())
            };
            for (w, z) in self.waiting.iter_mut().zip(&zeros) {
                w.extend_from_slice(z);
            }
            self.run(&mut limited)?;
            self.given = given;
        }
        Ok(())
    }
}

pub fn frames_after(frames: u64, from: u32, to: u32) -> u64 {
    (frames as f64 * to as f64 / from as f64).round() as u64
}
