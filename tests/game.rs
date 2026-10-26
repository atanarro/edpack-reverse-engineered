//! End-to-end tests: the demo adventure is played to its ending, from the
//! original packed EDGAME.DAT, from its .ED sources compiled by our compiler,
//! and from an EDGAME.DAT built by our packer; the compiler output is compared
//! with what the original ED.EXE produced.
//!
//! The demo is not part of this repository: set EDPACK_DEMO_DAT to its
//! EDGAME.DAT and EDPACK_DEMO_SRC to the folder with its sources, otherwise
//! these tests are skipped.
//!
//! Other games, played to their endings when their EDGAME.DAT is given:
//! EDPACK_NAURU_DAT for "El Dios de Nauru" (registered EDPACK v2.03),
//! EDPACK_DADTON_DAT for "Dadton Aguinández" (RUNDAT v1.02), and
//! EDPACK_DEMO103_DAT for the demo as rebuilt for the EDPACK v2.02 shareware
//! kit (PCManía 67, RUNDAT v1.03), with EDPACK_DEMO103_SRC for its sources.

use std::collections::BTreeMap;
use std::path::PathBuf;

use edpack::compiler::codegen::compile_script;
use edpack::compiler::parser::{Item, parse_ed};
use edpack::compiler::{compile_project, pack_game};
use edpack::dat::GameArchive;
use edpack::engine::Options;
use edpack::game::Game;
use edpack::harness::Harness;
use edpack::level::{LevelFormat, load_level, text_string};
use edpack::memory::{Memory, ds};

