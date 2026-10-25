//! EDPACK v1 (RUNVGA.EXE) tests with "Las Aventuras de Manolo Kabezabolo" and the v1 demo.
//! The game is not part of this repository: set EDPACK_MANOLO to its folder
//! (the one with RESOURCE.DAT), otherwise these tests are skipped.

use std::path::PathBuf;

use edpack::memory::Memory;
use edpack::v1::data::{self, Source};
use edpack::v1::harness::Harness;

fn dir() -> Option<PathBuf> {
    std::env::var_os("EDPACK_MANOLO").map(PathBuf::from)
}

fn game() -> Option<Source> {
    let dir = dir()?;
    let src = Source::open(&dir);
    if src.is_none() {
        eprintln!("EDPACK_MANOLO does not point to a v1 game");
    }
    src
}

fn levels(src: &Source) -> Vec<usize> {
    (0..998).filter(|&n| src.entry(n).is_some()).collect()
}

#[test]
fn every_level_loads_with_valid_resources() {
    let Some(src) = game() else { return };
    let levels = levels(&src);
    assert_eq!(levels.len(), 25);
    for n in levels {
        let mut m = Memory::new();
        data::load_level(&mut m, &src, n).unwrap_or_else(|e| panic!("level {n}: {e}"));
        for i in 0..20 {
            let (name, off, size) = data::file_entry(&m, i);
            if size == 0 {
                continue;
            }
            let bytes = src.resource(&name, off, size).unwrap_or_else(|| panic!("level {n} file {i}"));
            let lname = String::from_utf8_lossy(&name).to_lowercase();
            if lname.ends_with(".pcx") {
                data::decode_pcx(&bytes).unwrap_or_else(|c| panic!("level {n}: {lname}: error {c:#x}"));
            } else if lname.ends_with(".cmf") {
                edpack::audio::cmf::parse_cmf(&bytes).unwrap_or_else(|e| panic!("level {n}: {lname}: {e}"));
            } else if lname.ends_with(".voc") {
                edpack::audio::voc::parse_voc(&bytes).unwrap_or_else(|e| panic!("level {n}: {lname}: {e}"));
            }
        }
    }
}

#[test]
fn inventory_and_font_decode() {
    let Some(src) = game() else { return };
    let (text, name) = data::inventory_record(&src, 2).unwrap();
    assert_eq!(edpack::cp850_to_string(&name), "Llavero calavera");
    assert!(edpack::cp850_to_string(&text).starts_with("Son las llaves de mi cuarto"));
    let (b, off) = src.entry(998).unwrap();
    let font = data::Font::new(&b[off..off + data::FONT_SIZE]);
    assert!(font.width(b"Manolo") > 0);
}

#[test]
fn walkthrough_reaches_the_end() {
    let Some(src) = game() else { return };
    let script =
        std::fs::read_to_string(PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tests/walkthrough/manolo.txt"))
            .unwrap();
    // the game's own SETUP.CFG (speed 2, text speed 2), as the original reads it
    let opts = edpack::v1::Options::from_setup_cfg(&dir().unwrap()).unwrap_or_default();
    let mut h = Harness::with_options(src, opts).unwrap();
    h.run(&script).unwrap_or_else(|e| panic!("{e}"));
    assert!(h.exited(), "the game should end with EXIT after THE END");
}

#[test]
fn save_and_load_restore_the_game() {
    let Some(src) = game() else { return };
    let script =
        std::fs::read_to_string(PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tests/walkthrough/manolo.txt"))
            .unwrap();
    let (before, after) = script.split_once("# she throws him out after a while").unwrap();
    let opts = edpack::v1::Options::from_setup_cfg(&dir().unwrap()).unwrap_or_default();
    let mut h = Harness::with_options(src, opts).unwrap();
    h.run(before).unwrap();
    let saved = (h.state_core(), h.var(1004), h.var(1311));
    h.run("save 0 catedral").unwrap();
    // play on: thrown out of the cathedral, into the bar, Reggae, Johnny...
    let more: String =
        after.lines().skip(1).take_while(|l| !l.starts_with("# The toilet")).collect::<Vec<_>>().join("\n");
    h.run(&more).unwrap();
    assert_ne!(h.state_core(), saved.0);
    h.run("load 0").unwrap();
    assert_eq!((h.state_core(), h.var(1004), h.var(1311)), saved);
}

#[test]
fn save_and_load_through_the_space_menu() {
    let Some(src) = game() else { return };
    let script =
        std::fs::read_to_string(PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tests/walkthrough/manolo.txt"))
            .unwrap();
    let (before, after) = script.split_once("# Hallway (2)").unwrap();
    let mut h = Harness::new(src).unwrap();
    h.run(before).unwrap();
    let saved = h.state_core();
    // SPACE, "Guardar", first slot, a name, Enter (the menus use the cursor's corner: hotspot - 2)
    h.run("key 57; wait 1500; tap 240 68; wait 1000; tap 100 31; wait 500; type manolo; enter; wait 1500").unwrap();
    let more: String = after.lines().skip(1).take_while(|l| !l.starts_with("# Street")).collect::<Vec<_>>().join("\n");
    h.run(&more).unwrap();
    assert_ne!(h.state_core(), saved);
    // SPACE, "Cargar", first slot
    h.run("key 57; wait 1500; tap 240 54; wait 1000; tap 100 31; wait 4000; waitidle").unwrap();
    assert_eq!(h.state_core(), saved);
}

/// The EDPACK v1 demo (RUNVGA v1.0, 12-byte LIMIT records): set EDPACK_V1DEMO
/// to its folder.
#[test]
fn v1_demo_plays_to_the_end() {
    let Some(dir) = std::env::var_os("EDPACK_V1DEMO").map(PathBuf::from) else { return };
    let src = Source::open(&dir).expect("EDPACK_V1DEMO does not point to a v1 game");
    assert_eq!(src.version(), data::Version::V100);
    let script =
        std::fs::read_to_string(PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tests/walkthrough/v1demo.txt"))
            .unwrap();
    let opts = edpack::v1::Options::from_setup_cfg(&dir).unwrap_or_default();
    let mut h = Harness::with_options(src, opts).unwrap();
    h.run(&script).unwrap_or_else(|e| panic!("{e}"));
    assert!(h.exited(), "the demo should end with EXIT after the order form");
}
