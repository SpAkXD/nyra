//! Inflate: the decompressor of DEFLATE (RFC 1951) and of the zlib wrapper around it (RFC 1950),
//! for PNG image data and PNG's compressed chunks (zero crates). All three block kinds (stored,
//! fixed and dynamic Huffman) are decoded. Codes are looked up in a 10-bit table with second-level
//! tables for longer codes, the input is read through a 64-bit bit buffer, and a fast loop with no
//! per-symbol bounds on input or output runs until either is close to its end; the speed is about
//! that of zlib-ng.
//!
//! Every caller passes a `limit` on the output size, so a small hostile file cannot inflate to
//! gigabytes. Corrupt or truncated input is an error, never a panic or an endless loop.

use super::common::Res;

/// Bits looked up in the first-level table.
const FAST: u32 = 10;
const FAST_MASK: u64 = (1 << FAST) - 1;
/// A table entry with this flag points to a second-level table (start in the high 16 bits, its
/// index bits in the low 4); other entries hold the symbol (high 16 bits) and the code length
/// (low 4 bits; 0 means no code).
const SUB: u32 = 0x10;

const LEN_BASE: [u16; 29] =
    [3, 4, 5, 6, 7, 8, 9, 10, 11, 13, 15, 17, 19, 23, 27, 31, 35, 43, 51, 59, 67, 83, 99, 115, 131, 163, 195, 227, 258];
const LEN_EXTRA: [u8; 29] = [0, 0, 0, 0, 0, 0, 0, 0, 1, 1, 1, 1, 2, 2, 2, 2, 3, 3, 3, 3, 4, 4, 4, 4, 5, 5, 5, 5, 0];
const DIST_BASE: [u16; 30] = [
    1, 2, 3, 4, 5, 7, 9, 13, 17, 25, 33, 49, 65, 97, 129, 193, 257, 385, 513, 769, 1025, 1537, 2049, 3073, 4097, 6145, 8193, 12289,
    16385, 24577,
];
const DIST_EXTRA: [u8; 30] = [0, 0, 0, 0, 1, 1, 2, 2, 3, 3, 4, 4, 5, 5, 6, 6, 7, 7, 8, 8, 9, 9, 10, 10, 11, 11, 12, 12, 13, 13];
/// The order the code-length code lengths are stored in.
const CL_ORDER: [usize; 19] = [16, 17, 18, 0, 8, 7, 9, 6, 10, 5, 11, 4, 12, 3, 13, 2, 14, 1, 15];

const TRUNCATED: &str = "truncated DEFLATE data";

fn corrupt(what: &str) -> String {
    format!("corrupt DEFLATE data ({what})")
}

fn over_limit(limit: usize) -> String {
    format!("the decompressed data is larger than the limit of {limit} bytes")
}

/// Decompresses zlib data (a 2-byte header, DEFLATE data and an Adler-32 checksum) into at most
/// `limit` bytes. A wrong checksum is an error; a missing one (the data ends right after the last
/// block) is accepted, since the data itself was complete.
pub fn zlib(data: &[u8], limit: usize) -> Res<Vec<u8>> {
    let mut out = Vec::with_capacity(first_capacity(data.len(), limit));
    zlib_to(data, &mut out, limit, true)?;
    Ok(out)
}

/// Decompresses raw DEFLATE data (no header) into at most `limit` bytes.
#[cfg(test)]
pub fn inflate(data: &[u8], limit: usize) -> Res<Vec<u8>> {
    let mut out = Vec::with_capacity(first_capacity(data.len(), limit));
    inflate_to(data, &mut out, limit)?;
    Ok(out)
}

