const MAGIC: [u8; 2] = [0x1F, 0x8B];
const DEFLATED: u8 = 8;
const HEADER: usize = 10;
const MOST_UNPACKED_BYTES: usize = 64 << 20;
const HEADER_CRC: u8 = 0x02;
const EXTRA: u8 = 0x04;
const NAME: u8 = 0x08;
const COMMENT: u8 = 0x10;
const SIZE_AT: usize = 4;
const MAX_BITS: usize = 15;
const END: u16 = 256;
const STORED: u32 = 0;
const FIXED: u32 = 1;
const DYNAMIC: u32 = 2;
const FIXED_LITERALS: u16 = 288;
const FIXED_DISTANCES: usize = 30;
const MOST_LITERALS: usize = 286;
const MOST_DISTANCES: usize = 30;
const CODE_LENGTHS: usize = 19;
const CODE_ORDER: [usize; CODE_LENGTHS] = [16, 17, 18, 0, 8, 7, 9, 6, 10, 5, 11, 4, 12, 3, 13, 2, 14, 1, 15];
const LENGTH_BASE: [u16; 29] = [3, 4, 5, 6, 7, 8, 9, 10, 11, 13, 15, 17, 19, 23, 27, 31, 35, 43, 51, 59, 67, 83, 99, 115, 131, 163, 195, 227, 258];
const LENGTH_EXTRA: [u8; 29] = [0, 0, 0, 0, 0, 0, 0, 0, 1, 1, 1, 1, 2, 2, 2, 2, 3, 3, 3, 3, 4, 4, 4, 4, 5, 5, 5, 5, 0];
const DISTANCE_BASE: [u16; 30] = [1, 2, 3, 4, 5, 7, 9, 13, 17, 25, 33, 49, 65, 97, 129, 193, 257, 385, 513, 769, 1025, 1537, 2049, 3073, 4097, 6145, 8193, 12289, 16385, 24577];
const DISTANCE_EXTRA: [u8; 30] = [0, 0, 0, 0, 1, 1, 2, 2, 3, 3, 4, 4, 5, 5, 6, 6, 7, 7, 8, 8, 9, 9, 10, 10, 11, 11, 12, 12, 13, 13];

struct Bits<'a> {
    bytes: &'a [u8],
    held: u64,
    count: u32,
}

impl Bits<'_> {
    fn bits(&mut self, count: u32) -> Option<u32> {
        while self.count < count {
            let (&byte, rest) = self.bytes.split_first()?;
            self.bytes = rest;
            self.held |= u64::from(byte) << self.count;
            self.count += 8;
        }
        let value = (self.held & ((1u64 << count) - 1)) as u32;
        self.held >>= count;
        self.count -= count;
        Some(value)
    }

    fn align(&mut self) {
        let partial = self.count % 8;
        self.held >>= partial;
        self.count -= partial;
    }
}

struct Huffman {
    counts: [u16; MAX_BITS + 1],
    symbols: Vec<u16>,
}

impl Huffman {
    fn new(lengths: &[u8]) -> Option<Huffman> {
        let mut counts = [0u16; MAX_BITS + 1];
        for &length in lengths {
            counts[usize::from(length)] += 1;
        }
        counts[0] = 0;
        let mut left = 1i32;
        for &count in &counts[1..] {
            left = (left << 1) - i32::from(count);
            if left < 0 {
                return None;
            }
        }
        let mut offsets = [0u16; MAX_BITS + 1];
        for length in 1..MAX_BITS {
            offsets[length + 1] = offsets[length] + counts[length];
        }
        let mut symbols = vec![0u16; lengths.len()];
        for (symbol, &length) in lengths.iter().enumerate() {
            if length != 0 {
                let offset = &mut offsets[usize::from(length)];
                symbols[usize::from(*offset)] = symbol as u16;
                *offset += 1;
            }
        }
        Some(Huffman { counts, symbols })
    }

    fn decode(&self, bits: &mut Bits) -> Option<u16> {
        let (mut code, mut first, mut index) = (0u32, 0u32, 0usize);
        for &count in &self.counts[1..] {
            code |= bits.bits(1)?;
            let count = u32::from(count);
            if code < first + count {
                return self.symbols.get(index + (code - first) as usize).copied();
            }
            index += count as usize;
            first = (first + count) << 1;
            code <<= 1;
        }
        None
    }
}

fn fixed() -> Option<(Huffman, Huffman)> {
    let literals: Vec<u8> = (0..FIXED_LITERALS)
        .map(|symbol| match symbol {
            0..=143 => 8,
            144..=255 => 9,
            256..=279 => 7,
            _ => 8,
        })
        .collect();
    Some((Huffman::new(&literals)?, Huffman::new(&[5; FIXED_DISTANCES])?))
}

