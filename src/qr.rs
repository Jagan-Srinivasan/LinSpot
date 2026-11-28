//! Minimal, dependency-free QR code generator (QR Model 2, byte mode, ECC level M).
//! Supports versions 1-10, which covers every realistic "WIFI:" payload (SSID + password).
//! Also contains a tiny PNG encoder (grayscale, stored-deflate) so the QR can be saved to
//! a file for the desktop widgets, and the standard Wi-Fi connect string builder.

// ------------------------------------------------------------------ GF(256) / Reed-Solomon
const GF_EXP: [u8; 512] = {
    let mut t = [0u8; 512];
    let mut x = 1usize;
    let mut i = 0;
    while i < 255 {
        t[i] = x as u8;
        t[i + 255] = x as u8;
        x <<= 1;
        if x & 0x100 != 0 {
            x ^= 0x11d;
        }
        i += 1;
    }
    t
};

fn gf_mul(a: u8, b: u8) -> u8 {
    if a == 0 || b == 0 {
        0
    } else {
        // log/anti-log tables derived from GF_EXP
        let mut la = 0usize;
        while GF_EXP[la] != a {
            la += 1;
        }
        let mut lb = 0usize;
        while GF_EXP[lb] != b {
            lb += 1;
        }
        GF_EXP[la + lb]
    }
}

/// Generator polynomial for `deg` ECC codewords, coefficients high->low.
fn rs_generator(deg: usize) -> Vec<u8> {
    let mut g = vec![1u8];
    for i in 0..deg {
        let root = GF_EXP[i];
        let mut next = vec![0u8; g.len() + 1];
        for (k, &c) in g.iter().enumerate() {
            next[k] ^= gf_mul(c, root);
            next[k + 1] ^= c;
        }
        g = next;
    }
    // strip leading zero (cannot happen: monic) and reverse to high->low with leading 1
    g
}

/// Reed-Solomon ECC codewords for a data block.
fn rs_encode(data: &[u8], ecc_len: usize) -> Vec<u8> {
    let gen = rs_generator(ecc_len);
    let mut rem = vec![0u8; ecc_len];
    for &b in data {
        let factor = b ^ rem[0];
        rem.remove(0);
        rem.push(0);
        for i in 0..ecc_len {
            rem[i] ^= gf_mul(gen[ecc_len - 1 - i], factor);
        }
    }
    rem
}

// ------------------------------------------------------------------ version tables
/// Total codewords per version (1-based index, versions 1..=10).
const TOTAL_CW: [usize; 11] = [0, 26, 44, 70, 100, 134, 172, 196, 242, 292, 346];
/// ECC codewords per block, ECC level M, versions 1..=10.
const ECC_PER_BLOCK_M: [usize; 11] = [0, 10, 16, 26, 18, 24, 16, 18, 22, 22, 26];
/// Number of ECC blocks, ECC level M, versions 1..=10.
const NUM_BLOCKS_M: [usize; 11] = [0, 1, 1, 1, 2, 2, 4, 4, 4, 5, 5];
/// Alignment pattern centre coordinates (version 1 has none).
const ALIGN: [&[u32]; 11] = [
    &[],        // (unused, index 0)
    &[],        // version 1: no alignment patterns
    &[6, 18],
    &[6, 22],
    &[6, 26],
    &[6, 30],
    &[6, 34],
    &[6, 22, 38],
    &[6, 24, 42],
    &[6, 26, 46],
    &[6, 28, 50],
];

fn data_capacity(ver: usize) -> usize {
    let raw = TOTAL_CW[ver];
    let ecc_total = ECC_PER_BLOCK_M[ver] * NUM_BLOCKS_M[ver];
    let bits = (raw - ecc_total) * 8 - 4 - 8; // mode + length header
    bits / 8
}

#[derive(Clone)]
pub struct Qr {
    pub size: usize,
    /// true = dark module
    pub modules: Vec<bool>,
    /// true = function pattern (not data); used by tests, harmless to keep around
    #[cfg_attr(not(test), allow(dead_code))]
    pub func: Vec<bool>,
}

