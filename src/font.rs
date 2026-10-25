//! EDLETRAS bitmap fonts (.FNT / NIVEL.FNT).
//!
//! `u8 space_width, u8 separation`, then an RLE block (level format) of 256
//! glyphs x 34 bytes: byte 0 = `(width << 4) | height`, then 2 bits per
//! pixel, MSB first, continuous across rows: 1 = text colour, 2 = border
//! colour, 0/3 = transparent.  The outline is part of the glyph artwork.

use crate::Error;
use crate::dat::ByteReader;

pub const LINE_HEIGHT: i32 = 11;

pub struct Font {
    pub space: i32,
    pub sep: i32,
    glyphs: Vec<u8>,
}

impl Font {
    pub fn new(bytes: &[u8]) -> Result<Font, Error> {
        if bytes.len() < 3 {
            return Err(Error::new("No encuentro el fichero de LETRAS 'NIVEL.FNT'."));
        }
        let glyphs = ByteReader::new(bytes, 2).rle(256 * 34)?;
        Ok(Font { space: bytes[0] as i32, sep: bytes[1] as i32, glyphs })
    }

    pub fn size(&self, ch: u8) -> (i32, i32) {
        let b = self.glyphs[ch as usize * 34];
        ((b >> 4) as i32, (b & 15) as i32)
    }

    pub fn advance(&self, ch: u8) -> i32 {
        if ch == 0x20 {
            return self.space;
        }
        let (w, h) = self.size(ch);
        if w != 0 && h != 0 { w + self.sep - 1 } else { 0 }
    }

    /// TextWidth (0x78a8): sum of advances minus one separation.
    pub fn width(&self, s: &[u8]) -> i32 {
        s.iter().map(|&c| self.advance(c)).sum::<i32>() - self.sep
    }

    /// Draw one glyph (0x261), clipped to the 320x200 screen.
    pub fn glyph(&self, dst: &mut [u8], x: i32, y: i32, ch: u8, col: u8, border: u8) {
        let g = ch as usize * 34;
        let (w, h) = self.size(ch);
        if w == 0 || h == 0 {
            return;
        }
        let mut bit = 0usize;
        for r in 0..h {
            for c in 0..w {
                let v = (self.glyphs[g + 1 + (bit >> 2)] >> (6 - 2 * (bit & 3))) & 3;
                bit += 1;
                if v != 1 && v != 2 {
                    continue;
                }
                let (px, py) = (x + c, y + r);
                if !(0..320).contains(&px) || !(0..200).contains(&py) {
                    continue;
                }
                dst[(py * 320 + px) as usize] = if v == 1 { col } else { border };
            }
        }
    }

    /// DrawText (0x70d0): no line breaks.
    pub fn text(&self, dst: &mut [u8], mut x: i32, y: i32, s: &[u8], col: u8, border: u8) {
        for &ch in s {
            if ch != 0x20 {
                self.glyph(dst, x, y, ch, col, border);
            }
            x += self.advance(ch);
        }
    }

    /// DrawTextClip (0x7168): ' ' and '^' are spaces, stops before x 318.
    pub fn text_clip(&self, dst: &mut [u8], mut x: i32, y: i32, s: &[u8], col: u8, border: u8) {
        for &c in s {
            let ch = if c == b'^' { b' ' } else { c };
            if ch == b' ' {
                if x + self.space > 318 {
                    return;
                }
                x += self.space;
                continue;
            }
            let (w, _) = self.size(ch);
            if x + w + self.sep - 1 > 318 {
                return;
            }
            self.glyph(dst, x, y, ch, col, border);
            x += self.advance(ch);
        }
    }
}
