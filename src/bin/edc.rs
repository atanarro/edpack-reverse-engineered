//! Modern replacement for ED.EXE + ED2DAT.EXE.
//!
//! ```text
//! edc <source dir> <output dir> [--pack] [--v202] [--chain ROOM4.ED,ROOM2.ED,...]
//! ```
//! Compiles every .ED file of the source directory into NIVEL.* files
//! (RUNFILES format); with `--pack` it also writes EDGAME.DAT (RUNDAT format)
//! with all the graphics, music, sound and animation files the levels use.
//! `--v202` writes ED.EXE v2.02's formats (for RUNDAT.EXE v1.03) instead of
//! ED.EXE v2.0's. `--chain` compiles those files in that order in one run, the
//! way ED.EXE leaves each level's data to the next (to rebuild a shipped
//! EDGAME.DAT byte for byte).

use std::collections::BTreeMap;
use std::path::Path;
use std::process::exit;

use edpack::Error;
use edpack::compiler::{CompileOptions, Target, compile_project_with, pack_game};

fn main() {
    let args: Vec<String> = std::env::args().skip(1).collect();
    if args.len() < 2 {
        eprintln!("uso: edc <directorio de fuentes> <directorio de salida> [--pack] [--v202] [--chain A.ED,B.ED,...]");
        exit(2);
    }
    let target = if args.iter().any(|a| a == "--v202") { Target::V202 } else { Target::V20 };
    let carry_over = match args.iter().position(|a| a == "--chain") {
        Some(i) => args.get(i + 1).map(|l| l.split(',').map(|f| f.to_uppercase()).collect()).unwrap_or_default(),
        None => Vec::new(),
    };
    let opts = CompileOptions { target, carry_over };
    if let Err(e) = run(Path::new(&args[0]), Path::new(&args[1]), args.iter().any(|a| a == "--pack"), &opts) {
        eprintln!("{e}");
        exit(1);
    }
}

fn run(src: &Path, out: &Path, pack: bool, opts: &CompileOptions) -> Result<(), Error> {
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
    let mut compiled = compile_project_with(&sources, &|n: &str| files.get(&n.to_uppercase()).cloned(), opts)?;
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
        let dat = pack_game(&files, 6, opts.target.level_format())?;
        std::fs::write(out.join("EDGAME.DAT"), &dat).map_err(io)?;
        println!("EDGAME.DAT     {} bytes", dat.len());
    }
    Ok(())
}
