use crate::base64;
use sdk::json::Json;
use std::sync::{Arc, LazyLock, OnceLock};

pub const SAMPLE_RATE: u32 = 48_000;
const CHANNELS: u16 = 2;
const BITS_PER_SAMPLE: u16 = 24;
const BYTES_PER_FRAME: usize = 6;
const HEADER_BYTES: usize = 68;
const FORMAT_EXTENSIBLE: u16 = 0xFFFE;
const EXTENSION_BYTES: u16 = 22;
const FRONT_LEFT_AND_RIGHT: u32 = 0x3;
const PCM_GUID: [u8; 16] = [0x01, 0x00, 0x00, 0x00, 0x00, 0x00, 0x10, 0x00, 0x80, 0x00, 0x00, 0xAA, 0x00, 0x38, 0x9B, 0x71];
const SCALE: f32 = 8_388_608.0;
const HIGHEST: f32 = 1.0 - 1.0 / SCALE;
const LOWEST: f32 = -1.0;
pub const MAX_FRAMES: usize = (u32::MAX as usize - (HEADER_BYTES - 8)) / BYTES_PER_FRAME;

pub struct Wav {
    bytes: Vec<u8>,
}

impl Wav {
    pub fn new(frames: usize) -> Wav {
        Wav::at(SAMPLE_RATE, frames)
    }

    pub fn at(rate: u32, frames: usize) -> Wav {
        let mut bytes = Vec::with_capacity(HEADER_BYTES + frames.min(MAX_FRAMES) * BYTES_PER_FRAME);
        bytes.extend_from_slice(b"RIFF");
        bytes.extend_from_slice(&0u32.to_le_bytes());
        bytes.extend_from_slice(b"WAVE");
        bytes.extend_from_slice(b"fmt ");
        bytes.extend_from_slice(&40u32.to_le_bytes());
        bytes.extend_from_slice(&FORMAT_EXTENSIBLE.to_le_bytes());
        bytes.extend_from_slice(&CHANNELS.to_le_bytes());
        bytes.extend_from_slice(&rate.to_le_bytes());
        bytes.extend_from_slice(&(rate * BYTES_PER_FRAME as u32).to_le_bytes());
        bytes.extend_from_slice(&(BYTES_PER_FRAME as u16).to_le_bytes());
        bytes.extend_from_slice(&BITS_PER_SAMPLE.to_le_bytes());
        bytes.extend_from_slice(&EXTENSION_BYTES.to_le_bytes());
        bytes.extend_from_slice(&BITS_PER_SAMPLE.to_le_bytes());
        bytes.extend_from_slice(&FRONT_LEFT_AND_RIGHT.to_le_bytes());
        bytes.extend_from_slice(&PCM_GUID);
        bytes.extend_from_slice(b"data");
        bytes.extend_from_slice(&0u32.to_le_bytes());
        Wav { bytes }
    }

    pub fn append(&mut self, left: &[f32], right: &[f32]) {
        for (&l, &r) in left.iter().zip(right) {
            for sample in [l, r] {
                let value = (sample.clamp(LOWEST, HIGHEST) * SCALE).round() as i32;
                self.bytes.extend_from_slice(&value.to_le_bytes()[..3]);
            }
        }
    }

    pub fn finish(mut self) -> Vec<u8> {
        let data = (self.bytes.len() - HEADER_BYTES) as u32;
        self.bytes[4..8].copy_from_slice(&(HEADER_BYTES as u32 - 8 + data).to_le_bytes());
        self.bytes[64..68].copy_from_slice(&data.to_le_bytes());
        self.bytes
    }
}

const FORMAT_PCM: u16 = 1;
const FORMAT_FLOAT: u16 = 3;
const RATE_LEAST: u32 = 8_000;
const RATE_MOST: u32 = 384_000;
pub const LONGEST_SECONDS: usize = 600;
const PEAK_SCALE: u32 = 32_768;
const SCALE_16: f32 = 32_768.0;
pub const FINE: usize = 64;
pub const COARSE: usize = 4096;
const ZEROS: usize = 32;
const PER_ZERO: usize = 512;
const BETA: f64 = 12.0;
const LIKENESS_STEPS: usize = 16;
const LIKENESS_LEVEL: f64 = 0.1;
const LIKENESS_OCTAVES: f64 = 1.0;
static KERNEL: LazyLock<Vec<f32>> = LazyLock::new(kernel);

