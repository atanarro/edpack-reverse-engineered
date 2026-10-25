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

pub struct LoadedLevel {
    /// 5000 words of bytecode.
    pub code: Vec<i16>,
    /// Number of bytes consumed from the start of the file.
    pub end: usize,
}

/// Load the level at `offset` of `bytes` into memory.  `packed` is false for
/// loose NIVEL.nnn files (RUNFILES.EXE), which lack the trailing resource
/// offset tables.  Strings are decrypted in place.
pub fn load_level(mem: &mut Memory, bytes: &[u8], offset: usize, packed: bool) -> Result<LoadedLevel, Error> {
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
    for b in &LEVEL_BLOCKS {
        match *b {
            Block::Skip => {
                r.byte()?;
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
    r.byte()?; // the skip after SELECTEXT
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
    Ok(LoadedLevel { code, end: r.pos })
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
