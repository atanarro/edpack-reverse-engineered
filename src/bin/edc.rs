//! Modern replacement for ED.EXE + ED2DAT.EXE.
//!
//! ```text
//! edc <source dir> <output dir> [--pack]
//! ```
//! Compiles every .ED file of the source directory into NIVEL.* files
//! (RUNFILES format); with `--pack` it also writes EDGAME.DAT (RUNDAT format)
//! with all the graphics, music, sound and animation files the levels use.

use std::collections::BTreeMap;
use std::path::Path;
use std::process::exit;

use edpack::Error;
use edpack::compiler::{compile_project, pack_game};

fn main() {
    let args: Vec<String> = std::env::args().skip(1).collect();
    if args.len() < 2 {
        eprintln!("uso: edc <directorio de fuentes> <directorio de salida> [--pack]");
        exit(2);
    }
    if let Err(e) = run(Path::new(&args[0]), Path::new(&args[1]), args.iter().any(|a| a == "--pack")) {
        eprintln!("{e}");
        exit(1);
    }
}

fn run(src: &Path, out: &Path, pack: bool) -> Result<(), Error> {
    let io = |e: std::io::Error| Error::new(e.to_string());
    let mut files = BTreeMap::new();
    for entry in std::fs::read_dir(src).map_err(io)?.flatten() {
        if entry.path().is_file() {
            files.insert(entry.file_name().to_string_lossy().to_uppercase(), std::fs::read(entry.path()).map_err(io)?);
        }
    }
    let sources: BTreeMap<String, Vec<u8>> =
        files.iter().filter(|(n, _)| n.ends_with(".ED")).map(|(n, b)| (n.clone(), b.clone())).collect();
    if sources.is_empty() {
        return Err(Error::new(format!("{}: no hay archivos .ED", src.display())));
    }
    let mut compiled = compile_project(&sources, &|n: &str| files.get(&n.to_uppercase()).cloned())?;
    if !compiled.contains_key("NIVEL.FNT")
        && !files.contains_key("NIVEL.FNT")
        && let Some(f) = files.keys().find(|n| n.ends_with(".FNT"))
    {
        println!("NIVEL.FNT <- {f}");
        compiled.insert("NIVEL.FNT".into(), files[f].clone());
    }
    std::fs::create_dir_all(out).map_err(io)?;
    for (name, bytes) in &compiled {
        std::fs::write(out.join(name), bytes).map_err(io)?;
        println!("{name:<14} {} bytes", bytes.len());
    }
    if pack {
        files.extend(compiled);
        let dat = pack_game(&files, 6)?;
        std::fs::write(out.join("EDGAME.DAT"), &dat).map_err(io)?;
        println!("EDGAME.DAT     {} bytes", dat.len());
    }
    Ok(())
}
