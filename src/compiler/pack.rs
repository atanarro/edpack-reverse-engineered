//! A modern ED2DAT.EXE: packs compiled NIVEL.* files and their resources into
//! a single EDGAME.DAT in the layout RUNDAT.EXE expects:
//! `u32 table[12]` (levels 0..5, NIVEL.INV, .FNT, .MOV, .MOU, .IAC, .TIT);
//! each level = loose level file + FILE/gfx offset tables (u32 offset, u32
//! size); resources stored once, every byte +5.

use std::collections::{BTreeMap, HashMap};

use crate::Error;
use crate::dat::Special;
use crate::level::{file_entry, gfx_entry, load_level};
use crate::memory::Memory;

pub fn pack_game(files: &BTreeMap<String, Vec<u8>>, levels: usize) -> Result<Vec<u8>, Error> {
    let get = |n: &str| files.get(&n.to_uppercase());
    let n_entries = levels + Special::ALL.len();
    let mut out = vec![0u8; n_entries * 4];
    let mut table = vec![0u32; n_entries];
    let mut stored: HashMap<String, (usize, usize)> = HashMap::new();

    #[allow(clippy::needless_range_loop)] // n is the level number, not just an index
    for n in 0..levels {
        let lname = format!("NIVEL.{n:03}");
        let Some(file) = get(&lname) else { continue };
        let mut mem = Memory::new();
        let end = load_level(&mut mem, file, 0, false)?.end;
        table[n] = out.len() as u32;
        out.extend(&file[..end]); // the offset tables must follow the last block
        let tables_at = out.len();
        out.extend([0u8; 0xa0 + 0x20]);
        let mut want = Vec::new();
        for i in 0..20 {
            let f = file_entry(&mem, i);
            if !f.name.is_empty() {
                want.push((i * 8, f.name));
            }
        }
        for i in 0..4 {
            let g = gfx_entry(&mem, i);
            if !g.name.is_empty() {
                want.push((0xa0 + i * 8, g.name));
            }
        }
        for (at, name) in want {
            let key = String::from_utf8_lossy(&name).to_uppercase();
            if !stored.contains_key(&key) {
                let b = get(&key).ok_or_else(|| Error::new(format!("{lname}: no encuentro el archivo '{key}'")))?;
                stored.insert(key.clone(), (out.len(), b.len()));
                out.extend(b.iter().map(|x| x.wrapping_add(5)));
            }
            let (off, size) = stored[&key];
            out[tables_at + at..tables_at + at + 4].copy_from_slice(&(off as u32).to_le_bytes());
            out[tables_at + at + 4..tables_at + at + 8].copy_from_slice(&(size as u32).to_le_bytes());
        }
    }
    for (k, s) in Special::ALL.iter().enumerate() {
        let b = get(s.file_name()).ok_or_else(|| Error::new(format!("Falta {}", s.file_name())))?;
        table[levels + k] = out.len() as u32;
        out.extend(b);
    }
    for (i, o) in table.iter().enumerate() {
        out[i * 4..i * 4 + 4].copy_from_slice(&o.to_le_bytes());
    }
    Ok(out)
}
