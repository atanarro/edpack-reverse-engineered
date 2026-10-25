//! Readers for the packed game archive (EDGAME.DAT, produced by ED2DAT.EXE).
//!
//! Layout (reverse-engineered from RUNDAT.EXE):
//! * `u32 table[]`, entry i at offset 4*i. Shareware layout: 0..5 = levels 0..5,
//!   6 = NIVEL.INV, 7 = NIVEL.FNT (raw font), 8 = NIVEL.MOV, 9 = NIVEL.MOU,
//!   10 = NIVEL.IAC, 11 = NIVEL.TIT.
//! * Compiled files start with a 30 byte header: `02 00 00 <type>` + 26 bytes.
//! * Resource files (PCX, CMF, VOC, FLI) referenced by levels are stored
//!   verbatim with every byte +5 and located by absolute (offset, size)
//!   tables inside each level.

use crate::Error;

pub const HEADER_SIZE: usize = 30;

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Special {
    Inv,
    Fnt,
    Mov,
    Mou,
    Iac,
    Tit,
}

impl Special {
    pub const ALL: [Special; 6] = [Special::Inv, Special::Fnt, Special::Mov, Special::Mou, Special::Iac, Special::Tit];
    pub fn index(self) -> usize {
        match self {
            Special::Inv => 6,
            Special::Fnt => 7,
            Special::Mov => 8,
            Special::Mou => 9,
            Special::Iac => 10,
            Special::Tit => 11,
        }
    }
    pub fn file_name(self) -> &'static str {
        match self {
            Special::Inv => "NIVEL.INV",
            Special::Fnt => "NIVEL.FNT",
            Special::Mov => "NIVEL.MOV",
            Special::Mou => "NIVEL.MOU",
            Special::Iac => "NIVEL.IAC",
            Special::Tit => "NIVEL.TIT",
        }
    }
}

pub mod file_type {
    pub const LEVEL: u8 = 0;
    pub const INV: u8 = 1;
    pub const MOV: u8 = 2;
    pub const MOU: u8 = 3;
    pub const IAC: u8 = 4;
    pub const TIT: u8 = 5;
}

fn le32(b: &[u8], o: usize) -> u32 {
    if o + 4 > b.len() {
        return 0;
    }
    u32::from_le_bytes([b[o], b[o + 1], b[o + 2], b[o + 3]])
}

pub struct GameArchive {
    pub bytes: Vec<u8>,
    entries: Vec<u32>,
}

impl GameArchive {
    pub fn new(bytes: Vec<u8>) -> Self {
        let first = le32(&bytes, 0);
        let count = ((first >> 2) as usize).min(1000).min(bytes.len() / 4);
        let entries = (0..count).map(|i| le32(&bytes, i * 4)).collect();
        GameArchive { bytes, entries }
    }

    pub fn is_archive(bytes: &[u8]) -> bool {
        if bytes.len() < 48 {
            return false;
        }
        let first = le32(bytes, 0) as usize;
        first >= 48 && first.is_multiple_of(4) && first < bytes.len() && bytes[first] == 2
    }

    /// Offset of table entry i, or 0 if absent.
    pub fn entry(&self, i: usize) -> usize {
        self.entries.get(i).copied().unwrap_or(0) as usize
    }

    pub fn level_offset(&self, n: usize) -> usize {
        self.entry(n)
    }

    /// Raw bytes of a special entry that is not a compiled file (the font).
    pub fn special_raw(&self, s: Special) -> Option<&[u8]> {
        let start = self.entry(s.index());
        if start == 0 {
            return None;
        }
        let end = self.entries.iter().map(|&e| e as usize).filter(|&e| e > start).min().unwrap_or(self.bytes.len());
        Some(&self.bytes[start..end.min(self.bytes.len())])
    }

    /// A resource embedded in the archive, de-obfuscated (-5 per byte).
    pub fn resource(&self, offset: usize, size: usize) -> Vec<u8> {
        let end = (offset + size).min(self.bytes.len());
        self.bytes[offset.min(end)..end].iter().map(|b| b.wrapping_sub(5)).collect()
    }
}

/// Sequential reader for compiled level/data files.
pub struct ByteReader<'a> {
    pub bytes: &'a [u8],
    pub pos: usize,
}

pub struct Header {
    pub major: u8,
    pub minor: u8,
    pub kind: u8,
}

impl<'a> ByteReader<'a> {
    pub fn new(bytes: &'a [u8], pos: usize) -> Self {
        ByteReader { bytes, pos }
    }
    pub fn byte(&mut self) -> Result<u8, Error> {
        let b = *self.bytes.get(self.pos).ok_or_else(|| Error::new("Fin de datos inesperado"))?;
        self.pos += 1;
        Ok(b)
    }
    pub fn raw(&mut self, n: usize) -> Result<&'a [u8], Error> {
        if self.pos + n > self.bytes.len() {
            return Err(Error::new("Fin de datos inesperado"));
        }
        let v = &self.bytes[self.pos..self.pos + n];
        self.pos += n;
        Ok(v)
    }
    pub fn u16(&mut self) -> Result<u16, Error> {
        let r = self.raw(2)?;
        Ok(u16::from_le_bytes([r[0], r[1]]))
    }
    pub fn header(&mut self) -> Result<Header, Error> {
        let h = self.raw(HEADER_SIZE)?;
        Ok(Header { major: h[0], minor: h[1], kind: h[3] })
    }
    /// RLE block exactly as the original reader (0x1709): a byte > 0xC0
    /// starts a run of (b & 0x3f) copies of the next byte; any other byte is
    /// literal.  A run crossing the end of the block is truncated.
    pub fn rle(&mut self, size: usize) -> Result<Vec<u8>, Error> {
        let mut out = Vec::with_capacity(size);
        while out.len() < size {
            let c = self.byte()?;
            if c > 0xc0 {
                let k = (c & 0x3f) as usize;
                let v = self.byte()?;
                for _ in 0..k {
                    if out.len() < size {
                        out.push(v);
                    }
                }
            } else {
                out.push(c);
            }
        }
        Ok(out)
    }
}