/// Like [`zlib`], but appends to `out` (reserve its capacity first when the size is known), and
/// checks the Adler-32 checksum only when `check_adler` is set. On an error `out` keeps what was
/// decoded until then; when the output would pass `limit`, it is filled to exactly `limit` bytes
/// before the error, so a caller that knows the size it wants can tell complete data from short.
pub fn zlib_to(data: &[u8], out: &mut Vec<u8>, limit: usize, check_adler: bool) -> Res<()> {
    let &[cmf, flg, ..] = data else { return Err("truncated zlib data".into()) };
    if cmf & 15 != 8 || cmf >> 4 > 7 {
        return Err("not zlib data (unknown compression method)".into());
    }
    if (u16::from(cmf) << 8 | u16::from(flg)) % 31 != 0 {
        return Err("not zlib data (bad header check)".into());
    }
    if flg & 0x20 != 0 {
        return Err("zlib data with a preset dictionary is not supported".into());
    }
    let start = out.len();
    let used = inflate_to(&data[2..], out, limit)?;
    if let Some(sum) = data.get(2 + used..2 + used + 4) {
        if check_adler && u32::from_be_bytes([sum[0], sum[1], sum[2], sum[3]]) != adler32(&out[start..]) {
            return Err("corrupt zlib data (wrong Adler-32 checksum)".into());
        }
    }
    Ok(())
}

/// A first guess of the output size, so the vector does not start tiny.
fn first_capacity(input: usize, limit: usize) -> usize {
    limit.min(input.saturating_mul(4).max(4096))
}

/// The Adler-32 checksum of `data`.
pub fn adler32(data: &[u8]) -> u32 {
    let (mut a, mut b) = (1u32, 0u32);
    // 5552 bytes is the most that can be summed before `b` could pass 2^32.
    for chunk in data.chunks(5552) {
        for &x in chunk {
            a += u32::from(x);
            b += a;
        }
        a %= 65521;
        b %= 65521;
    }
    b << 16 | a
}

/// The input as a stream of bits, least significant first. Past the end of the input it reads
/// zeros and counts them in `over`; once fewer bits are left in the buffer than were padded,
/// the decoder has read past the end and the data is truncated.
struct Bits<'a> {
    data: &'a [u8],
    pos: usize,
    buf: u64,
    n: u32,
    over: u32,
}

impl Bits<'_> {
    /// Fills the buffer to at least 56 bits. The bits above `n` are either zero or the true next
    /// bits of the input, so OR-ing the same bytes in again is harmless.
    #[inline(always)]
    fn refill(&mut self) {
        if let Some(&b) = self.data.get(self.pos..).and_then(|s| s.first_chunk::<8>()) {
            self.buf |= u64::from_le_bytes(b) << self.n;
            let k = (63 - self.n) >> 3;
            self.pos += k as usize;
            self.n += k * 8;
        } else {
            while self.n < 56 {
                let byte = match self.data.get(self.pos) {
                    Some(&b) => {
                        self.pos += 1;
                        b
                    }
                    None => {
                        self.over = self.over.saturating_add(8);
                        0
                    }
                };
                self.buf |= u64::from(byte) << self.n;
                self.n += 8;
            }
        }
    }

    /// True once the decoder has used bits from past the end of the input.
    #[inline(always)]
    fn overrun(&self) -> bool {
        self.n < self.over
    }

    #[inline(always)]
    fn consume(&mut self, k: u32) {
        self.buf >>= k;
        self.n -= k;
    }

    /// The next `k` bits (k <= 32) without refilling; the caller made sure there are enough.
    #[inline(always)]
    fn take(&mut self, k: u32) -> u32 {
        let v = (self.buf & ((1u64 << k) - 1)) as u32;
        self.consume(k);
        v
    }

    /// The next `k` bits (k <= 32).
    fn bits(&mut self, k: u32) -> u32 {
        if self.n < k {
            self.refill();
        }
        self.take(k)
    }

    /// The next Huffman-coded symbol; the buffer must hold at least 15 bits.
    #[inline(always)]
    fn decode(&mut self, table: &[u32]) -> Res<usize> {
        let e = lookup(table, self.buf);
        let len = e & 15;
        if len == 0 {
            return Err(bad_code());
        }
        self.consume(len);
        Ok((e >> 16) as usize)
    }

    /// The match length of length symbol `sym` and the distance after it. The buffer must hold
    /// at least 33 bits (5 extra length bits, a 15-bit distance code and 13 extra bits).
    #[inline(always)]
    fn length_distance(&mut self, sym: usize, dist: &[u32]) -> Res<(usize, usize)> {
        let i = sym - 257;
        if i >= 29 {
            return Err(corrupt("invalid length code"));
        }
        let len = LEN_BASE[i] as usize + self.take(u32::from(LEN_EXTRA[i])) as usize;
        let ds = self.decode(dist)?;
        if ds >= 30 {
            return Err(corrupt("invalid distance code"));
        }
        Ok((len, DIST_BASE[ds] as usize + self.take(u32::from(DIST_EXTRA[ds])) as usize))
    }

    /// The input bytes used so far (whole bytes; a partly used byte counts as used).
    fn used(&self) -> usize {
        self.pos - ((self.n - self.over) / 8) as usize
    }
}

