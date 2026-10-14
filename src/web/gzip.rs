//! The size a file has after `gzip -9`, computed without compressing it to bytes (zero crates):
//! the DEFLATE symbols of an LZ77 pass with lazy matching like zlib's, coded as one block with
//! its own Huffman codes, plus the gzip header and trailer. On text it lands within about 1% of
//! `gzip -9` (motion.js for a typical page: 1,472 bytes here, 1,473 from gzip). The size budgets
//! of `use web` (motion JS 1.5 KB, a site's CSS and JS) are checked with it.

// The build (WP5) checks its budgets with this; until it lands only the tests use it.
#![allow(dead_code)]

use std::cmp::Reverse;
use std::collections::BinaryHeap;

const LEN_BASE: [usize; 29] =
    [3, 4, 5, 6, 7, 8, 9, 10, 11, 13, 15, 17, 19, 23, 27, 31, 35, 43, 51, 59, 67, 83, 99, 115, 131, 163, 195, 227, 258];
const LEN_EXTRA: [usize; 29] = [0, 0, 0, 0, 0, 0, 0, 0, 1, 1, 1, 1, 2, 2, 2, 2, 3, 3, 3, 3, 4, 4, 4, 4, 5, 5, 5, 5, 0];
const DIST_BASE: [usize; 30] = [
    1, 2, 3, 4, 5, 7, 9, 13, 17, 25, 33, 49, 65, 97, 129, 193, 257, 385, 513, 769, 1025, 1537, 2049, 3073, 4097, 6145, 8193,
    12289, 16385, 24577,
];
const DIST_EXTRA: [usize; 30] = [0, 0, 0, 0, 1, 1, 2, 2, 3, 3, 4, 4, 5, 5, 6, 6, 7, 7, 8, 8, 9, 9, 10, 10, 11, 11, 12, 12, 13, 13];
/// The order the code-length code lengths are written in.
const CL_ORDER: [usize; 19] = [16, 17, 18, 0, 8, 7, 9, 6, 10, 5, 11, 4, 12, 3, 13, 2, 14, 1, 15];

/// Bytes of `data` after `gzip -9`, within about 1% on text.
pub fn gzip_size(data: &[u8]) -> usize {
    let (mut lit, mut dist) = (vec![0usize; 286], vec![0usize; 30]);
    let mut extra = 0usize;
    for s in lz77(data) {
        match s {
            Sym::Lit(b) => lit[b as usize] += 1,
            Sym::Match(l, d) => {
                let lc = LEN_BASE.iter().rposition(|&b| b <= l).unwrap();
                let dc = DIST_BASE.iter().rposition(|&b| b <= d).unwrap();
                lit[257 + lc] += 1;
                dist[dc] += 1;
                extra += LEN_EXTRA[lc] + DIST_EXTRA[dc];
            }
        }
    }
    lit[256] = 1;
    let ll = lengths(&lit, 15);
    let dl = lengths(&dist, 15);
    let data_bits: usize = lit.iter().zip(&ll).chain(dist.iter().zip(&dl)).map(|(f, l)| f * l).sum::<usize>() + extra;
    let bits = 3 + header_bits(&ll, &dl) + data_bits;
    bits.div_ceil(8) + 18
}

enum Sym {
    Lit(u8),
    Match(usize, usize),
}

/// The longest earlier match at `i` (length, distance), searching up to 4096 earlier places.
fn longest(data: &[u8], i: usize, chains: &std::collections::HashMap<[u8; 3], Vec<usize>>) -> (usize, usize) {
    if i + 3 > data.len() {
        return (0, 0);
    }
    let mut best = (0, 0);
    if let Some(c) = chains.get(&[data[i], data[i + 1], data[i + 2]]) {
        for &j in c.iter().rev().take(4096) {
            if i - j > 32768 {
                break;
            }
            let mut l = 0;
            while l < 258 && i + l < data.len() && data[j + l] == data[i + l] {
                l += 1;
            }
            if l > best.0 {
                best = (l, i - j);
                if l == 258 {
                    break;
                }
            }
        }
    }
    best
}

fn lz77(data: &[u8]) -> Vec<Sym> {
    let mut chains: std::collections::HashMap<[u8; 3], Vec<usize>> = std::collections::HashMap::new();
    let add = |chains: &mut std::collections::HashMap<[u8; 3], Vec<usize>>, k: usize| {
        if k + 3 <= data.len() {
            chains.entry([data[k], data[k + 1], data[k + 2]]).or_default().push(k);
        }
    };
    let mut out = Vec::new();
    let mut i = 0;
    while i < data.len() {
        let m = longest(data, i, &chains);
        add(&mut chains, i);
        // lazy: a longer match one byte later wins over this one
        if m.0 >= 3 && (m.0 >= 32 || longest(data, i + 1, &chains).0 <= m.0) {
            out.push(Sym::Match(m.0, m.1));
            for k in i + 1..i + m.0 {
                add(&mut chains, k);
            }
            i += m.0;
        } else {
            out.push(Sym::Lit(data[i]));
            i += 1;
        }
    }
    out
}