impl Qr {
    pub fn is_dark(&self, x: usize, y: usize) -> bool {
        self.modules[y * self.size + x]
    }
}

/// Encode `payload` (bytes) into a QR code, ECC level M.
pub fn encode(payload: &[u8]) -> Result<Qr, String> {
    let mut ver = 0usize;
    for v in 1..=10 {
        if payload.len() <= data_capacity(v) {
            ver = v;
            break;
        }
    }
    if ver == 0 {
        return Err(format!("payload too long for QR version 10 ({} bytes)", payload.len()));
    }

    // --- bit stream: byte mode
    let mut bits: Vec<bool> = vec![];
    let push = |v: u32, n: usize, bits: &mut Vec<bool>| {
        for i in (0..n).rev() {
            bits.push((v >> i) & 1 == 1);
        }
    };
    push(0b0100, 4, &mut bits);
    push(payload.len() as u32, 8, &mut bits);
    for &b in payload {
        push(b as u32, 8, &mut bits);
    }
    let cap_bits = (TOTAL_CW[ver] - ECC_PER_BLOCK_M[ver] * NUM_BLOCKS_M[ver]) * 8;
    for _ in 0..4.min(cap_bits.saturating_sub(bits.len())) {
        bits.push(false); // terminator
    }
    while bits.len() % 8 != 0 {
        bits.push(false);
    }
    let mut bytes: Vec<u8> = bits
        .chunks(8)
        .map(|c| c.iter().fold(0u8, |a, &b| (a << 1) | b as u8))
        .collect();
    let pad = [0xECu8, 0x11];
    let mut pi = 0;
    while bytes.len() < cap_bits / 8 {
        bytes.push(pad[pi % 2]);
        pi += 1;
    }

    // --- split into blocks, add ECC, interleave
    let n = NUM_BLOCKS_M[ver];
    let e = ECC_PER_BLOCK_M[ver];
    let raw = TOTAL_CW[ver];
    let short_len = (raw - n * e) / n; // data length of a short block
    let num_short = n - (raw - n * e) % n;
    let mut blocks: Vec<(Vec<u8>, Vec<u8>)> = vec![];
    let mut off = 0;
    for i in 0..n {
        let len = short_len + if i < num_short { 0 } else { 1 };
        let data = bytes[off..off + len].to_vec();
        off += len;
        let ecc = rs_encode(&data, e);
        blocks.push((data, ecc));
    }
    let mut final_cw: Vec<u8> = vec![];
    let long_len = short_len + 1;
    for col in 0..short_len {
        for b in &blocks {
            final_cw.push(b.0[col]);
        }
    }
    if long_len > short_len {
        for b in blocks.iter().skip(num_short) {
            final_cw.push(b.0[short_len]);
        }
    }
    for col in 0..e {
        for b in &blocks {
            final_cw.push(b.1[col]);
        }
    }
    debug_assert_eq!(final_cw.len(), raw);

    // --- build module grid
    let size = ver * 4 + 17;
    let mut modules = vec![false; size * size];
    let mut func = vec![false; size * size];
    let set = |m: &mut Vec<bool>, f: &mut [bool], x: usize, y: usize, dark: bool| {
        m[y * size + x] = dark;
        f[y * size + x] = true;
    };

    // timing patterns
    for i in 0..size {
        let dark = i % 2 == 0;
        if !func[6 * size + i] {
            set(&mut modules, &mut func, i, 6, dark);
        }
        if !func[i * size + 6] {
            set(&mut modules, &mut func, 6, i, dark);
        }
    }
    // finder patterns
    let mut finder = |cx: usize, cy: usize| {
        for dy in -4i32..=4 {
            for dx in -4i32..=4 {
                let x = cx as i32 + dx;
                let y = cy as i32 + dy;
                if x < 0 || y < 0 || x >= size as i32 || y >= size as i32 {
                    continue;
                }
                let d = dx.abs().max(dy.abs());
                set(&mut modules, &mut func, x as usize, y as usize, d != 2 && d != 4);
            }
        }
    };
    finder(3, 3);
    finder(size - 4, 3);
    finder(3, size - 4);

    // alignment patterns (skip overlaps with finders)
    let pos = ALIGN[ver];
    for &ay in pos {
        let ay = ay as usize;
        for &ax in pos {
            let ax = ax as usize;
            let near_finder = (ax < 8 && ay < 8) || (ax >= size - 8 && ay < 8) || (ax < 8 && ay >= size - 8);
            if near_finder {
                continue;
            }
            for dy in -2i32..=2 {
                for dx in -2i32..=2 {
                    let d = dx.abs().max(dy.abs());
                    set(&mut modules, &mut func, (ax as i32 + dx) as usize, (ay as i32 + dy) as usize, d != 1);
                }
            }
        }
    }

    // reserve format info areas
    for i in 0..9 {
        if !func[i * size + 8] {
            set(&mut modules, &mut func, 8, i, false);
        }
        if !func[8 * size + i] {
            set(&mut modules, &mut func, i, 8, false);
        }
    }
    for i in 0..8 {
        if !func[(size - 1 - i) * size + 8] {
            set(&mut modules, &mut func, 8, size - 1 - i, false);
        }
        if !func[8 * size + (size - 1 - i)] {
            set(&mut modules, &mut func, size - 1 - i, 8, false);
        }
    }
    // the special always-dark module below the second format copy
    set(&mut modules, &mut func, 8, size - 8, true);

    // version info (v >= 7)
    if ver >= 7 {
        let mut rem = ver as u32;
        for _ in 0..12 {
            rem = (rem << 1) ^ (if rem & 0x800 != 0 { 0x1F25 } else { 0 });
        }
        let vbits = ((ver as u32) << 12) | rem;
        for i in 0..18u32 {
            let bit = (vbits >> i) & 1 == 1;
            let a = size - 11 + (i % 3) as usize;
            let b = (i / 3) as usize;
            set(&mut modules, &mut func, a, b, bit);
            set(&mut modules, &mut func, b, a, bit);
        }
    }

    // --- place data bits (zigzag)
    let mut bi = 0usize;
    let total_bits = final_cw.len() * 8;
    let mut right = size - 1;
    while right >= 1 {
        if right == 6 {
            right = 5;
        }
        for vert in 0..size {
            for j in 0..2 {
                let x = right - j;
                let upward = (right + 1) & 2 == 0;
                let y = if upward { size - 1 - vert } else { vert };
                let idx = y * size + x;
                if !func[idx] && bi < total_bits {
                    let bit = (final_cw[bi / 8] >> (7 - (bi % 8))) & 1 == 1;
                    modules[idx] = bit;
                    bi += 1;
                }
            }
        }
        if right < 2 {
            break;
        }
        right -= 2;
    }

    // --- pick the mask with the lowest penalty score
    let mut chosen = 0u32;
    let mut chosen_score = u64::MAX;
    for m in 0..8u32 {
        let mut mm = modules.clone();
        for y in 0..size {
            for x in 0..size {
                if !func[y * size + x] && mask_bit(m, x, y) {
                    mm[y * size + x] = !mm[y * size + x];
                }
            }
        }
        let score = penalty(&mm, size);
        if score < chosen_score {
            chosen_score = score;
            chosen = m;
        }
    }
    let mut modules = modules;
    for y in 0..size {
        for x in 0..size {
            if !func[y * size + x] && mask_bit(chosen, x, y) {
                modules[y * size + x] = !modules[y * size + x];
            }
        }
    }

    // --- format info
    let mut rem = chosen;
    for _ in 0..10 {
        rem = (rem << 1) ^ (if rem & 0x200 != 0 { 0x537 } else { 0 });
    }
    let fbits = ((chosen << 10) | rem) ^ 0x5412;
    let fb = |i: u32| (fbits >> i) & 1 == 1;
    let mut place = |x: usize, y: usize, dark: bool| modules[y * size + x] = dark;
    for i in 0..6u32 {
        place(8, i as usize, fb(i));
    }
    place(8, 7, fb(6));
    place(8, 8, fb(7));
    place(7, 8, fb(8));
    for i in 9..15u32 {
        place(14 - i as usize, 8, fb(i));
    }
    for i in 0..8u32 {
        place(size - 1 - i as usize, 8, fb(i));
    }
    for i in 8..15u32 {
        place(8, size - 15 + i as usize, fb(i));
    }

    Ok(Qr { size, modules, func })
}

