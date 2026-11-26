const ALPHABET: &[u8; 64] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";
const LINE: usize = 1024;
const BYTES_PER_LINE: usize = LINE / 4 * 3;
const WORDS_PER_LINE: usize = BYTES_PER_LINE / 2;
const BITS_PER_CHAR: usize = 6;
const CHARS_PER_BLOCK: usize = 8;
const BYTES_PER_BLOCK: usize = CHARS_PER_BLOCK * BITS_PER_CHAR / 8;
const WORDS_PER_BLOCK: usize = BYTES_PER_BLOCK / 2;
const INVALID: u8 = 0xFF;
const PAD: u8 = b'=';
const DECODE: [u8; 256] = decode_table();

const fn decode_table() -> [u8; 256] {
    let mut table = [INVALID; 256];
    let mut i = 0;
    while i < ALPHABET.len() {
        table[ALPHABET[i] as usize] = i as u8;
        i += 1;
    }
    table
}

pub fn lines(words: &[i16]) -> Vec<String> {
    words.chunks(WORDS_PER_LINE).map(encode).collect()
}

fn encode(words: &[i16]) -> String {
    let length = (words.len() * 2).div_ceil(3) * 4;
    let mut text = Vec::with_capacity(length);
    let (blocks, rest) = words.as_chunks::<WORDS_PER_BLOCK>();
    for block in blocks {
        text.extend_from_slice(&encode_block(block));
    }
    if !rest.is_empty() {
        let mut last = [0; WORDS_PER_BLOCK];
        last[..rest.len()].copy_from_slice(rest);
        text.extend_from_slice(&encode_block(&last)[..(rest.len() * i16::BITS as usize).div_ceil(BITS_PER_CHAR)]);
        text.resize(length, PAD);
    }
    String::from_utf8(text).unwrap_or_default()
}

fn encode_block(block: &[i16; WORDS_PER_BLOCK]) -> [u8; CHARS_PER_BLOCK] {
    let bits = block.iter().fold(0, |bits, word| bits << i16::BITS | u64::from(u16::from_be_bytes(word.to_le_bytes())));
    std::array::from_fn(|at| ALPHABET[(bits >> (BITS_PER_CHAR * (CHARS_PER_BLOCK - 1 - at))) as usize & 63])
}

pub fn words<'a>(lines: impl Iterator<Item = &'a str> + Clone) -> Option<Vec<i16>> {
    let chars: usize = lines.clone().map(str::len).sum();
    let mut words = Vec::with_capacity(chars / CHARS_PER_BLOCK * WORDS_PER_BLOCK + WORDS_PER_BLOCK);
    let mut block = Block::default();
    for line in lines {
        block.read(line.as_bytes(), &mut words)?;
    }
    block.finish(&mut words);
    Some(words)
}

#[derive(Default)]
struct Block {
    bits: u64,
    held: usize,
}

impl Block {
    fn read(&mut self, mut line: &[u8], words: &mut Vec<i16>) -> Option<()> {
        loop {
            if self.held == 0 {
                line = whole_blocks(line, words);
            }
            let Some((&byte, rest)) = line.split_first() else {
                return Some(());
            };
            if byte == PAD {
                return Some(());
            }
            line = rest;
            if !byte.is_ascii_whitespace() {
                self.hold(DECODE[usize::from(byte)], words)?;
            }
        }
    }

    fn hold(&mut self, value: u8, words: &mut Vec<i16>) -> Option<()> {
        if value == INVALID {
            return None;
        }
        self.bits = (self.bits << BITS_PER_CHAR) | u64::from(value);
        self.held += 1;
        if self.held == CHARS_PER_BLOCK {
            put(self.bits, WORDS_PER_BLOCK, words);
            *self = Block::default();
        }
        Some(())
    }

    fn finish(self, words: &mut Vec<i16>) {
        put(self.bits << (BITS_PER_CHAR * (CHARS_PER_BLOCK - self.held)), self.held * BITS_PER_CHAR / i16::BITS as usize, words);
    }
}

fn whole_blocks<'a>(line: &'a [u8], words: &mut Vec<i16>) -> &'a [u8] {
    let mut read = 0;
    for block in line.as_chunks::<CHARS_PER_BLOCK>().0 {
        let values = block.map(|byte| DECODE[usize::from(byte)]);
        if values.contains(&INVALID) {
            break;
        }
        put(values.iter().fold(0, |bits, &value| (bits << BITS_PER_CHAR) | u64::from(value)), WORDS_PER_BLOCK, words);
        read += CHARS_PER_BLOCK;
    }
    &line[read..]
}

fn put(block: u64, count: usize, words: &mut Vec<i16>) {
    let bytes = block.to_be_bytes();
    words.extend(bytes[bytes.len() - BYTES_PER_BLOCK..].as_chunks::<2>().0[..count].iter().map(|pair| i16::from_le_bytes(*pair)));
}
