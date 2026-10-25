//! Developer tools for EDPACK games.
//!
//! ```text
//! edtool run <EDGAME.DAT|dir> <outdir> <script|script file>   headless play + PNG screenshots
//! edtool disasm <EDGAME.DAT|dir> <level>                      decompile a level's scripts
//! edtool dump <EDGAME.DAT|dir>                                summary of every level
//! edtool cmf2wav <in.cmf> <out.wav> [seconds]                 render CMF music
//! ```

use std::path::Path;
use std::process::exit;

use edpack::audio::cmf::CmfPlayer;
use edpack::audio::opl2::OPL_RATE;
use edpack::game::Game;
use edpack::harness::Harness;
use edpack::level::{file_entry, gfx_entry, load_level, text_string, thing_name};
use edpack::memory::{Memory, ds};
use edpack::png_out::save_png;
use edpack::vm::disassemble;
use edpack::{Error, cp850_to_string};

fn usage() -> ! {
    eprintln!(
        "uso: edtool run <juego> <dir_salida> <script>\n     edtool disasm <juego> <nivel>\n     edtool dump <juego>\n     edtool cmf2wav <in.cmf> <out.wav> [segundos]"
    );
    exit(2)
}

fn load(mem: &mut Memory, game: &Game, n: usize) -> Result<Option<Vec<i16>>, Error> {
    let Some((b, off)) = game.level(n) else { return Ok(None) };
    Ok(Some(load_level(mem, b, off, game.is_packed(), game.level_format())?.code))
}

fn main() {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let result = match args.first().map(String::as_str) {
        Some("run") if args.len() >= 4 => run(&args[1], &args[2], &args[3]),
        Some("disasm") if args.len() >= 3 => disasm(&args[1], args[2].parse().unwrap_or(0)),
        Some("dump") if args.len() >= 2 => dump(&args[1]),
        Some("cmf2wav") if args.len() >= 3 => {
            cmf2wav(&args[1], &args[2], args.get(3).and_then(|s| s.parse().ok()).unwrap_or(20.0))
        }
        _ => usage(),
    };
    if let Err(e) = result {
        eprintln!("{e}");
        exit(1);
    }
}

fn run(game: &str, out: &str, script: &str) -> Result<(), Error> {
    let script = if Path::new(script).is_file() {
        std::fs::read_to_string(script).map_err(|e| Error::new(e.to_string()))?
    } else {
        script.to_string()
    };
    std::fs::create_dir_all(out).map_err(|e| Error::new(e.to_string()))?;
    if let Some(src) = edpack::v1::data::Source::open(Path::new(game)) {
        let opts = edpack::v1::Options::from_setup_cfg(Path::new(game)).unwrap_or_default();
        return run_v1(src, opts, out, &script);
    }
    let mut h = Harness::new(Game::open(Path::new(game))?)?;
    let dir = out.to_string();
    h.on_shot = Some(Box::new(move |name, f| {
        let path = Path::new(&dir).join(format!("{name}.png"));
        if let Err(e) = save_png(&path, 320, 200, &f.screen, &f.palette, 2) {
            eprintln!("{}: {e}", path.display());
        }
    }));
    h.log = Box::new(|s| println!("{s}"));
    h.run(&script)?;
    if h.exited() {
        println!("el juego terminó en t={}", h.now)
    } else {
        println!("{}", h.state())
    }
    Ok(())
}

fn run_v1(src: edpack::v1::data::Source, opts: edpack::v1::Options, out: &str, script: &str) -> Result<(), Error> {
    let mut h = edpack::v1::harness::Harness::with_options(src, opts)?;
    let dir = out.to_string();
    h.on_shot = Some(Box::new(move |name, f| {
        let path = Path::new(&dir).join(format!("{name}.png"));
        if let Err(e) = save_png(&path, 320, 200, &f.screen, &f.palette, 2) {
            eprintln!("{}: {e}", path.display());
        }
    }));
    h.log = Box::new(|s| println!("{s}"));
    h.run(script)?;
    if h.exited() {
        println!("el juego terminó en t={}", h.now)
    } else {
        println!("{}", h.state())
    }
    Ok(())
}