/// Huffman code lengths for the frequencies, none longer than `max` (frequencies are halved
/// until they fit, which costs a fraction of a percent).
fn lengths(freq: &[usize], max: usize) -> Vec<usize> {
    let mut f: Vec<usize> = freq.to_vec();
    loop {
        let used: Vec<usize> = (0..f.len()).filter(|&i| f[i] > 0).collect();
        let mut len = vec![0usize; f.len()];
        if used.len() == 1 {
            len[used[0]] = 1;
            return len;
        }
        // nodes: leaves first, then inner nodes; parent links give the depths
        let mut parent: Vec<usize> = vec![usize::MAX; used.len()];
        let mut heap: BinaryHeap<Reverse<(usize, usize)>> = used.iter().enumerate().map(|(n, &s)| Reverse((f[s], n))).collect();
        while heap.len() > 1 {
            let Reverse((a, x)) = heap.pop().unwrap();
            let Reverse((b, y)) = heap.pop().unwrap();
            let n = parent.len();
            parent.push(usize::MAX);
            parent[x] = n;
            parent[y] = n;
            heap.push(Reverse((a + b, n)));
        }
        let mut deepest = 0;
        for (n, &s) in used.iter().enumerate() {
            let (mut d, mut p) = (0, n);
            while parent[p] != usize::MAX {
                p = parent[p];
                d += 1;
            }
            len[s] = d;
            deepest = deepest.max(d);
        }
        if deepest <= max {
            return len;
        }
        f = f.iter().map(|&x| if x > 0 { x.div_ceil(2) } else { 0 }).collect();
    }
}

/// The bits of a dynamic block's header: the counts, then the code lengths, run-length coded
/// and written with their own Huffman code.
fn header_bits(ll: &[usize], dl: &[usize]) -> usize {
    let hlit = (257..=286).rev().find(|&n| ll[n - 1] > 0).unwrap_or(257);
    let hdist = (1..=30).rev().find(|&n| dl[n - 1] > 0).unwrap_or(1);
    let seq: Vec<usize> = ll[..hlit].iter().chain(&dl[..hdist]).copied().collect();
    // run-length code: (symbol, extra bits)
    let mut rle: Vec<(usize, usize)> = Vec::new();
    let mut i = 0;
    while i < seq.len() {
        let v = seq[i];
        let run = seq[i..].iter().take_while(|&&x| x == v).count();
        if v == 0 && run >= 11 {
            let n = run.min(138);
            rle.push((18, 7));
            i += n;
        } else if v == 0 && run >= 3 {
            rle.push((17, 3));
            i += run.min(10);
        } else if v != 0 && run >= 4 {
            rle.push((v, 0));
            let mut left = run - 1;
            while left >= 3 {
                let n = left.min(6);
                rle.push((16, 2));
                left -= n;
            }
            i += run - left;
        } else {
            rle.push((v, 0));
            i += 1;
        }
    }
    let mut cf = vec![0usize; 19];
    for &(s, _) in &rle {
        cf[s] += 1;
    }
    let cl = lengths(&cf, 7);
    let hclen = (4..=19).rev().find(|&n| cl[CL_ORDER[n - 1]] > 0).unwrap_or(4);
    14 + 3 * hclen + rle.iter().map(|&(s, e)| cl[s] + e).sum::<usize>()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn sizes_are_plausible() {
        assert!(gzip_size(b"a") <= 40);
        let text = "the same words again and again ".repeat(200);
        let n = gzip_size(text.as_bytes());
        assert!(n < 100, "{n}");
        // a multiplicative hash: it looks random but repeats; `gzip -9` makes 936 bytes of it
        let hashed: Vec<u8> = (0..4000u32).map(|i| (i.wrapping_mul(2654435761) >> 13) as u8).collect();
        let n = gzip_size(&hashed) as f64;
        assert!((n / 936.0 - 1.0).abs() < 0.03, "{n}");
    }

    #[test]
    fn close_to_gzip_on_text() {
        // `gzip -9` (Python's gzip.compress at level 9) makes 1,629 bytes of this text
        let w = ["alpha", "beta", "gamma", "delta", "epsilon", "zeta", "eta"];
        let text: String = (0..300).map(|i| format!("line {i}: the value is {} and the word is {}\n", i * i % 97, w[i % 7])).collect();
        assert_eq!(text.len(), 14157);
        let n = gzip_size(text.as_bytes()) as f64;
        assert!((n / 1629.0 - 1.0).abs() < 0.03, "{n}");
    }

    #[test]
    fn code_lengths_are_a_prefix_code() {
        let f = [5, 9, 12, 13, 16, 45, 0, 1];
        let l = lengths(&f, 15);
        let kraft: f64 = l.iter().filter(|&&x| x > 0).map(|&x| 0.5f64.powi(x as i32)).sum();
        assert!((kraft - 1.0).abs() < 1e-9, "{l:?}");
        assert_eq!(l[6], 0);
        let limited = lengths(&[1, 1, 2, 4, 8, 16, 32, 64, 128, 256, 512, 1024], 5);
        assert!(limited.iter().all(|&x| x <= 5), "{limited:?}");
    }

}