fn mask_bit(mask: u32, x: usize, y: usize) -> bool {
    let x = x as u32;
    let y = y as u32;
    match mask {
        0 => (x + y) % 2 == 0,
        1 => y % 2 == 0,
        2 => x % 3 == 0,
        3 => (x + y) % 3 == 0,
        4 => (x / 3 + y / 2) % 2 == 0,
        5 => (x * y) % 2 + (x * y) % 3 == 0,
        6 => ((x * y) % 2 + (x * y) % 3) % 2 == 0,
        _ => ((x + y) % 2 + (x * y) % 3) % 2 == 0,
    }
}

fn penalty(m: &[bool], size: usize) -> u64 {
    let mut score: u64 = 0;
    // rule 1: runs of 5+
    for y in 0..size {
        let mut run = 1;
        for x in 1..size {
            if m[y * size + x] == m[y * size + x - 1] {
                run += 1;
            } else {
                if run >= 5 {
                    score += 3 + (run - 5) as u64;
                }
                run = 1;
            }
        }
        if run >= 5 {
            score += 3 + (run - 5) as u64;
        }
    }
    for x in 0..size {
        let mut run = 1;
        for y in 1..size {
            if m[y * size + x] == m[(y - 1) * size + x] {
                run += 1;
            } else {
                if run >= 5 {
                    score += 3 + (run - 5) as u64;
                }
                run = 1;
            }
        }
        if run >= 5 {
            score += 3 + (run - 5) as u64;
        }
    }
    // rule 2: 2x2 blocks
    for y in 0..size - 1 {
        for x in 0..size - 1 {
            let v = m[y * size + x];
            if m[y * size + x + 1] == v && m[(y + 1) * size + x] == v && m[(y + 1) * size + x + 1] == v {
                score += 3;
            }
        }
    }
    // rule 3: finder-like patterns 1011101000 / 00001011101
    let pat1 = [true, false, true, true, true, false, true, false, false, false, false];
    let pat2 = [false, false, false, false, true, false, true, true, true, false, true];
    let check = |get: &dyn Fn(usize) -> bool| -> u64 {
        let mut s = 0;
        for i in 0.. {
            if i + 11 > size {
                break;
            }
            let w1: Vec<bool> = (0..11).map(|k| get(i + k)).collect();
            if w1 == pat1 || w1 == pat2 {
                s += 40;
            }
        }
        s
    };
    for y in 0..size {
        score += check(&|i| m[y * size + i]);
    }
    for x in 0..size {
        score += check(&|i| m[i * size + x]);
    }
    // rule 4: dark proportion
    let dark = m.iter().filter(|&&b| b).count();
    let total = size * size;
    let k = (dark as i64 * 20 - total as i64 * 10).abs() + total as i64 - 1;
    score += ((k / total as i64) - 1).max(0) as u64 * 10;
    score
}

