//! Creative Voice File (.VOC) parser -> mono f32 PCM.

use crate::Error;

#[derive(Clone)]
pub struct Voice {
    pub rate: f64,
    pub samples: Vec<f32>,
}

pub fn parse_voc(b: &[u8]) -> Result<Voice, Error> {
    if b.len() < 26 || &b[..19] != b"Creative Voice File" {
        return Err(Error::new("No es un archivo VOC"));
    }
    let at = |p: usize| b.get(p).copied().unwrap_or(0) as usize;
    let mut p = at(20) | at(21) << 8;
    let mut rate = 11025.0;
    let mut samples = Vec::new();
    let mut push = |s: &[u8]| samples.extend(s.iter().map(|&x| (x as f32 - 128.0) / 128.0));
    while p < b.len() {
        let kind = b[p];
        if kind == 0 {
            break;
        }
        let len = at(p + 1) | at(p + 2) << 8 | at(p + 3) << 16;
        let d = p + 4;
        let end = (d + len).min(b.len());
        match kind {
            1 => {
                rate = (1e6 / (256.0 - at(d) as f64)).round();
                if at(d + 1) == 0 && d + 2 <= end {
                    push(&b[d + 2..end]);
                }
            }
            2 => push(&b[d.min(end)..end]),
            3 => {
                let n = (at(d) | at(d + 1) << 8) + 1;
                rate = (1e6 / (256.0 - at(d + 2) as f64)).round();
                push(&vec![128u8; n]);
            }
            9 => {
                rate = (at(d) | at(d + 1) << 8 | at(d + 2) << 16 | at(d + 3) << 24) as f64;
                if at(d + 4) == 8 && (at(d + 6) | at(d + 7) << 8) == 0 && d + 12 <= end {
                    push(&b[d + 12..end]);
                }
            }
            _ => {}
        }
        p = d + len;
    }
    Ok(Voice { rate, samples })
}