fn bessel_i0(x: f64) -> f64 {
    let (mut sum, mut term, mut k) = (1.0, 1.0, 1.0);
    while term > 1e-12 * sum {
        term *= (x / (2.0 * k)).powi(2);
        sum += term;
        k += 1.0;
    }
    sum
}

fn kernel() -> Vec<f32> {
    let peak = bessel_i0(BETA);
    (0..=ZEROS * PER_ZERO + 1)
        .map(|i| {
            let t = i as f64 / PER_ZERO as f64;
            if t >= ZEROS as f64 {
                return 0.0;
            }
            let x = std::f64::consts::PI * t;
            let sinc = if i == 0 { 1.0 } else { x.sin() / x };
            (sinc * bessel_i0(BETA * (1.0 - (t / ZEROS as f64).powi(2)).sqrt()) / peak) as f32
        })
        .collect()
}

pub struct Likeness {
    levels: [f64; LIKENESS_STEPS],
    hz: f64,
}

impl Likeness {
    pub fn alike(&self, other: &Likeness) -> bool {
        let apart = self.levels.iter().zip(&other.levels).map(|(mine, theirs)| (mine - theirs).abs()).sum::<f64>() / LIKENESS_STEPS as f64;
        let octaves = match (self.hz > 0.0, other.hz > 0.0) {
            (true, true) => (self.hz / other.hz).log2().abs(),
            (false, false) => 0.0,
            _ => f64::INFINITY,
        };
        apart <= LIKENESS_LEVEL && octaves <= LIKENESS_OCTAVES
    }
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Refusal {
    Unreadable,
    TooLong,
}

pub struct Wave {
    pub name: String,
    pub path: Option<String>,
    pub rate: f64,
    pub channels: usize,
    pub pcm: Vec<i16>,
    pub fine: Vec<u8>,
    pub coarse: Vec<u8>,
    written: OnceLock<(usize, Arc<str>)>,
}

struct Format {
    tag: u16,
    channels: usize,
    rate: u32,
    bits: u16,
}

fn u16_at(bytes: &[u8], at: usize) -> Option<u16> {
    Some(u16::from_le_bytes([*bytes.get(at)?, *bytes.get(at + 1)?]))
}

fn u32_at(bytes: &[u8], at: usize) -> Option<u32> {
    Some(u32::from_le_bytes([*bytes.get(at)?, *bytes.get(at + 1)?, *bytes.get(at + 2)?, *bytes.get(at + 3)?]))
}

impl Format {
    fn parse(body: &[u8]) -> Option<Format> {
        let mut tag = u16_at(body, 0)?;
        if tag == FORMAT_EXTENSIBLE {
            tag = u16_at(body, 24)?;
        }
        let channels = usize::from(u16_at(body, 2)?);
        let rate = u32_at(body, 4)?;
        let bits = u16_at(body, 14)?;
        (channels > 0 && (RATE_LEAST..=RATE_MOST).contains(&rate)).then_some(Format { tag, channels, rate, bits })
    }

    fn decoder(&self) -> Option<fn(&[u8]) -> i16> {
        Some(match (self.tag, self.bits) {
            (FORMAT_PCM, 8) => |b| (i16::from(b[0]) - 128) << 8,
            (FORMAT_PCM, 16) => |b| i16::from_le_bytes([b[0], b[1]]),
            (FORMAT_PCM, 24) => |b| i16::from_le_bytes([b[1], b[2]]),
            (FORMAT_PCM, 32) => |b| i16::from_le_bytes([b[2], b[3]]),
            (FORMAT_FLOAT, 32) => |b| (f32::from_le_bytes([b[0], b[1], b[2], b[3]]).clamp(-1.0, 1.0) * (SCALE_16 - 1.0)).round() as i16,
            (FORMAT_FLOAT, 64) => |b| (f64::from_le_bytes([b[0], b[1], b[2], b[3], b[4], b[5], b[6], b[7]]).clamp(-1.0, 1.0) * f64::from(SCALE_16 - 1.0)).round() as i16,
            _ => return None,
        })
    }
}

fn loudest(pcm: &[i16]) -> u8 {
    let peak = pcm.iter().map(|sample| sample.unsigned_abs()).max().unwrap_or(0);
    ((u32::from(peak) * 255 + PEAK_SCALE / 2) / PEAK_SCALE).min(255) as u8
}

pub fn quantized(sample: f32) -> i16 {
    (sample * SCALE_16).round().clamp(f32::from(i16::MIN), f32::from(i16::MAX)) as i16
}

impl Wave {
    pub fn missing(name: String, path: Option<String>) -> Wave {
        Wave { name, path, rate: f64::from(SAMPLE_RATE), channels: 1, pcm: Vec::new(), fine: Vec::new(), coarse: Vec::new(), written: OnceLock::new() }
    }