/// The table entry for the next code in `buf` (following a second-level pointer).
#[inline(always)]
fn lookup(table: &[u32], buf: u64) -> u32 {
    // Every table starts with the 1 << FAST first-level entries; saying so drops a bounds check.
    let first: &[u32; 1 << FAST] = table[..1 << FAST].try_into().unwrap();
    let e = first[(buf & FAST_MASK) as usize];
    if e & SUB == 0 {
        return e;
    }
    table[(e >> 16) as usize + ((buf >> FAST) & ((1 << (e & 15)) - 1)) as usize]
}

fn bad_code() -> String {
    corrupt("invalid Huffman code")
}

/// The output being written: `v[..pos]` is done, `v[pos..]` is room already allocated (zeros,
/// or bytes a fast copy wrote past its end, which are overwritten later). `pos` may not pass
/// `max`; `base` is where this stream's output starts.
struct Out<'a> {
    v: &'a mut Vec<u8>,
    pos: usize,
    base: usize,
    max: usize,
    limit: usize,
}

/// The room the fast loop wants: the longest match plus the 8 bytes a chunked copy may overshoot.
const FAST_ROOM: usize = 258 + 8;

impl Out<'_> {
    /// Makes room for `need` more bytes, growing the vector (not past `max`). False if `max` is
    /// too close.
    #[inline(always)]
    fn room(&mut self, need: usize) -> bool {
        self.v.len() - self.pos >= need || self.grow(need)
    }

    #[inline(never)]
    fn grow(&mut self, need: usize) -> bool {
        let want = self.pos.saturating_add(need).max(self.pos + (1 << 15)).max(self.v.len().saturating_mul(2)).max(self.v.capacity());
        let target = self.max.min(want);
        if target > self.v.len() {
            self.v.resize(target, 0);
        }
        self.v.len() - self.pos >= need
    }

    /// Copies a match byte by byte (room for `len` bytes must be there).
    fn copy_slow(&mut self, d: usize, len: usize) {
        for i in self.pos..self.pos + len {
            self.v[i] = self.v[i - d];
        }
        self.pos += len;
    }
}

/// Inflates `data` onto the end of `out`, returning the number of input bytes the DEFLATE stream
/// used. Distances may not reach before the start of the output this call wrote.
fn inflate_to(data: &[u8], out: &mut Vec<u8>, limit: usize) -> Res<usize> {
    let base = out.len();
    let mut o = Out { pos: base, base, max: base.saturating_add(limit), limit, v: out };
    let r = blocks(&mut Bits { data, pos: 0, buf: 0, n: 0, over: 0 }, &mut o);
    let pos = o.pos;
    out.truncate(pos);
    r
}

fn blocks(b: &mut Bits, o: &mut Out) -> Res<usize> {
    let (mut lit, mut dist) = (Vec::new(), Vec::new());
    let (mut fixed_lit, mut fixed_dist) = (Vec::new(), Vec::new());
    loop {
        b.refill();
        let last = b.take(1);
        match b.take(2) {
            0 => stored(b, o)?,
            1 => {
                if fixed_lit.is_empty() {
                    let mut lens = [0u8; 288];
                    lens[..144].fill(8);
                    lens[144..256].fill(9);
                    lens[256..280].fill(7);
                    lens[280..].fill(8);
                    build(&lens, &mut fixed_lit)?;
                    build(&[5; 32], &mut fixed_dist)?;
                }
                codes(b, o, &fixed_lit, &fixed_dist)?;
            }
            2 => {
                dynamic(b, &mut lit, &mut dist)?;
                codes(b, o, &lit, &dist)?;
            }
            _ => return Err(corrupt("invalid block type")),
        }
        if b.overrun() {
            return Err(TRUNCATED.into());
        }
        if last == 1 {
            return Ok(b.used());
        }
    }
}

