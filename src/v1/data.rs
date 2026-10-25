//! RUNVGA v1 data: the RESOURCE.DAT archive (or loose `base.nnn` files), the
//! level loader, PCX pictures, the font and the inventory file.
//! See docs/v1-runvga/01-formats.md.

use std::collections::BTreeMap;
use std::path::Path;

use crate::Error;
use crate::memory::Memory;

/// DS addresses of the level tables.
pub mod ds {
    pub const LIMITMOV: usize = 0x30fe;
    pub const LIMIT: usize = 0x3392;
    pub const THINGS: usize = 0x39a5;
    pub const DOORS: usize = 0x2de4;
    /// 80 x 250 bytes: messages 0..5, level texts 6..49, inventory texts 50..79.
    pub const TEXTS: usize = 0x41bb;
    pub const ACTION_OFFS: usize = 0x2eac;
    pub const ANIMS: usize = 0x94b0;
    pub const GFX_NAMES: usize = 0xa48;
    pub const ACINVTH: usize = 0x300e;
    pub const COORANIM: usize = 0xb80;
    pub const FILE_NAMES: usize = 0xa7c;
    pub const ACINVINV: usize = 0x2f14;
    pub const FILE_TAB: usize = 0x9a8;
    pub const GFX_TAB: usize = 0x988;
    pub const FRAME_W: usize = 0x980;
    pub const FRAME_H: usize = 0x982;
    pub const FRAMES: usize = 0x96b0;
    pub const CURSOR_DEF: usize = 0x96a4;
}

pub const THING_SIZE: usize = 0x45;
pub const TEXT_SIZE: usize = 250;
pub const KEY_TEXT: u8 = 0x0f;
pub const KEY_THING: u8 = 0x0d;
pub const KEY_FILE: u8 = 0x19;
pub const KEY_GFX: u8 = 0x2d;
pub const KEY_INV_TEXT: u8 = 0x1e;
pub const KEY_INV_NAME: u8 = 0x20;
/// Size of an inventory record (text 250 + name 30).
pub const INV_RECORD: usize = 0x118;
/// Size of the font (entry 998).
pub const FONT_SIZE: usize = 0x32cd;

/// Where a v1 game's data comes from (`RUNVGA -f<archive>` or `-n<base>`).
pub enum Source {
    Archive(Vec<u8>),
    /// upper-case file name -> contents; levels are `<base>.nnn`
    Files {
        base: String,
        files: BTreeMap<String, Vec<u8>>,
    },
}

fn le32(b: &[u8], o: usize) -> usize {
    b.get(o..o + 4).map(|s| u32::from_le_bytes([s[0], s[1], s[2], s[3]]) as usize).unwrap_or(0)
}

impl Source {
    /// Is `bytes` a RESOURCE.DAT-style archive (1000-entry table)?
    pub fn is_archive(bytes: &[u8]) -> bool {
        if bytes.len() < 4000 + 0x7000 {
            return false;
        }
        let first = le32(bytes, 0);
        // level 0 must exist and follow the table directly; v2 archives start with a smaller table
        first == 4000 && (0..1000).all(|i| le32(bytes, i * 4) < bytes.len())
    }

    /// Open a directory or file holding a v1 game, if it is one.
    pub fn open(path: &Path) -> Option<Source> {
        let read = |p: &Path| std::fs::read(p).ok();
        if path.is_file() {
            let b = read(path)?;
            return Source::is_archive(&b).then_some(Source::Archive(b));
        }
        let mut files = BTreeMap::new();
        for entry in std::fs::read_dir(path).ok()?.flatten() {
            if entry.path().is_file() {
                files.insert(entry.file_name().to_string_lossy().to_uppercase(), read(&entry.path())?);
            }
        }
        if let Some((_, b)) = files.iter().find(|(_, b)| Source::is_archive(b)) {
            return Some(Source::Archive(b.clone()));
        }
        // -n mode: a <base>.000 level 0 next to <base>.998 and <base>.999
        let base = files.keys().find_map(|n| n.strip_suffix(".000").map(str::to_string))?;
        (files.contains_key(&format!("{base}.998")) && files.contains_key(&format!("{base}.999")))
            .then_some(Source::Files { base, files })
    }