    pub fn renamed(&self, name: &str) -> Wave {
        Wave { name: String::from(name), path: self.path.clone(), rate: self.rate, channels: self.channels, pcm: self.pcm.clone(), fine: self.fine.clone(), coarse: self.coarse.clone(), written: OnceLock::new() }
    }

    pub fn sound(name: String, rate: f64, channels: usize, pcm: Vec<i16>) -> Wave {
        let fine: Vec<u8> = pcm.chunks(FINE * channels).map(loudest).collect();
        let coarse = fine.chunks(COARSE / FINE).map(|run| run.iter().copied().max().unwrap_or(0)).collect();
        LazyLock::force(&KERNEL);
        Wave { name, path: None, rate, channels, pcm, fine, coarse, written: OnceLock::new() }
    }

    fn header(&self) -> Vec<(String, Json)> {
        vec![(String::from("name"), Json::String(self.name.clone())), (String::from("sampleRateHz"), Json::Number(self.rate)), (String::from("channels"), Json::Number(self.channels as f64))]
    }

    fn sourced(&self, fields: &mut Vec<(String, Json)>) {
        if let Some(path) = &self.path {
            fields.push((String::from("sourcePath"), Json::String(path.clone())));
        }
    }

    pub fn summary(&self) -> Json {
        let mut fields = self.header();
        fields.push((String::from("seconds"), Json::Number(self.frames() as f64 / self.rate)));
        self.sourced(&mut fields);
        Json::Object(fields)
    }

    pub fn to_json(&self) -> Json {
        let mut fields = self.header();
        self.sourced(&mut fields);
        fields.push((String::from("pcm16Base64"), Json::Array(base64::lines(&self.pcm).into_iter().map(Json::String).collect())));
        Json::Object(fields)
    }

    pub fn to_json_at(&self, depth: usize) -> Json {
        let (written, text) = self.written.get_or_init(|| {
            let text = self.to_json().write_at(depth);
            (depth, Arc::from(text))
        });
        if *written == depth { Json::Raw(Arc::clone(text)) } else { self.to_json() }
    }

    pub fn from_json(json: &Json) -> Option<Wave> {
        let name = String::from(json.get("name").and_then(Json::as_str)?);
        let rate = json.get("sampleRateHz").and_then(Json::as_f64).filter(|rate| (f64::from(RATE_LEAST)..=f64::from(RATE_MOST)).contains(rate))?;
        let channels = json.get("channels").and_then(Json::as_i64).map_or(1, |channels| channels.clamp(1, 2) as usize);
        let mut pcm = base64::words(json.get("pcm16Base64").and_then(Json::as_array)?.iter().filter_map(Json::as_str))?;
        pcm.truncate(pcm.len() / channels * channels);
        if pcm.len() / channels > rate as usize * LONGEST_SECONDS {
            return None;
        }
        let mut wave = Wave::sound(name, rate, channels, pcm);
        wave.path = json.get("sourcePath").and_then(Json::as_str).map(String::from);
        Some(wave)
    }

    pub fn parse(name: String, path: String, bytes: &[u8]) -> Result<Wave, Refusal> {
        if bytes.len() < 12 || &bytes[0..4] != b"RIFF" || &bytes[8..12] != b"WAVE" {
            return Err(Refusal::Unreadable);
        }
        let (mut format, mut data) = (None, None);
        let mut at = 12;
        while let Some(size) = u32_at(bytes, at + 4) {
            let size = size as usize;
            let body = &bytes[(at + 8).min(bytes.len())..(at + 8).saturating_add(size).min(bytes.len())];
            match &bytes[at..at + 4] {
                b"fmt " => format = Format::parse(body),
                b"data" => data = Some(body),
                _ => {}
            }
            at = (at + 8).saturating_add(size).saturating_add(size & 1).min(bytes.len());
        }
        let (Some(format), Some(data)) = (format, data) else {
            return Err(Refusal::Unreadable);
        };
        let decode = format.decoder().ok_or(Refusal::Unreadable)?;
        let width = usize::from(format.bits / 8);
        let stride = width * format.channels;
        let frames = data.len() / stride;
        if frames > format.rate as usize * LONGEST_SECONDS {
            return Err(Refusal::TooLong);
        }
        let channels = format.channels.min(2);
        let mut pcm = Vec::with_capacity(frames * channels);
        for frame in data.chunks_exact(stride) {
            for channel in 0..channels {
                pcm.push(decode(&frame[channel * width..(channel + 1) * width]));
            }
        }
        let mut wave = Wave::sound(name, f64::from(format.rate), channels, pcm);
        wave.path = Some(path);
        Ok(wave)
    }

