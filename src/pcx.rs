//! ZSoft PCX decoder for the 320x200x256 images used by EDPACK.

use crate::Error;

pub struct Image {
    pub width: usize,
    pub height: usize,
    /// width*height palette indices
    pub pixels: Vec<u8>,
    /// 768 bytes, 8-bit RGB
    pub palette: Vec<u8>,
}

pub fn decode_pcx(bytes: &[u8]) -> Result<Image, Error> {
    if bytes.len() < 128 + 769 || bytes[0] != 0x0a {
        return Err(Error::new("Fichero no es un PCX."));
    }
    let w16 = |o: usize| u16::from_le_bytes([bytes[o], bytes[o + 1]]) as usize;
    let (bpp, planes, bpl) = (bytes[3], bytes[65], w16(66));
    if bpp != 8 || planes != 1 {
        return Err(Error::new("El archivo de graficos no es del formato 320*200 256 colores."));
    }
    let width = w16(8).saturating_sub(w16(4)) + 1;
    let height = w16(10).saturating_sub(w16(6)) + 1;
    let mut pixels = vec![0u8; width * height];
    let mut line = vec![0u8; bpl.max(width)];
    let mut p = 128;
    for y in 0..height {
        let mut x = 0;
        while x < bpl && p < bytes.len() {
            let c = bytes[p];
            p += 1;
            if c & 0xc0 == 0xc0 {
                let n = (c & 0x3f) as usize;
                let v = bytes.get(p).copied().unwrap_or(0);
                p += 1;
                for _ in 0..n {
                    if x < bpl {
                        line[x] = v;
                        x += 1;
                    }
                }
            } else {
                line[x] = c;
                x += 1;
            }
        }
        pixels[y * width..(y + 1) * width].copy_from_slice(&line[..width]);
    }
    let mut palette = vec![0u8; 768];
    let pal_start = bytes.len() - 769;
    if bytes[pal_start] == 0x0c {
        palette.copy_from_slice(&bytes[pal_start + 1..]);
    }
    Ok(Image { width, height, pixels, palette })
}