    /// Entry n (levels 0..997, 998 font, 999 inventory): (bytes, offset).
    pub fn entry(&self, n: usize) -> Option<(&[u8], usize)> {
        match self {
            Source::Archive(b) => match le32(b, n * 4) {
                0 => None,
                off => Some((&b[..], off)),
            },
            Source::Files { base, files } => files.get(&format!("{base}.{n:03}")).map(|b| (&b[..], 0)),
        }
    }

    pub fn is_archive_mode(&self) -> bool {
        matches!(self, Source::Archive(_))
    }

    /// A resource by FILE/GFX table entry: archive offset and size, or (-n) its name.
    pub fn resource(&self, name: &[u8], offset: usize, size: usize) -> Option<Vec<u8>> {
        match self {
            Source::Archive(b) => (size > 0 && offset + size <= b.len()).then(|| b[offset..offset + size].to_vec()),
            Source::Files { files, .. } => files.get(&String::from_utf8_lossy(name).to_uppercase()).cloned(),
        }
    }
}

/// Load level `n` into memory (0x1e23 / 0x1895). Returns the 5000-word code.
pub fn load_level(m: &mut Memory, src: &Source, n: usize) -> Result<Vec<i16>, Error> {
    let (bytes, off) = src.entry(n).ok_or_else(|| Error::new(format!("No encuentro el nivel '{n}'.")))?;
    let need = if src.is_archive_mode() { 0x7000 } else { 0x6f40 };
    if off + need > bytes.len() {
        return Err(Error::new(format!("Error al intentar leer el nivel '{n}'.")));
    }
    let b = &bytes[off..];
    let i16at = |o: usize| i16::from_le_bytes([b[o], b[o + 1]]) as i32;
    m.write(ds::LIMITMOV, &b[0..0x294]);
    m.write(ds::LIMIT, &b[0x294..0x438]);
    m.write(ds::THINGS, &b[0x438..0xc4e]);
    m.write(ds::DOORS, &b[0xc4e..0xd16]);
    // Messages 0..5 are decoded at once and copied only if non-empty.
    for i in 0..6 {
        let raw = &b[0xd16 + i * TEXT_SIZE..0xd16 + (i + 1) * TEXT_SIZE];
        let decoded: Vec<u8> = raw.iter().take_while(|&&c| c != 0).map(|c| c.wrapping_sub(KEY_TEXT)).collect();
        // strcpy: up to the first (decoded) NUL
        let mut msg: Vec<u8> = decoded.into_iter().take_while(|&c| c != 0).collect();
        if !msg.is_empty() {
            msg.push(0);
            m.write(ds::TEXTS + i * TEXT_SIZE, &msg[..msg.len().min(TEXT_SIZE)]);
        }
    }
    // texts 6..49 follow the six messages (0xd16 + 6 * 250 = 0x12f2)
    m.write(ds::TEXTS + 6 * TEXT_SIZE, &b[0x12f2..0x12f2 + 44 * TEXT_SIZE]);
    let code = (0..5000).map(|i| i16::from_le_bytes([b[0x3dea + 2 * i], b[0x3deb + 2 * i]])).collect();
    m.write(ds::ACTION_OFFS, &b[0x64fa..0x655e]);
    m.write(ds::ANIMS, &b[0x655e..0x6752]);
    m.write(ds::GFX_NAMES, &b[0x6752..0x6786]);
    m.write(ds::ACINVTH, &b[0x6786..0x6876]);
    m.write(ds::COORANIM, &b[0x6876..0x6b96]);
    m.write(ds::FILE_NAMES, &b[0x6b96..0x6c9a]);
    // ACINVINV: a record is replaced only if its flag byte is set.
    for i in 0..50 {
        let r = &b[0x6c9a + 5 * i..0x6c9a + 5 * i + 5];
        if r[0] != 0 {
            m.write(ds::ACINVINV + 5 * i, r);
        }
    }
    let (w, h) = (i16at(0x6d94), i16at(0x6d96));
    if w >= 0 {
        m.set16(ds::FRAME_W, w);
    }
    if h >= 0 {
        m.set16(ds::FRAME_H, h);
    }
    // QUIRK: presence of the player-animation block is a 1-byte read into a
    // word whose high byte is left over from the previous (height) read.
    let flag = (b[0x6d98] as i32) | (b[0x6d97] as i32) << 8;
    if (flag as i16) >= 0 {
        m.set8(0x9a, b[0x6d98] as i32);
        for (k, &a) in [0x9b, 0x9c, 0x9e, 0x9f, 0x9d, 0xa1, 0xa2, 0xa3, 0xa4, 0xa5, 0xa6].iter().enumerate() {
            m.set8(a, b[0x6d99 + k] as i32);
        }
        m.write(ds::FRAMES, &b[0x6da4..0x6da4 + 400]);
    }
    let cur = i16at(0x6f34);
    if cur >= 0 {
        m.write(ds::CURSOR_DEF, &b[0x6f34..0x6f40]);
        // {srcx (also the presence flag), srcy, w, h, hotx, hoty}
        let c = |k: usize| i16at(0x6f34 + 2 * k);
        for (k, a) in [0x92ac, 0x92ae, 0x92bc, 0x92be, 0x92c4, 0x92c6].into_iter().enumerate() {
            m.set16(a, c(k));
        }
        m.set16(0x92b8, m.i16(0x92b4) - 1);
    }
    if src.is_archive_mode() {
        m.write(ds::FILE_TAB, &b[0x6f40..0x6fe0]);
        m.write(ds::GFX_TAB, &b[0x6fe0..0x7000]);
    }
    decrypt_strings(m);
    Ok(code)
}