fn disasm(game: &str, n: usize) -> Result<(), Error> {
    let game = Game::open(Path::new(game))?;
    let mut mem = Memory::new();
    let code = load(&mut mem, &game, n)?.ok_or_else(|| Error::new(format!("no existe el nivel {n}")))?;
    for i in 0..50 {
        let off = mem.i16(ds::ACTION_OFFS + i * 2);
        if off == 0 && i > 0 {
            continue;
        }
        println!("ACTION {i} @{off}");
        for l in disassemble(&code, off as usize, 256) {
            println!("  {l}");
        }
    }
    for i in 0..50 {
        let a = ds::ANIMS + i * 10;
        let base = mem.i16(a + 2);
        if base == 0 && mem.i16(a) == 0 {
            continue;
        }
        println!("ANIMATION {i} active={} @{base}", mem.i16(a));
        for l in disassemble(&code, base as usize, 256) {
            println!("  {l}");
        }
    }
    Ok(())
}

fn dump(game: &str) -> Result<(), Error> {
    let game = Game::open(Path::new(game))?;
    for n in 0..1000 {
        let mut mem = Memory::new();
        if load(&mut mem, &game, n)?.is_none() {
            continue;
        }
        println!("== nivel {n}");
        let gfx: Vec<String> = (0..4)
            .map(|i| gfx_entry(&mem, i))
            .filter(|g| !g.name.is_empty())
            .map(|g| cp850_to_string(&g.name))
            .collect();
        println!("  gráficos: {}", gfx.join(" "));
        let files: Vec<String> = (0..20)
            .map(|i| file_entry(&mem, i))
            .filter(|f| !f.name.is_empty())
            .map(|f| cp850_to_string(&f.name))
            .collect();
        println!("  archivos: {}", files.join(" "));
        let things: Vec<String> = (0..30)
            .filter(|&t| mem.u8(ds::THINGS + t * 92 + 8) != 0)
            .map(|t| format!("{t}:{}", cp850_to_string(&thing_name(&mem, t))))
            .collect();
        println!("  objetos: {}", things.join(", "));
        let texts = (0..250).filter(|&i| text_string(&mem, i).is_some()).count();
        println!("  textos: {texts}");
    }
    Ok(())
}

fn cmf2wav(input: &str, output: &str, secs: f64) -> Result<(), Error> {
    let bytes = std::fs::read(input).map_err(|e| Error::new(format!("{input}: {e}")))?;
    let mut p = CmfPlayer::new();
    p.load(&bytes)?;
    let n = (OPL_RATE * secs) as usize;
    let mut buf = vec![0f32; n];
    for chunk in buf.chunks_mut(4096) {
        p.render(chunk);
    }
    let mut wav = Vec::with_capacity(44 + n * 2);
    let rate = OPL_RATE as u32;
    wav.extend(b"RIFF");
    wav.extend((36 + n as u32 * 2).to_le_bytes());
    wav.extend(b"WAVEfmt ");
    wav.extend(16u32.to_le_bytes());
    wav.extend(1u16.to_le_bytes());
    wav.extend(1u16.to_le_bytes());
    wav.extend(rate.to_le_bytes());
    wav.extend((rate * 2).to_le_bytes());
    wav.extend(2u16.to_le_bytes());
    wav.extend(16u16.to_le_bytes());
    wav.extend(b"data");
    wav.extend((n as u32 * 2).to_le_bytes());
    let mut peak = 0f32;
    for s in &buf {
        peak = peak.max(s.abs());
        wav.extend(((s.clamp(-1.0, 1.0) * 32767.0) as i16).to_le_bytes());
    }
    std::fs::write(output, wav).map_err(|e| Error::new(e.to_string()))?;
    println!("{output}: {secs} s, pico {peak:.3}");
    Ok(())
}
