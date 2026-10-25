use edpack::dat::ByteReader;
use edpack::memory::{Memory, ds};

#[test]
fn rle_reader_matches_the_original_rules() {
    // 0xC0 is a literal, 0xC3 = run of 3
    let mut r = ByteReader::new(&[0xc0, 0xc3, 7, 1, 0xff, 9], 0);
    assert_eq!(r.rle(5).unwrap(), vec![0xc0, 7, 7, 7, 1]);
    // runs are truncated at the block end
    assert_eq!(ByteReader::new(&[0xc5, 2], 0).rle(3).unwrap(), vec![2, 2, 2]);
}

#[test]
fn variables_alias_the_original_memory_layout() {
    let mut m = Memory::new();
    m.set_var(100 + 5 * 30 + 5, 155); // THING 5 POS x
    assert_eq!(m.i16(ds::THINGS + 5 * 92 + 0x19), 155);
    m.set_var(1500, -7);
    assert_eq!(m.get_var(1500), -7);
    // var 29 (info bar) is a byte field; a word write spills into the next byte
    m.set8(0xfd02, 0x55);
    m.set_var(29, 1);
    assert_eq!(m.u8(0xfd02), 0);
}

#[test]
fn cp850_round_trip() {
    for b in 0x20u8..=0xff {
        if b == 0x7f {
            continue;
        }
        let s = edpack::cp850_to_string(&[b]);
        assert_eq!(s.chars().count(), 1);
        assert_eq!(edpack::char_to_cp850(s.chars().next().unwrap()), Some(b), "byte {b:#x}");
    }
}
