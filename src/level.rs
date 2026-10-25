//! Loading of compiled level files into emulated DS memory, replicating the
//! block sequence of RUNDAT.EXE's level loader (0x24ec).

use crate::Error;
use crate::dat::{ByteReader, file_type};
use crate::memory::{Memory, THING_SIZE, ds};

pub const KEY_GFX: u8 = 0x2d;
pub const KEY_FILE: u8 = 0x19;
pub const KEY_TEXT: u8 = 0x0f;
pub const KEY_THING: u8 = 0x0d;

enum Block {
    To(usize, usize),
    Code,
    Skip,
}

const LEVEL_BLOCKS: [Block; 20] = [
    Block::To(ds::GFX_NAMES, 0x34),
    Block::Skip,
    Block::To(ds::FILE_NAMES, 0x104),
    Block::To(ds::LIMITMOV, 0x1b80),
    Block::Skip,
    Block::To(ds::LIMIT, 0x230),
    Block::To(ds::THINGS, 0xac8),
    Block::To(ds::THING_ORDER, 0x1e),
    Block::Skip,
    Block::To(ds::DOORS, 0xc8),
    Block::To(ds::TEXT_POOL, 0x61a8),
    Block::Skip,
    Block::To(ds::TEXT_OFFS, 0x1f4),
    Block::Code,
    Block::To(ds::ACTION_OFFS, 0x64),
    Block::To(ds::ANIMS, 0x1f4),
    Block::Skip,
    Block::To(ds::ACINVTH, 0xf0),
    Block::To(ds::COORANIM, 0x3e8),
    Block::To(ds::SELECTEXT, 0x258),
];

/// LOADDATA kinds -> (destination, size).
pub const DATA_KINDS: [(usize, usize); 7] = [
    (ds::LIMIT, 0x230),
    (ds::LIMITMOV, 0x1b80),
    (ds::SHADOWMOV, 0xf0),
    (ds::SCALEMOV, 0x168),
    (ds::SELECTEXT, 0x258),
    (ds::COORANIM, 0x3e8),
    (ds::DEFFRAMES, 0xa28),
];

/// The compiled-level layouts written by the different ED.EXE versions.
#[derive(Clone, Copy, PartialEq, Eq, Debug, Default)]
pub enum LevelFormat {
    /// Shareware v2.0 (the demo): single filler bytes between some blocks.
    #[default]
    Shareware,
    /// Registered v2.0 (RUNDAT.EXE v1.0, e.g. El Dios de Nauru): no filler bytes.
    Registered,
    /// Registered v2.0 with RUNDAT.EXE v1.02 (e.g. Dadton): no filler bytes,
    /// 12-byte animation records.
    V102,
    /// Shareware v2.02 with RUNDAT.EXE v1.03 (the demo on PCManía 67): filler
    /// bytes like the shareware, 12-byte animation records, the v1.02
    /// commands, and 16-bit program counters.
    SharewareV103,
}

impl LevelFormat {
    /// In detection order: the v1.02 layouts must be tried before the ones
    /// they extend, since old-layout parses of new data can still fit.
    pub const ALL: [LevelFormat; 4] =
        [LevelFormat::SharewareV103, LevelFormat::Shareware, LevelFormat::Registered, LevelFormat::V102];

    fn fillers(self) -> bool {
        matches!(self, LevelFormat::Shareware | LevelFormat::SharewareV103)
    }

    /// Size of one ANIMATION record.
    pub fn anim_size(self) -> usize {
        if self.is_v102() { 12 } else { 10 }
    }

    /// Made for the v1.02 runtime or later (skip key, PUTXY, SEEANIM...).
    pub fn is_v102(self) -> bool {
        matches!(self, LevelFormat::V102 | LevelFormat::SharewareV103)
    }

    /// RUNDAT v1.03: scripts' program counters are 16-bit words.
    pub fn wide_pc(self) -> bool {
        self == LevelFormat::SharewareV103
    }
}