// ------------------------------------------------------------------ PNG writer
fn crc32(data: &[u8]) -> u32 {
    let mut table = [0u32; 256];
    let mut i = 0;
    while i < 256 {
        let mut c = i as u32;
        let mut k = 0;
        while k < 8 {
            c = if c & 1 != 0 { 0xEDB88320 ^ (c >> 1) } else { c >> 1 };
            k += 1;
        }
        table[i] = c;
        i += 1;
    }
    let mut crc = 0xFFFFFFFFu32;
    for &b in data {
        crc = table[((crc ^ b as u32) & 0xFF) as usize] ^ (crc >> 8);
    }
    crc ^ 0xFFFFFFFF
}

fn adler32(data: &[u8]) -> u32 {
    let (mut a, mut b) = (1u32, 0u32);
    for &x in data {
        a = (a + x as u32) % 65521;
        b = (b + a) % 65521;
    }
    (b << 16) | a
}

fn chunk(out: &mut Vec<u8>, kind: &[u8; 4], body: &[u8]) {
    out.extend_from_slice(&(body.len() as u32).to_be_bytes());
    out.extend_from_slice(kind);
    out.extend_from_slice(body);
    out.extend_from_slice(&crc32(&[kind.as_slice(), body].concat()).to_be_bytes());
}