/// 0xc3a8: decode the level strings in place.
fn decrypt_strings(m: &mut Memory) {
    let mut run = |a: usize, len: usize, key: u8| {
        for i in 0..len {
            let c = m.u8(a + i);
            if c == 0 {
                break;
            }
            m.set8(a + i, c - key as i32);
        }
    };
    for i in 6..50 {
        run(ds::TEXTS + i * TEXT_SIZE, TEXT_SIZE, KEY_TEXT);
    }
    for t in 0..25 {
        run(ds::THINGS + t * THING_SIZE + 0x26, 15, KEY_THING);
    }
    for i in 0..20 {
        run(ds::FILE_NAMES + i * 13, 13, KEY_FILE);
    }
    for i in 0..4 {
        run(ds::GFX_NAMES + i * 13, 13, KEY_GFX);
    }
}

/// (name, archive offset, size) of FILE entry n.
pub fn file_entry(m: &Memory, n: usize) -> (Vec<u8>, usize, usize) {
    (
        m.cstr(ds::FILE_NAMES + n * 13, 13),
        m.u32(ds::FILE_TAB + n * 8) as usize,
        m.u32(ds::FILE_TAB + n * 8 + 4) as usize,
    )
}

/// (name, archive offset, size) of graphics slot n.
pub fn gfx_entry(m: &Memory, n: usize) -> (Vec<u8>, usize, usize) {
    (m.cstr(ds::GFX_NAMES + n * 13, 13), m.u32(ds::GFX_TAB + n * 8) as usize, m.u32(ds::GFX_TAB + n * 8 + 4) as usize)
}