pub struct LoadedLevel {
    /// 5000 words of bytecode.
    pub code: Vec<i16>,
    /// The ANIMATION table as stored (50 records of [`LevelFormat::anim_size`] bytes).
    pub anims: Vec<u8>,
    /// Number of bytes consumed from the start of the file.
    pub end: usize,
    /// Some RLE block did not end on a token boundary (wrong [`LevelFormat`]).
    pub overrun: bool,
}

/// Load the level at `offset` of `bytes` into memory.  `packed` is false for
/// loose NIVEL.nnn files (RUNFILES.EXE), which lack the trailing resource
/// offset tables.  Strings are decrypted in place.
pub fn load_level(
    mem: &mut Memory,
    bytes: &[u8],
    offset: usize,
    packed: bool,
    format: LevelFormat,
) -> Result<LoadedLevel, Error> {
    let mut r = ByteReader::new(bytes, offset);
    let h = r.header()?;
    if h.major < 2 {
        return Err(Error::new(
            "El nivel que se intenta cargar no tiene el formato correcto (versión anterior de ED.EXE)",
        ));
    }
    if h.kind != file_type::LEVEL {
        return Err(Error::new("Archivo 'NIVEL.???' no es del tipo correcto."));
    }
    let mut code = vec![0i16; 5000];
    let mut anims = Vec::new();
    for b in &LEVEL_BLOCKS {
        match *b {
            Block::Skip => {
                if format.fillers() {
                    r.byte()?;
                }
            }
            Block::To(ds::ANIMS, _) => {
                anims = r.rle(50 * format.anim_size())?;
                for i in 0..50 {
                    let rec = &anims[i * format.anim_size()..];
                    mem.write(ds::ANIMS + i * 10, &rec[..10]);
                }
            }
            Block::Code => {
                let data = r.rle(0x2710)?;
                for (i, w) in code.iter_mut().enumerate() {
                    *w = i16::from_le_bytes([data[i * 2], data[i * 2 + 1]]);
                }
            }
            Block::To(dst, size) => {
                let data = r.rle(size)?;
                mem.write(dst, &data);
            }
        }
    }
    if format.fillers() {
        r.byte()?; // the skip after SELECTEXT
    }
    // Five fixed messages (slot 1 NOACINVINV, 2 NOACINVOBJ, 4 YESACINVINV).
    for i in 0..5 {
        let mut m = r.rle(250)?;
        let mut n = 0;
        while n < 250 && m[n] != 0 {
            m[n] = m[n].wrapping_sub(KEY_TEXT);
            n += 1;
        }
        if m[0] == 0 {
            continue; // empty: the original keeps the previous/default message
        }
        let end = m.iter().position(|&c| c == 0).map(|e| e + 1).unwrap_or(249);
        mem.write(ds::MESSAGES + i * 250, &m[..end]);
    }
    let v = r.rle(0x168)?;
    mem.write(ds::SCALEMOV, &v);
    let v = r.rle(0xf0)?;
    mem.write(ds::SHADOWMOV, &v);
    let v = r.rle(0xa28)?;
    mem.write(ds::DEFFRAMES, &v);
    if packed {
        let t = r.raw(0xa0)?;
        mem.write(ds::FILE_TAB, t);
        let t = r.raw(0x20)?;
        mem.write(ds::GFX_TAB, t);
    } else {
        mem.fill(ds::FILE_TAB, 0xa0, 0);
        mem.fill(ds::GFX_TAB, 0x20, 0);
    }
    decrypt_level_strings(mem);
    for i in 0..25 {
        mem.set8(ds::DRAWANIM_TAB + i * 20 + 0xd, 0);
    }
    Ok(LoadedLevel { code, anims, end: r.pos, overrun: r.overrun })
}