fn dynamic(bits: &mut Bits) -> Option<(Huffman, Huffman)> {
    let literals = bits.bits(5)? as usize + 257;
    let distances = bits.bits(5)? as usize + 1;
    let codes = bits.bits(4)? as usize + 4;
    if literals > MOST_LITERALS || distances > MOST_DISTANCES {
        return None;
    }
    let mut code_lengths = [0u8; CODE_LENGTHS];
    for &at in &CODE_ORDER[..codes] {
        code_lengths[at] = bits.bits(3)? as u8;
    }
    let code = Huffman::new(&code_lengths)?;
    let mut lengths = Vec::with_capacity(literals + distances);
    while lengths.len() < literals + distances {
        let symbol = code.decode(bits)?;
        let (repeated, times) = match symbol {
            0..=15 => (symbol as u8, 1),
            16 => (*lengths.last()?, 3 + bits.bits(2)?),
            17 => (0, 3 + bits.bits(3)?),
            _ => (0, 11 + bits.bits(7)?),
        };
        lengths.extend(std::iter::repeat_n(repeated, times as usize));
    }
    if lengths.len() > literals + distances || lengths[usize::from(END)] == 0 {
        return None;
    }
    Some((Huffman::new(&lengths[..literals])?, Huffman::new(&lengths[literals..])?))
}

fn stored(bits: &mut Bits, out: &mut Vec<u8>) -> Option<()> {
    bits.align();
    let length = bits.bits(16)? as usize;
    let complement = bits.bits(16)? as usize;
    if length != (!complement) & 0xFFFF || out.len() + length > MOST_UNPACKED_BYTES {
        return None;
    }
    let (taken, rest) = bits.bytes.split_at_checked(length)?;
    out.extend_from_slice(taken);
    bits.bytes = rest;
    Some(())
}

fn codes(bits: &mut Bits, out: &mut Vec<u8>, literals: &Huffman, distances: &Huffman) -> Option<()> {
    loop {
        if out.len() > MOST_UNPACKED_BYTES {
            return None;
        }
        let symbol = literals.decode(bits)?;
        if symbol == END {
            return Some(());
        }
        if symbol < END {
            out.push(symbol as u8);
            continue;
        }
        let index = usize::from(symbol - END - 1);
        let length = usize::from(*LENGTH_BASE.get(index)?) + bits.bits(u32::from(LENGTH_EXTRA[index]))? as usize;
        let index = usize::from(distances.decode(bits)?);
        let distance = usize::from(*DISTANCE_BASE.get(index)?) + bits.bits(u32::from(DISTANCE_EXTRA[index]))? as usize;
        let from = out.len().checked_sub(distance)?;
        for offset in 0..length {
            out.push(out[from + offset]);
        }
    }
}

fn inflate(bytes: &[u8]) -> Option<(Vec<u8>, &[u8])> {
    let mut bits = Bits { bytes, held: 0, count: 0 };
    let mut out = Vec::new();
    loop {
        let last = bits.bits(1)? == 1;
        match bits.bits(2)? {
            STORED => stored(&mut bits, &mut out)?,
            FIXED => {
                let (literals, distances) = fixed()?;
                codes(&mut bits, &mut out, &literals, &distances)?;
            }
            DYNAMIC => {
                let (literals, distances) = dynamic(&mut bits)?;
                codes(&mut bits, &mut out, &literals, &distances)?;
            }
            _ => return None,
        }
        if last {
            bits.align();
            return Some((out, bits.bytes));
        }
    }
}

pub fn gunzip(bytes: &[u8]) -> Option<Vec<u8>> {
    let (header, mut rest) = bytes.split_at_checked(HEADER)?;
    if header[..2] != MAGIC || header[2] != DEFLATED {
        return None;
    }
    let flags = header[3];
    if flags & EXTRA != 0 {
        let (length, after) = rest.split_at_checked(2)?;
        rest = after.get(usize::from(u16::from_le_bytes([length[0], length[1]]))..)?;
    }
    for flag in [NAME, COMMENT] {
        if flags & flag != 0 {
            rest = rest.get(rest.iter().position(|&byte| byte == 0)? + 1..)?;
        }
    }
    if flags & HEADER_CRC != 0 {
        rest = rest.get(2..)?;
    }
    let (out, trailer) = inflate(rest)?;
    let size = u32::from_le_bytes(trailer.get(SIZE_AT..SIZE_AT + 4)?.try_into().ok()?);
    (size == out.len() as u32).then_some(out)
}