/// Encode `pixels` (row-major, 255 = dark) as a grayscale PNG.
pub fn png_gray(width: usize, height: usize, pixels: &[u8]) -> Result<Vec<u8>, String> {
    if pixels.len() != width * height {
        return Err("pixel buffer size mismatch".into());
    }
    // raw scanlines with filter byte 0
    let mut raw = Vec::with_capacity((width + 1) * height);
    for y in 0..height {
        raw.push(0);
        raw.extend_from_slice(&pixels[y * width..(y + 1) * width]);
    }
    // zlib with stored deflate blocks
    let mut z = vec![0x78, 0x01];
    let mut off = 0;
    while off < raw.len() {
        let take = (raw.len() - off).min(65535);
        let last = off + take >= raw.len();
        z.push(if last { 1 } else { 0 });
        z.extend_from_slice(&(take as u16).to_le_bytes());
        z.extend_from_slice(&(!(take as u16)).to_le_bytes());
        z.extend_from_slice(&raw[off..off + take]);
        off += take;
    }
    z.extend_from_slice(&adler32(&raw).to_be_bytes());

    let mut out = vec![0x89, b'P', b'N', b'G', 0x0D, 0x0A, 0x1A, 0x0A];
    let mut ihdr = Vec::new();
    ihdr.extend_from_slice(&(width as u32).to_be_bytes());
    ihdr.extend_from_slice(&(height as u32).to_be_bytes());
    ihdr.extend_from_slice(&[8, 0, 0, 0, 0]); // 8-bit grayscale
    chunk(&mut out, b"IHDR", &ihdr);
    chunk(&mut out, b"IDAT", &z);
    chunk(&mut out, b"IEND", &[]);
    Ok(out)
}

/// Render a QR into a grayscale pixel buffer with a 4-module quiet zone.
pub fn qr_pixels(qr: &Qr, scale: usize) -> Vec<u8> {
    let quiet = 4;
    let dim = (qr.size + quiet * 2) * scale;
    let mut px = vec![255u8; dim * dim];
    for y in 0..qr.size {
        for x in 0..qr.size {
            if qr.is_dark(x, y) {
                for sy in 0..scale {
                    for sx in 0..scale {
                        let py = (y + quiet) * scale + sy;
                        let pxx = (x + quiet) * scale + sx;
                        px[py * dim + pxx] = 0;
                    }
                }
            }
        }
    }
    px
}

/// Save a QR PNG to `path` at the given module scale.
pub fn save_png(qr: &Qr, scale: usize, path: &str) -> Result<(), String> {
    let px = qr_pixels(qr, scale);
    let png = png_gray((qr.size + 8) * scale, (qr.size + 8) * scale, &px)?;
    std::fs::write(path, png).map_err(|e| e.to_string())
}

// ------------------------------------------------------------------ Wi-Fi payload
fn esc(s: &str) -> String {
    s.replace('\\', "\\\\").replace(';', "\\;").replace(',', "\\,").replace(':', "\\:").replace('"', "\\\"")
}

/// Standard "WIFI:" string that Android/iOS/Windows cameras parse to join a network.
pub fn wifi_string(ssid: &str, password: &str) -> String {
    format!("WIFI:T:WPA;S:{};P:{};;", esc(ssid), esc(password))
}

/// Convenience: encode a Wi-Fi QR.
pub fn wifi_qr(ssid: &str, password: &str) -> Result<Qr, String> {
    encode(wifi_string(ssid, password).as_bytes())
}

// ------------------------------------------------------------------ tests

#[cfg(test)]
mod tests {
    use super::*;