fn env_path(name: &str) -> PathBuf {
    std::env::var_os(name).map(PathBuf::from).unwrap_or_default()
}
fn dat_path() -> PathBuf {
    env_path("EDPACK_DEMO_DAT")
}
fn src_path() -> PathBuf {
    env_path("EDPACK_DEMO_SRC")
}
fn have_demo() -> bool {
    let ok = dat_path().is_file() && src_path().is_dir();
    if !ok {
        eprintln!("skipped: set EDPACK_DEMO_DAT and EDPACK_DEMO_SRC to run the demo tests");
    }
    ok
}
fn walkthrough() -> String {
    std::fs::read_to_string(PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tests/walkthrough/peter.txt")).unwrap()
}

fn play(game: Game) -> f64 {
    let mut h = Harness::new(game).unwrap();
    h.run(&walkthrough()).unwrap_or_else(|e| panic!("{e}"));
    assert!(h.exited());
    h.now
}

fn sources() -> BTreeMap<String, Vec<u8>> {
    std::fs::read_dir(src_path())
        .unwrap()
        .flatten()
        .map(|e| (e.file_name().to_string_lossy().to_uppercase(), std::fs::read(e.path()).unwrap()))
        .collect()
}

fn compiled() -> BTreeMap<String, Vec<u8>> {
    let all = sources();
    let src: BTreeMap<_, _> =
        all.iter().filter(|(n, _)| n.ends_with(".ED")).map(|(n, b)| (n.clone(), b.clone())).collect();
    compile_project(&src, &|n: &str| all.get(&n.to_uppercase()).cloned()).unwrap()
}

#[test]
fn demo_dat_and_compiled_sources_play_identically() {
    if !have_demo() {
        return;
    }
    let t_dat = play(Game::open(&dat_path()).unwrap());
    let t_src = play(Game::open(&src_path()).unwrap());
    assert_eq!(t_dat, t_src, "both versions should reach the ending at the same moment");
}

#[test]
fn packed_sources_play_to_the_end() {
    if !have_demo() {
        return;
    }
    let mut files = sources();
    files.extend(compiled());
    let fnt = files["LETRAS1.FNT"].clone();
    files.insert("NIVEL.FNT".into(), fnt);
    let dat = pack_game(&files, 6).unwrap();
    play(Game::Packed(GameArchive::new(dat)));
}

#[test]
fn save_and_load_restore_the_game_state() {
    if !have_demo() {
        return;
    }
    let mut h = Harness::new(Game::open(&dat_path()).unwrap()).unwrap();
    h.run("wait 9000; do 2 186 95; expect inv 1; save 0 celda; do 3 225 115").unwrap();
    assert_eq!(h.var(1003), 1);
    h.run("load 0; expect inv 0; expect inv 1").unwrap();
    assert_eq!(h.var(1003), 0);
}

#[test]
fn compiled_scripts_match_ed_exe_word_for_word() {
    if !have_demo() {
        return;
    }
    let arc = GameArchive::new(std::fs::read(dat_path()).unwrap());
    let mut checked = 0;
    for l in 0..=5 {
        let src = std::fs::read(src_path().join(format!("ROOM{l}.ED"))).unwrap();
        let items = parse_ed(&src, "room").unwrap();
        let mut mem = Memory::new();
        let code = load_level(&mut mem, &arc.bytes, arc.level_offset(l), true, LevelFormat::Shareware).unwrap().code;
        for it in &items {
            let Item::Block { kind, n, body, .. } = it else { continue };
            let base = match kind.as_str() {
                "ACTION" => mem.i16(ds::ACTION_OFFS + *n as usize * 2),
                "ANIMATION" => mem.i16(ds::ANIMS + *n as usize * 10 + 2),
                _ => continue,
            } as usize;
            let words = compile_script(body, "room").unwrap();
            let orig: Vec<i32> = code[base..base + words.len()].iter().map(|&w| w as i32).collect();
            assert_eq!(orig, words, "level {l} {kind} {n}");
            checked += 1;
        }
    }
    assert_eq!(checked, 54);
}

#[test]
fn compiled_tables_and_global_files_match_ed_exe() {
    if !have_demo() {
        return;
    }
    let arc = GameArchive::new(std::fs::read(dat_path()).unwrap());
    let out = compiled();
    let used = |m: &Memory, base: usize, size: usize, n: usize, flag: usize| -> Vec<Vec<u8>> {
        (0..n).filter(|&i| m.i16(base + i * size + flag) != 0).map(|i| m.slice(base + i * size, size)).collect()
    };
    for l in 0..=5usize {
        let (mut a, mut b) = (Memory::new(), Memory::new());
        load_level(&mut a, &out[&format!("NIVEL.{l:03}")], 0, false, LevelFormat::Shareware).unwrap();
        load_level(&mut b, &arc.bytes, arc.level_offset(l), true, LevelFormat::Shareware).unwrap();
        for (what, base, size, n, flag) in [
            ("LIMIT", ds::LIMIT, 14, 40, 0),
            ("LIMITMOV", ds::LIMITMOV, 22, 320, 0),
            ("DOOR", ds::DOORS, 10, 20, 0),
            ("ACINVTH", ds::ACINVTH, 8, 30, 0),
            ("COORANIM", ds::COORANIM, 10, 100, 8),
            ("SCALEMOV", ds::SCALEMOV, 18, 20, 0),
            ("SHADOWMOV", ds::SHADOWMOV, 12, 20, 0),
        ] {
            assert_eq!(used(&a, base, size, n, flag), used(&b, base, size, n, flag), "level {l} {what}");
        }
        for i in 0..250 {
            assert_eq!(text_string(&a, i), text_string(&b, i), "level {l} TEXT {i}");
        }
    }
    for (name, idx) in [("NIVEL.MOV", 8), ("NIVEL.MOU", 9), ("NIVEL.IAC", 10), ("NIVEL.TIT", 11)] {
        let mine = &out[name];
        let o = arc.entry(idx);
        assert_eq!(&mine[..], &arc.bytes[o..o + mine.len()], "{name}");
    }
}

#[test]
fn save_and_load_through_the_space_menu() {
    if !have_demo() {
        return;
    }
    let mut h = Harness::new(Game::open(&dat_path()).unwrap()).unwrap();
    // take the tray, SPACE -> GUARDAR -> slot 1, type a name, Enter
    h.run(
        "wait 9000; do 2 186 95; key 57; wait 500; click 237 67; wait 300; click 100 30; type Celda; enter; wait 500",
    )
    .unwrap();
    // overturn the bed, then SPACE -> CARGAR -> slot 1
    h.run("do 3 225 115; expect var 1003 1; key 57; wait 400; click 237 52; wait 300; click 100 30; wait 1500")
        .unwrap();
    assert_eq!(h.var(1003), 0, "the loaded game is from before the bed was moved");
    assert_eq!(h.u8(0xfd2c), 2, "the game's verb (Coger) is restored, not the menu cursor");
    h.run("expect inv 0; expect inv 1").unwrap();
}

#[test]
fn nauru_plays_to_the_end() {
    let dat = env_path("EDPACK_NAURU_DAT");
    if !dat.is_file() {
        eprintln!("skipped: set EDPACK_NAURU_DAT to El Dios de Nauru's EDGAME.DAT");
        return;
    }
    let game = Game::open(&dat).unwrap();
    assert_eq!(game.level_format(), LevelFormat::Registered);
    let script =
        std::fs::read_to_string(PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tests/walkthrough/nauru.txt")).unwrap();
    let mut h = Harness::new(game).unwrap();
    h.run(&script).unwrap_or_else(|e| panic!("{e}"));
    assert!(h.exited(), "the game quits from the menu shown after the ending");
    assert_eq!(h.var(1060), 1, "the true god was chosen");
}

#[test]
fn dadton_plays_to_the_end() {
    let dat = env_path("EDPACK_DADTON_DAT");
    if !dat.is_file() {
        eprintln!("skipped: set EDPACK_DADTON_DAT to Dadton Aguinández's EDGAME.DAT");
        return;
    }
    let game = Game::open(&dat).unwrap();
    assert_eq!(game.level_format(), LevelFormat::V102);
    let script =
        std::fs::read_to_string(PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tests/walkthrough/dadton.txt"))
            .unwrap();
    let mut h = Harness::new(game).unwrap();
    h.run(&script).unwrap_or_else(|e| panic!("{e}"));
    assert!(h.exited(), "the game ends with EXIT after the credits");
    assert_eq!(h.var(1017), 2, "two pirates joined the crew");
}

fn demo103() -> Option<(PathBuf, PathBuf)> {
    let (dat, src) = (env_path("EDPACK_DEMO103_DAT"), env_path("EDPACK_DEMO103_SRC"));
    if !dat.is_file() {
        eprintln!("skipped: set EDPACK_DEMO103_DAT to the v2.02 shareware demo's EDGAME.DAT (PCManía 67)");
        return None;
    }
    Some((dat, src))
}

#[test]
fn demo103_plays_to_the_end() {
    let Some((dat, _)) = demo103() else { return };
    let game = Game::open(&dat).unwrap();
    assert_eq!(game.level_format(), LevelFormat::SharewareV103);
    // the same solution as the v2.0 demo: the rebuilt game only adds an
    // introduction (skippable with ASLT) and four more cursors
    play(game);
}

#[test]
fn demo103_scripts_match_ed_exe() {
    let Some((dat, src)) = demo103() else { return };
    if !src.is_dir() {
        eprintln!("skipped: set EDPACK_DEMO103_SRC to the v2.02 demo's FUENTES folder");
        return;
    }
    let arc = GameArchive::new(std::fs::read(&dat).unwrap());
    let mut checked = 0;
    for l in 0..=5 {
        let items = parse_ed(&std::fs::read(src.join(format!("ROOM{l}.ED"))).unwrap(), "room").unwrap();
        let mut mem = Memory::new();
        let code =
            load_level(&mut mem, &arc.bytes, arc.level_offset(l), true, LevelFormat::SharewareV103).unwrap().code;
        for it in &items {
            let Item::Block { kind, n, body, .. } = it else { continue };
            let base = match kind.as_str() {
                "ACTION" => mem.i16(ds::ACTION_OFFS + *n as usize * 2),
                "ANIMATION" => mem.i16(ds::ANIMS + *n as usize * 10 + 2),
                _ => continue,
            } as usize;
            let words = compile_script(body, "room").unwrap();
            let orig: Vec<i32> = code[base..base + words.len()].iter().map(|&w| w as i32).collect();
            assert_eq!(orig, words, "level {l} {kind} {n}");
            checked += 1;
        }
    }
    // the v2.0 demo's 54 scripts plus the new ACTION 9 of level 5 (with ASLT)
    assert_eq!(checked, 55);
}

/// The shareware runtime's start-up notice (RUNDAT 0xbb0e): white text on
/// black for 6 s, cut short by a key but not by a mouse click.
#[test]
fn shareware_notice_before_level_0() {
    if !have_demo() {
        return;
    }
    let notice = |h: &Harness| {
        let f = h.last.as_ref().unwrap();
        let white = f.screen[60 * 320..120 * 320].iter().filter(|&&c| c == 15).count();
        white > 500 && f.palette[45..48] == [255, 255, 255]
    };
    let mut h = Harness::with_options(Game::open(&dat_path()).unwrap(), Options::default()).unwrap();
    h.run("wait 1000; click 160 100; wait 4500").unwrap();
    assert!(notice(&h), "the notice stays up for 6 s, clicks don't dismiss it");
    h.run("wait 1000").unwrap();
    assert!(!notice(&h), "the game starts after 6 s");

    let mut h = Harness::with_options(Game::open(&dat_path()).unwrap(), Options::default()).unwrap();
    h.run("wait 500").unwrap();
    assert!(notice(&h));
    h.run("key 57; wait 300").unwrap();
    assert!(!notice(&h), "a key dismisses the notice");

    // the harness default (used by the walkthroughs) starts at level 0
    let mut h = Harness::new(Game::open(&dat_path()).unwrap()).unwrap();
    h.run("wait 500").unwrap();
    assert!(!notice(&h));
}
