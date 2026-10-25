//! Autodesk Animator FLI/FLC decoder (320x200, 256 colours).

use crate::Error;

pub struct FliDecoder {
    bytes: Vec<u8>,
    pub frames: usize,
    pub width: usize,
    pub height: usize,
    first_frame: usize,
    pos: usize,
    index: usize,
    pub pixels: Vec<u8>,
    /// 8-bit RGB
    pub palette: Vec<u8>,
}

impl FliDecoder {
    pub fn new(bytes: Vec<u8>) -> Result<FliDecoder, Error> {
        if bytes.len() < 128 {
            return Err(Error::new("El archivo no es un .FLI"));
        }
        let w16 = |o: usize| u16::from_le_bytes([bytes[o], bytes[o + 1]]) as usize;
        let magic = w16(4);
        if magic != 0xaf11 && magic != 0xaf12 {
            return Err(Error::new("El archivo no es un .FLI"));
        }
        let is_flc = magic == 0xaf12;
        let (frames, width, height) = (w16(6), w16(8).max(1), w16(10).max(1));
        let off80 = u32::from_le_bytes([bytes[80], bytes[81], bytes[82], bytes[83]]) as usize;
        let first_frame = if is_flc && off80 != 0 { off80 } else { 128 };
        Ok(FliDecoder {
            bytes,
            frames,
            width,
            height,
            first_frame,
            pos: first_frame,
            index: 0,
            pixels: vec![0; width * height],
            palette: vec![0; 768],
        })
    }

    pub fn rewind(&mut self) {
        self.pos = self.first_frame;
        self.index = 0;
    }

    fn b(&self, p: usize) -> u8 {
        self.bytes.get(p).copied().unwrap_or(0)
    }
    fn w(&self, p: usize) -> usize {
        self.b(p) as usize | (self.b(p + 1) as usize) << 8
    }
    fn d(&self, p: usize) -> usize {
        self.w(p) | self.w(p + 2) << 16
    }
    fn put(&mut self, i: usize, v: u8) {
        if let Some(p) = self.pixels.get_mut(i) {
            *p = v;
        }
    }

    /// Decode the next frame; false after the last one.
    pub fn next_frame(&mut self) -> bool {
        if self.index >= self.frames {
            return false;
        }
        loop {
            if self.pos + 16 > self.bytes.len() {
                return false;
            }
            if self.w(self.pos + 4) == 0xf1fa {
                break;
            }
            self.pos += self.d(self.pos).max(16);
        }
        let size = self.d(self.pos);
        let chunks = self.w(self.pos + 6);
        let mut p = self.pos + 16;
        for _ in 0..chunks {
            let (csize, ctype) = (self.d(p), self.w(p + 4));
            self.chunk(ctype, p + 6);
            p += csize.max(6);
        }
        self.pos += size.max(16);
        self.index += 1;
        true
    }

    fn chunk(&mut self, kind: usize, mut p: usize) {
        let (wd, ht) = (self.width, self.height);
        match kind {
            4 | 11 => {
                let packets = self.w(p);
                p += 2;
                let mut idx = 0usize;
                for _ in 0..packets {
                    idx += self.b(p) as usize;
                    let mut n = self.b(p + 1) as usize;
                    p += 2;
                    if n == 0 {
                        n = 256;
                    }
                    for _ in 0..n {
                        if idx >= 256 {
                            break;
                        }
                        for c in 0..3 {
                            let v = self.b(p) as u32;
                            p += 1;
                            self.palette[idx * 3 + c] =
                                if kind == 11 { ((v * 4) | ((v * 4) >> 6)).min(255) as u8 } else { v as u8 };
                        }
                        idx += 1;
                    }
                }
            }
            13 => self.pixels.fill(0),
            16 => {
                for i in 0..wd * ht {
                    let v = self.b(p + i);
                    self.put(i, v);
                }
            }
            15 => {
                for y in 0..ht {
                    p += 1;
                    let mut x = 0;
                    while x < wd {
                        let n = self.b(p) as i8 as i32;
                        p += 1;
                        if n >= 0 {
                            let v = self.b(p);
                            p += 1;
                            for _ in 0..n {
                                self.put(y * wd + x, v);
                                x += 1;
                            }
                        } else {
                            for _ in 0..-n {
                                let v = self.b(p);
                                p += 1;
                                self.put(y * wd + x, v);
                                x += 1;
                            }
                        }
                        if p > self.bytes.len() {
                            return;
                        }
                    }
                }
            }
            12 => {
                let mut y = self.w(p);
                let lines = self.w(p + 2);
                p += 4;
                for _ in 0..lines {
                    let mut x = 0;
                    let packets = self.b(p);
                    p += 1;
                    for _ in 0..packets {
                        x += self.b(p) as usize;
                        let n = self.b(p + 1) as i8 as i32;
                        p += 2;
                        if n >= 0 {
                            for _ in 0..n {
                                let v = self.b(p);
                                p += 1;
                                self.put(y * wd + x, v);
                                x += 1;
                            }
                        } else {
                            let v = self.b(p);
                            p += 1;
                            for _ in 0..-n {
                                self.put(y * wd + x, v);
                                x += 1;
                            }
                        }
                    }
                    y += 1;
                }
            }
            7 => {
                let mut lines = self.w(p);
                p += 2;
                let mut y: i64 = 0;
                while lines > 0 && p < self.bytes.len() {
                    let w = self.w(p) as u16 as i16 as i32;
                    p += 2;
                    if w < 0 {
                        if w & 0x4000 != 0 {
                            y -= w as i64;
                        } else {
                            self.put((y as usize) * wd + wd - 1, (w & 0xff) as u8);
                        }
                        continue;
                    }
                    let mut x = 0;
                    for _ in 0..w {
                        x += self.b(p) as usize;
                        let n = self.b(p + 1) as i8 as i32;
                        p += 2;
                        let row = y as usize * wd;
                        if n >= 0 {
                            for _ in 0..n * 2 {
                                let v = self.b(p);
                                p += 1;
                                self.put(row + x, v);
                                x += 1;
                            }
                        } else {
                            let (lo, hi) = (self.b(p), self.b(p + 1));
                            p += 2;
                            for _ in 0..-n {
                                self.put(row + x, lo);
                                self.put(row + x + 1, hi);
                                x += 2;
                            }
                        }
                    }
                    y += 1;
                    lines -= 1;
                }
            }
            _ => {}
        }
    }
}