    /// Re-extract the codewords from a finished grid exactly like a decoder does
    /// (read format, unmask, zigzag read) and verify the Reed-Solomon codes.
    fn self_check(qr: &Qr, expect: &[u8]) {
        let size = qr.size;
        let ver = (size - 17) / 4;
        assert!((1..=10).contains(&ver), "version {}", ver);

        // read format (first copy)
        const XS: [usize; 15] = [8, 8, 8, 8, 8, 8, 8, 8, 7, 5, 4, 3, 2, 1, 0];
        const YS: [usize; 15] = [0, 1, 2, 3, 4, 5, 7, 8, 8, 8, 8, 8, 8, 8, 8];
        let mut fmt = 0u16;
        for i in (0..15).rev() {
            fmt = (fmt << 1) | qr.is_dark(XS[i], YS[i]) as u16;
        }
        let placed = fmt;
        fmt ^= 0x5412;
        // The mask index we encoded (brute force: recompute all 8 candidates)
        let mut mask = None;
        for m in 0..8u32 {
            let mut rem = m;
            for _ in 0..10 {
                rem = (rem << 1) ^ (if rem & 0x200 != 0 { 0x537 } else { 0 });
            }
            let cand = ((m << 10) | rem) ^ 0x5412;
            if cand & 0x7FFF == (placed as u32) & 0x7FFF {
                mask = Some(m);
            }
        }
        let mask = mask.expect("format did not match any candidate");
        // ECC level must be M (format bits 0)
        assert_eq!(fmt >> 13, 0, "ecc level bits");

        // zigzag read
        let total_cw = TOTAL_CW[ver];
        let mut bits: Vec<bool> = vec![];
        let mut right = size - 1;
        while right >= 1 {
            if right == 6 {
                right = 5;
            }
            for vert in 0..size {
                for j in 0..2 {
                    let x = right - j;
                    let upward = (right + 1) & 2 == 0;
                    let y = if upward { size - 1 - vert } else { vert };
                    if x >= size {
                        continue;
                    }
                    if qr.func[y * size + x] {
                        continue; // function pattern: a decoder skips it
                    }
                    let dark = qr.is_dark(x, y);
                    let m = mask_bit(mask, x, y);
                    bits.push(dark != m);
                }
            }
            if right < 2 {
                break;
            }
            right -= 2;
        }
        assert!(bits.len() >= total_cw * 8, "too few raw bits: {} < {}", bits.len(), total_cw * 8);
        bits.truncate(total_cw * 8); // remainder bits (if any) stay light
        let raw: Vec<u8> = bits.chunks(8).map(|c| c.iter().fold(0u8, |a, &b| (a << 1) | b as u8)).collect();

        // de-interleave
        let n = NUM_BLOCKS_M[ver];
        let e = ECC_PER_BLOCK_M[ver];
        let short_len = (total_cw - n * e) / n;
        let num_short = n - (total_cw - n * e) % n;
        let mut blocks: Vec<(Vec<u8>, Vec<u8>)> = vec![(vec![], vec![]); n];
        let mut k = 0;
        for col in 0..short_len {
            for b in blocks.iter_mut() {
                b.0.push(raw[k]);
                k += 1;
            }
        }
        for b in blocks.iter_mut().skip(num_short) {
            b.0.push(raw[k]);
            k += 1;
        }
        for col in 0..e {
            for b in blocks.iter_mut() {
                b.1.push(raw[k]);
                k += 1;
            }
        }
        assert_eq!(k, total_cw);

        // RS decode: re-encode data and compare ECC
        let mut payload: Vec<u8> = vec![];
        for (d, ecc) in &blocks {
            let check = rs_encode(d, e);
            assert_eq!(&check, ecc, "ECC mismatch (data may be corrupt)");
            payload.extend_from_slice(d);
        }
        // strip padding to compare against the expected payload
        // rebuild the expected bit stream (mode + count + payload + terminator + padding)
        let cap_bits = (total_cw - NUM_BLOCKS_M[ver] * ECC_PER_BLOCK_M[ver]) * 8;
        let mut want: Vec<bool> = vec![];
        for b in [(0b0100u32, 4usize), (expect.len() as u32, 8)] {
            for i in (0..b.1).rev() {
                want.push((b.0 >> i) & 1 == 1);
            }
        }
        for &b in expect {
            for i in (0..8).rev() {
                want.push((b >> i) & 1 == 1);
            }
        }
        for _ in 0..4.min(cap_bits.saturating_sub(want.len())) {
            want.push(false);
        }
        while want.len() % 8 != 0 {
            want.push(false);
        }
        let mut want_bytes: Vec<u8> = want.chunks(8).map(|c| c.iter().fold(0u8, |a, &b| (a << 1) | b as u8)).collect();
        let mut pi = 0;
        while want_bytes.len() < cap_bits / 8 {
            want_bytes.push([0xECu8, 0x11][pi % 2]);
            pi += 1;
        }
        assert_eq!(payload.len(), want_bytes.len(), "codeword count");
        assert_eq!(payload, want_bytes, "codewords mismatch");
    }