/// A stored block: the length, its complement and the bytes as they are.
fn stored(b: &mut Bits, o: &mut Out) -> Res<()> {
    let skip = b.n % 8;
    b.consume(skip);
    let len = b.bits(16) as usize;
    let nlen = b.bits(16) as usize;
    if b.overrun() {
        return Err(TRUNCATED.into());
    }
    if len != !nlen & 0xFFFF {
        return Err(corrupt("stored block length does not match its complement"));
    }
    // Go back to reading bytes directly: rewind over the whole bytes still in the buffer.
    b.pos = b.used();
    b.buf = 0;
    b.n = 0;
    b.over = 0;
    let room = o.max - o.pos;
    let n = len.min(room).min(b.data.len() - b.pos);
    o.room(n);
    o.v[o.pos..o.pos + n].copy_from_slice(&b.data[b.pos..b.pos + n]);
    o.pos += n;
    b.pos += n;
    if n < len {
        return Err(if len > room { over_limit(o.limit) } else { TRUNCATED.into() });
    }
    Ok(())
}

/// The code lengths of a dynamic block, coded with the code-length code.
fn dynamic(b: &mut Bits, lit: &mut Vec<u32>, dist: &mut Vec<u32>) -> Res<()> {
    b.refill();
    let nlit = b.take(5) as usize + 257;
    let ndist = b.take(5) as usize + 1;
    let ncl = b.take(4) as usize + 4;
    if nlit > 286 || ndist > 30 {
        return Err(corrupt("too many length or distance codes"));
    }
    let mut cl = [0u8; 19];
    for &i in &CL_ORDER[..ncl] {
        cl[i] = b.bits(3) as u8;
    }
    let mut cl_table = Vec::new();
    build(&cl, &mut cl_table)?;
    let mut lens = [0u8; 286 + 30];
    let total = nlit + ndist;
    let mut i = 0;
    while i < total {
        b.refill();
        if b.overrun() {
            return Err(TRUNCATED.into());
        }
        let sym = b.decode(&cl_table)?;
        let (value, count) = match sym {
            0..=15 => (sym as u8, 1),
            16 => {
                if i == 0 {
                    return Err(corrupt("a repeat code with nothing to repeat"));
                }
                (lens[i - 1], 3 + b.take(2) as usize)
            }
            17 => (0, 3 + b.take(3) as usize),
            _ => (0, 11 + b.take(7) as usize),
        };
        if i + count > total {
            return Err(corrupt("code lengths run past their end"));
        }
        lens[i..i + count].fill(value);
        i += count;
    }
    if lens[256] == 0 {
        return Err(corrupt("no end-of-block code"));
    }
    build(&lens[..nlit], lit)?;
    build(&lens[nlit..total], dist)
}

