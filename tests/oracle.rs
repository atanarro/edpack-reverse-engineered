//! Differential tests against the original interpreter, one function at a time.
//!
//! `tools/re/oracle.py` runs a single function of RUNDAT.EXE in an emulator on
//! fuzzed data-segment contents and records, per case, the DS bytes the original
//! read or wrote and the bytes it changed (`tests/oracle/<prog>_<addr>.vec`).
//! Each case is replayed here on a zeroed `Memory`: the Rust function must change
//! exactly the same bytes to the same values. The vectors are derived from the
//! original executables, which are not part of this repository; see
//! `tools/re/README.md` to generate them. Without them these tests are skipped.

use std::collections::BTreeMap;
use std::path::PathBuf;

use edpack::memory::Memory;
use edpack::player;

struct Case {
    args: Vec<i32>,
    input: Vec<(usize, u8)>,
    output: BTreeMap<usize, u8>,
    fatal: Option<i32>,
}

fn load(name: &str) -> Option<Vec<Case>> {
    let dir = std::env::var_os("EDPACK_ORACLE")
        .map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tests/oracle"));
    let text = std::fs::read_to_string(dir.join(format!("{name}.vec"))).ok()?;
    let pairs = |s: &str| -> Vec<(usize, u8)> {
        s.split_whitespace()
            .map(|p| {
                let (a, v) = p.split_once('=').unwrap();
                (usize::from_str_radix(a, 16).unwrap(), u8::from_str_radix(v, 16).unwrap())
            })
            .collect()
    };
    let mut cases = Vec::new();
    let mut cur = None;
    for line in text.lines() {
        let (key, rest) = line.split_once(' ').unwrap_or((line, ""));
        match key {
            "case" => {
                let args = rest.split_whitespace().map(|a| a.parse().unwrap()).collect();
                cur = Some(Case { args, input: vec![], output: BTreeMap::new(), fatal: None });
            }
            "in" => cur.as_mut().unwrap().input = pairs(rest),
            "out" => cur.as_mut().unwrap().output = pairs(rest).into_iter().collect(),
            "fatal" => cur.as_mut().unwrap().fatal = Some(rest.parse().unwrap()),
            "end" => cases.push(cur.take().unwrap()),
            _ => {}
        }
    }
    Some(cases)
}

/// Replays every case of `name` through `f`. Cases where the original stopped
/// with a fatal error are only counted: the shareware runtime's level limit is
/// not reproduced on purpose.
fn check(name: &str, f: impl Fn(&mut Memory, &[i32])) {
    let Some(cases) = load(name) else {
        eprintln!("skipped: no oracle vectors for {name} (see tools/re/README.md)");
        return;
    };
    assert!(!cases.is_empty(), "{name}: no cases");
    let (mut failures, mut fatal) = (Vec::new(), 0);
    for (i, c) in cases.iter().enumerate() {
        if c.fatal.is_some() {
            fatal += 1;
            continue;
        }
        let mut m = Memory::new();
        for &(a, v) in &c.input {
            m.bytes[a] = v;
        }
        let before = m.bytes[..0x10000].to_vec();
        f(&mut m, &c.args);
        let got: BTreeMap<usize, u8> =
            (0..0x10000).filter(|&a| m.bytes[a] != before[a]).map(|a| (a, m.bytes[a])).collect();
        if got != c.output {
            let fmt = |d: &BTreeMap<usize, u8>| {
                d.iter().map(|(a, v)| format!("{a:04x}={v:02x}")).collect::<Vec<_>>().join(" ")
            };
            failures.push(format!(
                "case {i} args {:?}\n  original: {}\n  rust:     {}",
                c.args,
                fmt(&c.output),
                fmt(&got)
            ));
        }
    }
    eprintln!("{name}: {} cases, {} fatal (skipped), {} mismatches", cases.len(), fatal, failures.len());
    assert!(
        failures.is_empty(),
        "{name}: {} of {} cases differ from the original:\n{}",
        failures.len(),
        cases.len(),
        failures[..failures.len().min(5)].join("\n")
    );
}

#[test]
fn scalemov_685f() {
    check("rundat_685f", |m, _| player::apply_scale_mov(m));
}

#[test]
fn shadowmov_6930() {
    check("rundat_6930", |m, _| player::apply_shadow(m));
}

#[test]
fn layer_swap_5606() {
    check("rundat_5606", |m, _| player::layer_swap(m));
}

#[test]
fn goxy_7fdc() {
    check("rundat_7fdc", |m, a| player::go_xy(m, a[0], a[1]));
}