/// Work out which [`LevelFormat`] a game's levels use: the one under which
/// every level parses with each RLE block ending on a token boundary and, for
/// packed games, every resource offset table points inside the archive (for
/// loose files, each file is consumed exactly).
pub fn detect_format<'a>(levels: impl Iterator<Item = (&'a [u8], usize)> + Clone, packed: bool) -> LevelFormat {
    let fits = |format: LevelFormat| {
        levels.clone().all(|(bytes, off)| {
            let mut mem = Memory::new();
            let Ok(l) = load_level(&mut mem, bytes, off, packed, format) else { return false };
            if l.overrun {
                return false;
            }
            if !packed {
                return l.end == bytes.len();
            }
            let tables = (0..20).map(|i| file_entry(&mem, i)).chain((0..4).map(|i| gfx_entry(&mem, i)));
            tables
                .into_iter()
                .all(|e| e.size == 0 || e.offset.checked_add(e.size).is_some_and(|end| end <= bytes.len()))
        })
    };
    LevelFormat::ALL.into_iter().find(|&f| fits(f)).unwrap_or_default()
}

fn decrypt_run(mem: &mut Memory, addr: usize, len: usize, key: u8) {
    for i in 0..len {
        let c = mem.u8(addr + i);
        if c == 0 {
            break;
        }
        mem.set8(addr + i, c - key as i32);
    }
}

fn decrypt_level_strings(mem: &mut Memory) {
    for i in 0..4 {
        decrypt_run(mem, ds::GFX_NAMES + i * 13, 13, KEY_GFX);
    }
    for i in 0..20 {
        decrypt_run(mem, ds::FILE_NAMES + i * 13, 13, KEY_FILE);
    }
    for t in 0..30 {
        decrypt_run(mem, ds::THINGS + t * THING_SIZE + 0x31, 15, KEY_THING);
    }
    for b in &mut mem.bytes[ds::TEXT_POOL..ds::TEXT_POOL + 25000] {
        *b = b.wrapping_sub(KEY_TEXT);
    }
}

/// Read a LOADDATA file (header type 10+kind) into memory.
pub fn load_data_file(mem: &mut Memory, bytes: &[u8], offset: usize, kind: usize) -> Result<(), Error> {
    let mut r = ByteReader::new(bytes, offset);
    let h = r.header()?;
    let &(dst, size) = DATA_KINDS.get(kind).ok_or_else(|| Error::new("LOADDATA: tipo de datos no válido"))?;
    if h.kind as usize != kind + 10 {
        return Err(Error::new("Archivo de datos del tipo incorrecto"));
    }
    let v = r.rle(size)?;
    mem.write(dst, &v);
    Ok(())
}

pub fn text_string(mem: &Memory, id: i32) -> Option<Vec<u8>> {
    if !(0..250).contains(&id) {
        return None;
    }
    let off = mem.i16(ds::TEXT_OFFS + id as usize * 2);
    if !(0..25000).contains(&off) {
        return None;
    }
    Some(mem.cstr(ds::TEXT_POOL + off as usize, 25000 - off as usize))
}

#[derive(Clone, Debug)]
pub struct FileEntry {
    pub name: Vec<u8>,
    pub offset: usize,
    pub size: usize,
}

pub fn file_entry(mem: &Memory, n: usize) -> FileEntry {
    FileEntry {
        name: mem.cstr(ds::FILE_NAMES + n * 13, 13),
        offset: mem.u32(ds::FILE_TAB + n * 8) as usize,
        size: mem.u32(ds::FILE_TAB + n * 8 + 4) as usize,
    }
}

pub fn gfx_entry(mem: &Memory, n: usize) -> FileEntry {
    FileEntry {
        name: mem.cstr(ds::GFX_NAMES + n * 13, 13),
        offset: mem.u32(ds::GFX_TAB + n * 8) as usize,
        size: mem.u32(ds::GFX_TAB + n * 8 + 4) as usize,
    }
}

pub fn thing_name(mem: &Memory, t: usize) -> Vec<u8> {
    mem.cstr(ds::THINGS + t * THING_SIZE + 0x31, 15)
}