/// A 320x200 picture and its 8-bit palette, decoded like 0xa3e: runs (byte
/// > 0xC0) may cross row ends, the palette follows the pixel data.
pub fn decode_pcx(bytes: &[u8]) -> Result<(Vec<u8>, Vec<u8>), u32> {
    if bytes.first() != Some(&0x0a) {
        return Err(0x62);
    }
    let w16 = |o: usize| bytes.get(o..o + 2).map(|s| u16::from_le_bytes([s[0], s[1]]) as i32).unwrap_or(0);
    if bytes.get(3) != Some(&8) || w16(8) + 1 != 320 || w16(10) + 1 != 200 {
        return Err(6);
    }
    let mut pix = vec![0u8; 64000];
    let mut p = 128;
    let mut out = 0usize;
    let mut next = || {
        let c = bytes.get(p).copied().unwrap_or(0);
        p += 1;
        c
    };
    for _ in 0..200 {
        let mut count = 320i32;
        while count > 0 {
            let c = next();
            if c > 0xc0 {
                let n = (c & 0x3f) as usize;
                let v = next();
                for _ in 0..n {
                    if out < pix.len() {
                        pix[out] = v;
                    }
                    out += 1;
                }
                count -= n as i32;
            } else {
                if out < pix.len() {
                    pix[out] = c;
                }
                out += 1;
                count -= 1;
            }
        }
    }
    let _marker = next();
    let pal = (0..768).map(|_| next()).collect();
    Ok((pix, pal))
}

/// The v1 font: 255 glyphs x 51 bytes, (w << 4) | h, then 4-bit pixels.
pub struct Font {
    glyphs: Vec<u8>,
}

impl Font {
    pub fn new(bytes: &[u8]) -> Font {
        let mut glyphs = bytes.to_vec();
        glyphs.resize(255 * 51, 0);
        Font { glyphs }
    }

    pub fn size(&self, ch: u8) -> (i32, i32) {
        if ch == 255 {
            return (0, 0);
        }
        let b = self.glyphs[ch as usize * 51];
        ((b >> 4) as i32, (b & 15) as i32)
    }

    /// Advance of a character: width - 1.
    pub fn advance(&self, ch: u8) -> i32 {
        (self.size(ch).0 - 1).max(0)
    }

    pub fn width(&self, s: &[u8]) -> i32 {
        s.iter().map(|&c| self.advance(c)).sum()
    }

    /// Draw one glyph (0x910): 1 = colour, 2 = border, others transparent.
    pub fn glyph(&self, dst: &mut [u8], x: i32, y: i32, ch: u8, col: u8, border: u8) {
        let (w, h) = self.size(ch);
        let g = &self.glyphs[ch as usize * 51 + 1..];
        let mut bit = 0usize;
        for r in 0..h {
            for c in 0..w {
                let byte = g.get(bit / 2).copied().unwrap_or(0);
                let v = if bit.is_multiple_of(2) { byte >> 4 } else { byte & 15 };
                bit += 1;
                if v != 1 && v != 2 {
                    continue;
                }
                let a = (y + r) * 320 + x + c;
                if (0..64000).contains(&a) {
                    dst[a as usize] = if v == 1 { col } else { border };
                }
            }
        }
    }

    /// DrawText (0x6d15).
    pub fn text(&self, dst: &mut [u8], mut x: i32, y: i32, s: &[u8], col: u8, border: u8) {
        for &c in s {
            self.glyph(dst, x, y, c, col, border);
            x += self.advance(c);
        }
    }
}

/// Inventory record `obj` (entry 999): (text, name), decoded.
pub fn inventory_record(src: &Source, obj: i32) -> Option<(Vec<u8>, Vec<u8>)> {
    let (bytes, off) = src.entry(999)?;
    let at = off + (obj.max(0) as usize) * INV_RECORD;
    let rec = bytes.get(at..at + INV_RECORD)?;
    let dec = |s: &[u8], key: u8| s.iter().take_while(|&&c| c != 0).map(|c| c.wrapping_sub(key)).collect::<Vec<u8>>();
    Some((dec(&rec[..250], KEY_INV_TEXT), dec(&rec[250..280], KEY_INV_NAME)))
}
