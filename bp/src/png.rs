use sdk::draw::{Fill, Theme};

const SIGNATURE: [u8; 8] = [0x89, b'P', b'N', b'G', b'\r', b'\n', 0x1A, b'\n'];
const CHANNELS: usize = 3;
const BIT_DEPTH: u8 = 8;
const COLOR_RGB: u8 = 2;
const FILTER_UP: u8 = 2;
const ZLIB_HEADER: [u8; 2] = [0x78, 0x01];
const ADLER_MODULUS: u32 = 65521;
const ADLER_RUN: usize = 5552;
const WINDOW: usize = 1 << 15;
const HASH_BITS: u32 = 16;
const MATCH_MIN: usize = 3;
const MATCH_MAX: usize = 258;
const CHAIN: usize = 16;
const END_OF_BLOCK: u32 = 256;
const NONE: u32 = u32::MAX;
const CRC_TABLE: [u32; 256] = crc_table();

pub struct Png {
    width: usize,
    height: usize,
    scale: usize,
    grey: Vec<u8>,
}

impl Png {
    pub fn new(width: i32, height: i32, scale: i32) -> Png {
        let scale = scale.max(1) as usize;
        let (width, height) = (width.max(0) as usize * scale, height.max(0) as usize * scale);
        Png { width, height, scale, grey: vec![0; width * height] }
    }

    pub fn paint(&mut self, fills: &[Fill], theme: Theme) {
        for fill in fills {
            let level = (theme.rgba(*fill)[0] * 255.0).round() as u8;
            let rect = fill.rect;
            let (left, right) = (self.edge(rect.x, self.width), self.edge(rect.x + rect.w, self.width));
            let (top, bottom) = (self.edge(rect.y, self.height), self.edge(rect.y + rect.h, self.height));
            for row in top..bottom {
                self.grey[row * self.width + left..row * self.width + right].fill(level);
            }
        }
    }

    fn edge(&self, at: i32, limit: usize) -> usize {
        (i64::from(at) * self.scale as i64).clamp(0, limit as i64) as usize
    }

    pub fn finish(self) -> Vec<u8> {
        let mut raster = Vec::with_capacity(self.height * (1 + self.width * CHANNELS));
        for row in 0..self.height {
            raster.push(FILTER_UP);
            for column in 0..self.width {
                let level = self.grey[row * self.width + column];
                let above = if row > 0 { self.grey[(row - 1) * self.width + column] } else { 0 };
                raster.extend_from_slice(&[level.wrapping_sub(above); CHANNELS]);
            }
        }
        let mut header = Vec::with_capacity(13);
        header.extend_from_slice(&(self.width as u32).to_be_bytes());
        header.extend_from_slice(&(self.height as u32).to_be_bytes());
        header.extend_from_slice(&[BIT_DEPTH, COLOR_RGB, 0, 0, 0]);
        let mut file = SIGNATURE.to_vec();
        chunk(&mut file, b"IHDR", &header);
        chunk(&mut file, b"IDAT", &zlib(&raster));
        chunk(&mut file, b"IEND", &[]);
        file
    }
}

fn chunk(file: &mut Vec<u8>, kind: &[u8; 4], data: &[u8]) {
    file.extend_from_slice(&(data.len() as u32).to_be_bytes());
    let start = file.len();
    file.extend_from_slice(kind);
    file.extend_from_slice(data);
    let crc = crc32(&file[start..]);
    file.extend_from_slice(&crc.to_be_bytes());
}

const fn crc_table() -> [u32; 256] {
    let mut table = [0u32; 256];
    let mut n = 0;
    while n < 256 {
        let mut crc = n as u32;
        let mut bit = 0;
        while bit < 8 {
            crc = if crc & 1 != 0 { 0xEDB8_8320 ^ (crc >> 1) } else { crc >> 1 };
            bit += 1;
        }
        table[n] = crc;
        n += 1;
    }
    table
}

fn crc32(bytes: &[u8]) -> u32 {
    !bytes.iter().fold(!0u32, |crc, &byte| CRC_TABLE[((crc ^ u32::from(byte)) & 0xFF) as usize] ^ (crc >> 8))
}

fn adler32(bytes: &[u8]) -> u32 {
    let (mut a, mut b) = (1u32, 0u32);
    for run in bytes.chunks(ADLER_RUN) {
        for &byte in run {
            a += u32::from(byte);
            b += a;
        }
        a %= ADLER_MODULUS;
        b %= ADLER_MODULUS;
    }
    (b << 16) | a
}

