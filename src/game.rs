//! Game sources: a packed EDGAME.DAT (RUNDAT mode), or loose compiled NIVEL.*
//! files plus their resources (RUNFILES mode), or .ED sources compiled here.

use std::collections::BTreeMap;
use std::path::Path;

use crate::Error;
use crate::compiler::compile_project;
use crate::dat::{GameArchive, Special};
use crate::level::FileEntry;

pub enum Game {
    Packed(GameArchive),
    /// upper-case file name -> contents
    Loose(BTreeMap<String, Vec<u8>>),
}

impl Game {
    pub fn is_packed(&self) -> bool {
        matches!(self, Game::Packed(_))
    }

    fn get(&self, name: &[u8]) -> Option<&Vec<u8>> {
        match self {
            Game::Loose(f) => f.get(&String::from_utf8_lossy(name).to_uppercase()),
            Game::Packed(_) => None,
        }
    }

    /// Compiled level n: (file bytes, offset of its header).
    pub fn level(&self, n: usize) -> Option<(&[u8], usize)> {
        match self {
            Game::Packed(a) => match a.level_offset(n) {
                0 => None,
                off => Some((&a.bytes[..], off)),
            },
            Game::Loose(_) => self.get(format!("NIVEL.{n:03}").as_bytes()).map(|b| (&b[..], 0)),
        }
    }

    /// A global compiled file (the font is returned raw, offset 0).
    pub fn special(&self, s: Special) -> Option<(&[u8], usize)> {
        match self {
            Game::Packed(a) if s == Special::Fnt => a.special_raw(s).map(|b| (b, 0)),
            Game::Packed(a) => match a.entry(s.index()) {
                0 => None,
                off => Some((&a.bytes[..], off)),
            },
            Game::Loose(_) => self.get(s.file_name().as_bytes()).map(|b| (&b[..], 0)),
        }
    }

    /// A resource file referenced by the current level.
    pub fn resource(&self, e: &FileEntry) -> Option<Vec<u8>> {
        match self {
            Game::Packed(a) => (e.size > 0).then(|| a.resource(e.offset, e.size)),
            Game::Loose(_) => (!e.name.is_empty()).then(|| self.get(&e.name).cloned()).flatten(),
        }
    }

    /// A LOADDATA file: (bytes, offset).
    pub fn data_file(&self, e: &FileEntry) -> Option<(Vec<u8>, usize)> {
        match self {
            Game::Packed(a) => (e.size > 0).then(|| (a.bytes.clone(), e.offset)),
            Game::Loose(_) => self.get(&e.name).map(|b| (b.clone(), 0)),
        }
    }

    /// Pick the right kind of game from a set of files (upper-case name ->
    /// bytes): EDGAME.DAT, compiled NIVEL.* files, or .ED sources.
    pub fn from_files(mut files: BTreeMap<String, Vec<u8>>) -> Result<Game, Error> {
        if let Some(dat) = files.remove("EDGAME.DAT") {
            return Ok(Game::Packed(GameArchive::new(dat)));
        }
        if let Some(k) = files.iter().find(|(_, b)| GameArchive::is_archive(b)).map(|(k, _)| k.clone()) {
            return Ok(Game::Packed(GameArchive::new(files.remove(&k).unwrap())));
        }
        let sources: BTreeMap<String, Vec<u8>> =
            files.iter().filter(|(n, _)| n.ends_with(".ED")).map(|(n, b)| (n.clone(), b.clone())).collect();
        if !sources.is_empty() {
            let compiled = compile_project(&sources, &|name: &str| files.get(&name.to_uppercase()).cloned())?;
            files.extend(compiled);
        }
        // The font must be copied as NIVEL.FNT; fall back to the first LETRAS*.FNT.
        if !files.contains_key("NIVEL.FNT")
            && let Some(f) = files.keys().find(|n| n.ends_with(".FNT")).cloned()
        {
            let b = files[&f].clone();
            files.insert("NIVEL.FNT".into(), b);
        }
        if files.contains_key("NIVEL.000") {
            return Ok(Game::Loose(files));
        }
        Err(Error::new("No encuentro EDGAME.DAT, archivos NIVEL.* ni fuentes .ED."))
    }

    /// Open EDGAME.DAT (or any archive file) or a directory with a game.
    pub fn open(path: &Path) -> Result<Game, Error> {
        let mut files = BTreeMap::new();
        let read = |p: &Path| std::fs::read(p).map_err(|e| Error::new(format!("{}: {e}", p.display())));
        if path.is_dir() {
            let dir = std::fs::read_dir(path).map_err(|e| Error::new(format!("{}: {e}", path.display())))?;
            for entry in dir.flatten() {
                if entry.path().is_file() {
                    files.insert(entry.file_name().to_string_lossy().to_uppercase(), read(&entry.path())?);
                }
            }
        } else {
            files.insert("EDGAME.DAT".into(), read(path)?);
        }
        Game::from_files(files)
    }
}
