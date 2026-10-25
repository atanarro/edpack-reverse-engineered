//! Global compiled files shared by all levels: NIVEL.INV, .MOU, .IAC, .TIT.

use crate::Error;
use crate::dat::{ByteReader, file_type};

pub const INV_RECORD: usize = 0x4aa;

fn open<'a>(src: Option<(&'a [u8], usize)>, kind: u8, name: &str) -> Result<ByteReader<'a>, Error> {
    let (bytes, off) = src.ok_or_else(|| Error::new(format!("No encuentro el fichero '{name}'.")))?;
    let mut r = ByteReader::new(bytes, off);
    if r.header()?.kind != kind {
        return Err(Error::new(format!("Archivo '{name}' no es del tipo correcto.")));
    }
    Ok(r)
}

/// NIVEL.INV: u16 count, palette[768], count x 0x4aa-byte records
/// (text +0x1E, name +0x20, u16 w, u16 h, pixels).
pub struct InventoryFile {
    pub count: usize,
    pub palette: Vec<u8>,
    data: Vec<u8>,
}

impl InventoryFile {
    pub fn load(src: Option<(&[u8], usize)>) -> Result<Self, Error> {
        let mut r = open(src, file_type::INV, "NIVEL.INV")?;
        let count = r.u16()? as usize;
        let palette = r.raw(768)?.to_vec();
        let data = r.bytes[r.pos..].to_vec();
        Ok(InventoryFile { count, palette, data })
    }

    /// Decoded record of object n, as ADDINV reads it.
    pub fn record(&self, n: usize) -> Vec<u8> {
        // QUIRK: the original computes n*0x4aa in 16 bits.
        let at = (n * INV_RECORD) & 0xffff;
        let mut rec = vec![0u8; INV_RECORD];
        for (i, b) in rec.iter_mut().enumerate() {
            *b = self.data.get(at + i).copied().unwrap_or(0);
        }
        for b in rec.iter_mut().take(250) {
            if *b == 0 {
                break;
            }
            *b = b.wrapping_sub(0x1e);
        }
        for b in rec[0xfa..0xfa + 30].iter_mut() {
            if *b == 0 {
                break;
            }
            *b = b.wrapping_sub(0x20);
        }
        rec
    }
}

/// NIVEL.MOU: u16 count, records of 6 rows x {srcX,srcY,w,h,hotX,hotY}.
pub struct MouseFile {
    pub count: usize,
    data: Vec<u8>,
}

impl MouseFile {
    pub fn load(src: Option<(&[u8], usize)>) -> Result<Self, Error> {
        let mut r = open(src, file_type::MOU, "NIVEL.MOU")?;
        let count = r.u16()? as usize;
        Ok(MouseFile { count, data: r.bytes[r.pos..].to_vec() })
    }
    /// 72 bytes read at the (buggy) 60-byte stride used by NEWMOUSE.
    pub fn record(&self, n: usize) -> Vec<u8> {
        (0..0x48).map(|i| self.data.get(n * 0x3c + i).copied().unwrap_or(0)).collect()
    }
}

/// NIVEL.IAC: 50 x {used, obj1, obj2, new, mode}.
pub fn load_iac(src: Option<(&[u8], usize)>) -> Result<Vec<u8>, Error> {
    let mut r = open(src, file_type::IAC, "NIVEL.IAC")?;
    Ok(r.raw(0x1f4)?.to_vec())
}

/// NIVEL.TIT: char[50] stored as `c - 0xA5`, then a u16 check value.
pub fn load_title(src: Option<(&[u8], usize)>) -> Result<Vec<u8>, Error> {
    let mut r = open(src, file_type::TIT, "NIVEL.TIT")?;
    let raw = r.raw(0x32)?;
    Ok(raw
        .iter()
        .map(|b| b.wrapping_add(0xa5))
        .zip(raw)
        .take_while(|&(c, &b)| b != 0 && c != 0)
        .map(|(c, _)| c)
        .collect())
}