fn zlib(data: &[u8]) -> Vec<u8> {
    let mut stream = ZLIB_HEADER.to_vec();
    stream.extend(deflate(data));
    stream.extend_from_slice(&adler32(data).to_be_bytes());
    stream
}

struct Bits {
    bytes: Vec<u8>,
    held: u64,
    count: u32,
}

impl Bits {
    fn put(&mut self, value: u32, count: u32) {
        self.held |= u64::from(value) << self.count;
        self.count += count;
        while self.count >= 8 {
            self.bytes.push(self.held as u8);
            self.held >>= 8;
            self.count -= 8;
        }
    }

    fn huffman(&mut self, code: u32, count: u32) {
        self.put(code.reverse_bits() >> (32 - count), count);
    }

    fn symbol(&mut self, symbol: u32) {
        match symbol {
            0..=143 => self.huffman(0x30 + symbol, 8),
            144..=255 => self.huffman(0x190 + symbol - 144, 9),
            256..=279 => self.huffman(symbol - 256, 7),
            _ => self.huffman(0xC0 + symbol - 280, 8),
        }
    }

    fn length(&mut self, length: usize) {
        if length == MATCH_MAX {
            return self.symbol(285);
        }
        if length < 11 {
            return self.symbol(254 + length as u32);
        }
        let extra = (length - 3).ilog2() - 2;
        let base = (1usize << (extra + 2)) + 3;
        self.symbol(261 + extra * 4 + ((length - base) >> extra) as u32);
        self.put(((length - base) & ((1 << extra) - 1)) as u32, extra);
    }

    fn distance(&mut self, distance: usize) {
        if distance <= 4 {
            return self.huffman(distance as u32 - 1, 5);
        }
        let extra = (distance - 1).ilog2() - 1;
        let base = (1usize << (extra + 1)) + 1;
        self.huffman(2 + extra * 2 + ((distance - base) >> extra) as u32, 5);
        self.put(((distance - base) & ((1 << extra) - 1)) as u32, extra);
    }

    fn finish(mut self) -> Vec<u8> {
        if self.count > 0 {
            self.bytes.push(self.held as u8);
        }
        self.bytes
    }
}

struct Chains {
    head: Vec<u32>,
    prev: Vec<u32>,
}

impl Chains {
    fn new() -> Chains {
        Chains { head: vec![NONE; 1 << HASH_BITS], prev: vec![NONE; WINDOW] }
    }

    fn insert(&mut self, data: &[u8], at: usize) {
        if at + MATCH_MIN <= data.len() {
            let slot = hash(data, at);
            self.prev[at & (WINDOW - 1)] = self.head[slot];
            self.head[slot] = at as u32;
        }
    }

    fn longest(&self, data: &[u8], at: usize) -> (usize, usize) {
        let (mut best, mut best_distance) = (0, 0);
        if at + MATCH_MIN > data.len() {
            return (best, best_distance);
        }
        let limit = MATCH_MAX.min(data.len() - at);
        let mut candidate = self.head[hash(data, at)];
        for _ in 0..CHAIN {
            if candidate == NONE || candidate as usize >= at || at - candidate as usize > WINDOW {
                break;
            }
            let from = candidate as usize;
            let length = data[from..from + limit].iter().zip(&data[at..at + limit]).take_while(|(before, here)| before == here).count();
            if length > best {
                best = length;
                best_distance = at - from;
                if length == limit {
                    break;
                }
            }
            candidate = self.prev[from & (WINDOW - 1)];
        }
        (best, best_distance)
    }
}

fn hash(data: &[u8], at: usize) -> usize {
    let key = u32::from(data[at]) << 16 | u32::from(data[at + 1]) << 8 | u32::from(data[at + 2]);
    (key.wrapping_mul(0x9E37_79B1) >> (32 - HASH_BITS)) as usize
}

fn deflate(data: &[u8]) -> Vec<u8> {
    let mut bits = Bits { bytes: Vec::with_capacity(data.len() / 16), held: 0, count: 0 };
    bits.put(1, 1);
    bits.put(1, 2);
    let mut chains = Chains::new();
    let mut at = 0;
    while at < data.len() {
        let (length, distance) = chains.longest(data, at);
        if length >= MATCH_MIN {
            bits.length(length);
            bits.distance(distance);
            for skipped in at..at + length {
                chains.insert(data, skipped);
            }
            at += length;
        } else {
            bits.symbol(u32::from(data[at]));
            chains.insert(data, at);
            at += 1;
        }
    }
    bits.symbol(END_OF_BLOCK);
    bits.finish()
}