/// The fast loop of [`codes`]: while 16 input bytes and FAST_ROOM bytes of room are left,
/// nothing can run out within one turn, so neither is checked per symbol. A refill gives at
/// least 56 bits: two literals (2 x 15), or a length/distance pair (15 + 5 + 15 + 13 = 48).
/// The state lives in local variables so it stays in registers. True at the end of the block,
/// false when the input or the room runs low.
fn fast(b: &mut Bits, o: &mut Out, lit: &[u32], dist: &[u32]) -> Res<bool> {
    let data = b.data;
    let (mut buf, mut n, mut ip, mut op, base) = (b.buf, b.n, b.pos, o.pos, o.base);
    let out = o.v.as_mut_slice();
    let (in_end, out_end) = (data.len() - 16, out.len() - FAST_ROOM);
    macro_rules! refill {
        () => {{
            buf |= u64::from_le_bytes(data[ip..ip + 8].try_into().unwrap()) << n;
            let k = (63 - n) >> 3;
            ip += k as usize;
            n += k * 8;
        }};
    }
    macro_rules! take {
        ($k:expr) => {{
            let k = u32::from($k);
            let v = (buf & ((1u64 << k) - 1)) as usize;
            buf >>= k;
            n -= k;
            v
        }};
    }
    macro_rules! symbol {
        ($table:expr, $out:lifetime) => {{
            let e = lookup($table, buf);
            if e & 15 == 0 {
                break $out Err(bad_code());
            }
            // Bit 4 is clear in a symbol entry, so `& 63` is the length (and free on x86).
            buf >>= e & 63;
            n -= e & 15;
            (e >> 16) as usize
        }};
    }
    let r = 'outer: loop {
        if ip > in_end || op > out_end {
            break Ok(false);
        }
        refill!();
        let mut sym = symbol!(lit, 'outer);
        // Literals while the buffer holds a whole code: at most 56 of them, well within the room.
        while sym < 256 {
            out[op] = sym as u8;
            op += 1;
            if n < 15 {
                continue 'outer;
            }
            sym = symbol!(lit, 'outer);
        }
        if sym == 256 {
            break Ok(true);
        }
        if n < 33 {
            // The second refill of this turn; the 16 bytes of input checked above cover both.
            refill!();
        }
        let i = sym - 257;
        if i >= 29 {
            break Err(corrupt("invalid length code"));
        }
        let len = LEN_BASE[i] as usize + take!(LEN_EXTRA[i]);
        let ds = symbol!(dist, 'outer);
        if ds >= 30 {
            break Err(corrupt("invalid distance code"));
        }
        let d = DIST_BASE[ds] as usize + take!(DIST_EXTRA[ds]);
        if d > op - base {
            break Err(corrupt("a distance before the start of the data"));
        }
        copy_fast(out, op, d, len);
        op += len;
    };
    (b.buf, b.n, b.pos, o.pos) = (buf, n, ip, op);
    r
}

/// Copies a match of `len` bytes from `d` back to `out[op..]`, writing whole 8-byte chunks, so
/// up to 7 bytes past the match are overwritten too (there must be room for them).
#[inline(always)]
fn copy_fast(out: &mut [u8], op: usize, d: usize, len: usize) {
    let end = op + len;
    if d == 1 {
        let b = out[op - 1];
        out[op..end].fill(b);
        return;
    }
    // A period of d is also one of any multiple of d; use one of at least 8, so each chunk
    // reads only finished bytes, after copying the first few bytes one by one.
    let step = if d >= 8 { d } else { d * 8usize.div_ceil(d) };
    let mut t = op;
    while t < (op + step - d).min(end) {
        out[t] = out[t - d];
        t += 1;
    }
    while t < end {
        let chunk: [u8; 8] = out[t - step..t - step + 8].try_into().unwrap();
        out[t..t + 8].copy_from_slice(&chunk);
        t += 8;
    }
}

/// The symbols of a Huffman-coded block, up to its end-of-block code.
fn codes(b: &mut Bits, o: &mut Out, lit: &[u32], dist: &[u32]) -> Res<()> {
    loop {
        while b.pos + 16 <= b.data.len() && o.room(FAST_ROOM) {
            if fast(b, o, lit, dist)? {
                return Ok(());
            }
        }
        // Near the end of the input or of the room: one symbol with every check.
        b.refill();
        let sym = b.decode(lit)?;
        if sym < 256 {
            if b.overrun() {
                return Err(TRUNCATED.into());
            }
            if !o.room(1) {
                return Err(over_limit(o.limit));
            }
            o.v[o.pos] = sym as u8;
            o.pos += 1;
            continue;
        }
        if sym == 256 {
            return Ok(());
        }
        let (len, d) = b.length_distance(sym, dist)?;
        if b.overrun() {
            return Err(TRUNCATED.into());
        }
        if d > o.pos - o.base {
            return Err(corrupt("a distance before the start of the data"));
        }
        if !o.room(len) {
            let n = o.max - o.pos;
            o.room(n);
            o.copy_slow(d, n);
            return Err(over_limit(o.limit));
        }
        o.copy_slow(d, len);
    }
}

/// Builds the decoding table of a canonical Huffman code from its code lengths (0 = unused).
/// Over-subscribed codes are an error; incomplete ones are allowed (an unused bit pattern is an
/// error only when it is met).
fn build(lens: &[u8], table: &mut Vec<u32>) -> Res<()> {
    let mut count = [0u32; 16];
    for &l in lens {
        count[l as usize] += 1;
    }
    count[0] = 0;
    let mut left = 1i32;
    for &c in &count[1..] {
        left = left * 2 - c as i32;
        if left < 0 {
            return Err(corrupt("over-subscribed Huffman code"));
        }
    }
    let mut next = [0u32; 16];
    let mut code = 0;
    for l in 1..16 {
        code = (code + count[l - 1]) << 1;
        next[l] = code;
    }
    // Each code reversed (DEFLATE sends codes most significant bit first into an LSB-first stream).
    let mut rev = [0u16; 320];
    let mut sub_len = [0u8; 1 << FAST];
    for (s, &l) in lens.iter().enumerate() {
        if l == 0 {
            continue;
        }
        let r = next[l as usize].reverse_bits() >> (32 - u32::from(l));
        next[l as usize] += 1;
        rev[s] = r as u16;
        if u32::from(l) > FAST {
            let p = (r as u64 & FAST_MASK) as usize;
            sub_len[p] = sub_len[p].max(l);
        }
    }
    table.clear();
    table.resize(1 << FAST, 0);
    for (p, &l) in sub_len.iter().enumerate() {
        if l > 0 {
            let bits = u32::from(l) - FAST;
            let start = table.len() as u32;
            table.resize(table.len() + (1 << bits), 0);
            table[p] = start << 16 | SUB | bits;
        }
    }
    for (s, &l) in lens.iter().enumerate() {
        let l = u32::from(l);
        if l == 0 {
            continue;
        }
        let r = rev[s] as usize;
        let entry = (s as u32) << 16 | l;
        if l <= FAST {
            let mut i = r;
            while i < 1 << FAST {
                table[i] = entry;
                i += 1 << l;
            }
        } else {
            let e = table[r & FAST_MASK as usize];
            let (start, size) = ((e >> 16) as usize, 1usize << (e & 15));
            let mut i = r >> FAST;
            while i < size {
                table[start + i] = entry;
                i += 1 << (l - FAST);
            }
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The bytes the fixtures compress (`zdata` in tools/img_fixtures_png.py).
    fn zdata(n: usize) -> Vec<u8> {
        let mut out = Vec::new();
        let mut s: u64 = 12345;
        let mut i: usize = 0;
        while out.len() < n {
            s = (s * 1103515245 + 12345) & 0x7FFF_FFFF;
            match (s >> 16) % 4 {
                0 => out.push((s >> 8) as u8),
                1 => out.extend(std::iter::repeat_n((s >> 4) as u8, ((s >> 20) % 40 + 1) as usize)),
                2 => out.extend_from_slice(format!("the quick brown fox {} ", i % 97).as_bytes()),
                _ => out.extend((0..(s >> 12) % 300).map(|k| (i as u64 + k) as u8)),
            }
            i += 1;
        }
        out.truncate(n);
        out
    }

    const FIXTURES: [(&[u8], usize); 4] = [
        (include_bytes!("testdata/png-zlib-0.z"), 3000),
        (include_bytes!("testdata/png-zlib-1.z"), 20000),
        (include_bytes!("testdata/png-zlib-9.z"), 20000),
        (include_bytes!("testdata/png-zlib-fixed.z"), 5000),
    ];

    #[test]
    fn zlib_levels_0_1_9_and_fixed() {
        for (z, n) in FIXTURES {
            assert_eq!(zlib(z, 1 << 20).unwrap(), zdata(n));
            // The same stream without its zlib wrapper.
            assert_eq!(inflate(&z[2..z.len() - 4], 1 << 20).unwrap(), zdata(n));
        }
    }

    #[test]
    fn limit_and_partial_output() {
        let (z, n) = FIXTURES[2];
        assert!(zlib(z, n - 1).unwrap_err().contains("limit"));
        for limit in [0, 1, 1000, 12345, n - 1] {
            let mut out = Vec::new();
            assert!(zlib_to(z, &mut out, limit, true).unwrap_err().contains("limit"));
            assert_eq!(out, zdata(limit));
        }
        assert_eq!(zlib(z, n).unwrap().len(), n);
    }

    #[test]
    fn checksum_header_and_truncation() {
        let (z, n) = FIXTURES[1];
        let mut bad = z.to_vec();
        *bad.last_mut().unwrap() ^= 1;
        assert!(zlib(&bad, n).unwrap_err().contains("Adler-32"));
        let mut out = Vec::new();
        assert!(zlib_to(&bad, &mut out, n, false).is_ok());
        // A missing checksum is accepted; missing data is not.
        assert_eq!(zlib(&z[..z.len() - 4], n).unwrap().len(), n);
        for cut in 0..z.len() - 4 {
            assert!(zlib(&z[..cut], n).is_err(), "cut at {cut}");
        }
        assert!(zlib(b"\x78\x9c", 10).unwrap_err().contains("truncated"));
        assert!(zlib(b"\x78\x9d\x03\x00", 10).unwrap_err().contains("header"));
        assert!(zlib(b"\x79\x9c\x03\x00", 10).is_err());
    }

    #[test]
    fn hand_made_streams() {
        // An empty fixed block; an empty stored block; a stored block with a bad complement.
        assert_eq!(inflate(&[0x03, 0x00], 10).unwrap(), b"");
        assert_eq!(inflate(&[0x01, 0x00, 0x00, 0xFF, 0xFF], 10).unwrap(), b"");
        assert!(inflate(&[0x01, 0x00, 0x00, 0xFF, 0xFE], 10).is_err());
        // Two stored blocks, then the limit.
        let two = [0x00, 0x02, 0x00, 0xFD, 0xFF, b'a', b'b', 0x01, 0x01, 0x00, 0xFE, 0xFF, b'c'];
        assert_eq!(inflate(&two, 10).unwrap(), b"abc");
        assert!(inflate(&two, 2).is_err());
        // Block type 3 is invalid.
        assert!(inflate(&[0x07], 10).is_err());
        // A distance before the start: fixed block, length 3 (code 257), distance 1, at the start.
        assert!(inflate(&[0x03, 0x02, 0x00], 10).is_err());
    }

    #[test]
    fn overlapping_copies_and_adler() {
        // Every distance 1..40 and length 3..258 against a byte-by-byte copy.
        let data = zdata(20000);
        let mut want = data[..100].to_vec();
        let mut v = want.clone();
        let mut o = Out { pos: 100, base: 0, max: usize::MAX, limit: usize::MAX, v: &mut v };
        for d in 1..40 {
            for len in (3..=258).step_by(17) {
                assert!(o.room(FAST_ROOM));
                copy_fast(o.v, o.pos, d, len);
                o.pos += len;
                for _ in 0..len {
                    want.push(want[want.len() - d]);
                }
            }
        }
        o.copy_slow(7, 20);
        for _ in 0..20 {
            want.push(want[want.len() - 7]);
        }
        let pos = o.pos;
        v.truncate(pos);
        assert_eq!(v, want);
        assert_eq!(adler32(&data), {
            let (mut a, mut b) = (1u64, 0u64);
            for &x in &data {
                a = (a + u64::from(x)) % 65521;
                b = (b + a) % 65521;
            }
            (b << 16 | a) as u32
        });
    }

    #[test]
    fn corrupt_input_never_panics() {
        for (z, n) in FIXTURES {
            let mut s = 1u32;
            for _ in 0..300 {
                s = s.wrapping_mul(1664525).wrapping_add(1013904223);
                let mut bad = z.to_vec();
                let at = (s >> 8) as usize % bad.len();
                bad[at] ^= (s >> 24) as u8 | 1;
                let r = std::panic::catch_unwind(|| zlib(&bad, n * 2));
                assert!(r.is_ok(), "panic with byte {at} changed");
            }
        }
    }
}