    pub fn wav(&self) -> Vec<u8> {
        let mut file = Wav::at(self.rate.round() as u32, self.frames());
        for frame in self.pcm.chunks(self.channels) {
            file.append(&[f32::from(frame[0]) / SCALE_16], &[f32::from(frame[self.channels - 1]) / SCALE_16]);
        }
        file.finish()
    }

    pub fn reversed(self) -> Wave {
        let pcm: Vec<i16> = self.pcm.chunks(self.channels).rev().flatten().copied().collect();
        Wave::sound(self.name, self.rate, self.channels, pcm)
    }

    pub fn frames(&self) -> usize {
        self.pcm.len() / self.channels
    }

    pub fn mono(&self, frame: usize) -> f64 {
        let at = frame * self.channels;
        let left = f64::from(self.pcm[at]);
        if self.channels == 2 { (left + f64::from(self.pcm[at + 1])) * 0.5 } else { left }
    }

    pub fn likeness(&self, from: f64, seconds: f64) -> Likeness {
        let start = ((from.max(0.0) * self.rate) as usize).min(self.frames());
        let length = ((seconds.max(0.0) * self.rate) as usize).min(self.frames() - start);
        let mut levels = [0.0f64; LIKENESS_STEPS];
        let (mut power, mut slope, mut last) = (0.0, 0.0, 0.0);
        for at in 0..length {
            let sample = self.mono(start + at);
            let level = &mut levels[at * LIKENESS_STEPS / length];
            *level = level.max(sample.abs());
            power += sample * sample;
            slope += (sample - last) * (sample - last);
            last = sample;
        }
        let loudest = levels.iter().copied().fold(0.0, f64::max);
        if loudest > 0.0 {
            levels = levels.map(|level| level / loudest);
        }
        let hz = if power > 0.0 { self.rate / std::f64::consts::PI * (slope / (4.0 * power)).sqrt().min(1.0).asin() } else { 0.0 };
        Likeness { levels, hz }
    }

    pub fn same_sound(&self, other: &Wave) -> bool {
        self.rate == other.rate && self.channels == other.channels && self.pcm == other.pcm
    }

    pub fn seconds(&self) -> f64 {
        self.frames() as f64 / self.rate
    }

    pub fn at(&self, frame: f64, step: f64) -> (f32, f32) {
        let stretch = step.max(1.0);
        let reach = ZEROS as f64 * stretch;
        let first = ((frame - reach).floor() as i64 + 1).max(0);
        let last = ((frame + reach).ceil() as i64 - 1).min(self.frames() as i64 - 1);
        if first > last {
            return (0.0, 0.0);
        }
        let table = KERNEL.as_slice();
        let per = PER_ZERO as f64 / stretch;
        let (mut left, mut right) = (0.0f32, 0.0f32);
        for k in first..=last {
            let t = (frame - k as f64).abs() * per;
            let i = t as usize;
            let weight = table[i] + (table[i + 1] - table[i]) * (t - i as f64) as f32;
            let at = k as usize * self.channels;
            let l = f32::from(self.pcm[at]);
            let r = if self.channels == 2 { f32::from(self.pcm[at + 1]) } else { l };
            left += l * weight;
            right += r * weight;
        }
        let gain = 1.0 / (SCALE_16 * stretch as f32);
        (left * gain, right * gain)
    }

    pub fn peak(&self, from: f64, to: f64) -> u8 {
        if to <= 0.0 || from >= self.frames() as f64 || to <= from {
            return 0;
        }
        if to - from < FINE as f64 {
            let first = (from.max(0.0) as usize).min(self.frames());
            let last = (to.ceil() as usize).clamp(first, self.frames());
            return loudest(&self.pcm[first * self.channels..last * self.channels]);
        }
        let (rung, bucket) = if to - from >= COARSE as f64 { (&self.coarse, COARSE) } else { (&self.fine, FINE) };
        let last = ((to / bucket as f64).ceil() as usize).min(rung.len());
        let first = ((from.max(0.0) / bucket as f64) as usize).min(last);
        rung[first..last].iter().copied().max().unwrap_or(0)
    }
}