    #[test]
    fn roundtrip_all_versions() {
        // one payload per version (sized to land exactly on it)
        for ver in 1..=10usize {
            let s = "A".repeat(data_capacity(ver));
            let q = encode(s.as_bytes()).unwrap();
            assert_eq!((q.size - 17) / 4, ver, "wrong version chosen");
            self_check(&q, s.as_bytes());
        }
        // real Wi-Fi payloads with special characters
        for (ssid, pass) in [
            ("LinSpot", "abc123xy"),
            ("S", "12345678"),
            ("My Home Net", "p4ssw0rd-with\"special;chars:and,commas"),
            ("this-is-a-very-long-network-name-exactly-32b", "a-long-password-with-63-characters-0123456789012345678901234"),
            ("\u{fc}n\u{ef}cod\u{e9}", "p\u{e4}ssw\u{f6}rd12"),
        ] {
            let s = wifi_string(ssid, pass);
            let q = encode(s.as_bytes()).unwrap();
            self_check(&q, s.as_bytes());
        }
    }
}




#[cfg(test)]
mod rs_compare {
    use super::*;

    fn nayuki_multiply(x: u8, y: u8) -> u8 {
        let (mut z, mut x, mut y) = (0u32, x as u32, y as u32);
        for i in (0..8).rev() {
            z = (z << 1) ^ ((z >> 7) * 0x11D);
            z ^= ((y >> i) & 1) * x;
        }
        z as u8
    }

    fn nayuki_divisor(degree: usize) -> Vec<u8> {
        let mut result = vec![0u8; degree];
        result[degree - 1] = 1;
        let mut root = 1u8;
        for _ in 0..degree {
            for j in 0..result.len() {
                result[j] = nayuki_multiply(result[j], root);
                if j + 1 < result.len() {
                    result[j] ^= result[j + 1];
                }
            }
            root = nayuki_multiply(root, 0x02);
        }
        result
    }

    fn nayuki_remainder(data: &[u8], divisor: &[u8]) -> Vec<u8> {
        let mut result = vec![0u8; divisor.len()];
        for &b in data {
            let factor = b ^ result[0];
            result.remove(0);
            result.push(0);
            for i in 0..result.len() {
                result[i] ^= nayuki_multiply(divisor[i], factor);
            }
        }
        result
    }

    #[test]
    fn rs_matches_reference() {
        for deg in [7usize, 10, 16, 26, 18, 24, 22] {
            let mine = rs_generator(deg);
            // mine is low->high with leading 1 at the end; theirs is high->low without leading
            let mine_hi_lo: Vec<u8> = mine[..deg].iter().rev().copied().collect();
            let theirs = nayuki_divisor(deg);
            assert_eq!(mine_hi_lo, theirs, "generator degree {}", deg);
        }
        let data: Vec<u8> = (0..60u8).collect();
        for deg in [7usize, 10, 16, 26] {
            let mine = rs_encode(&data, deg);
            let theirs = nayuki_remainder(&data, &nayuki_divisor(deg));
            assert_eq!(mine, theirs, "remainder degree {}", deg);
        }
    }
}

