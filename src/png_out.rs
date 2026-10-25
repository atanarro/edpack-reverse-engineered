//! Writing indexed 320x200 frames as PNG (tools and tests).

use std::io::BufWriter;
use std::path::Path;

/// Save `pixels` (indexed) with an 8-bit RGB `palette`, scaled by `scale`.
pub fn save_png(
    path: &Path,
    width: usize,
    height: usize,
    pixels: &[u8],
    palette: &[u8],
    scale: usize,
) -> std::io::Result<()> {
    let (w, h) = (width * scale, height * scale);
    let mut rgb = Vec::with_capacity(w * h * 3);
    for y in 0..h {
        for x in 0..w {
            let i = pixels[(y / scale) * width + x / scale] as usize * 3;
            rgb.extend_from_slice(&palette[i..i + 3]);
        }
    }
    save_rgb(path, w, h, &rgb)
}

/// Save 8-bit RGB pixels as PNG.
pub fn save_rgb(path: &Path, w: usize, h: usize, rgb: &[u8]) -> std::io::Result<()> {
    let file = std::fs::File::create(path)?;
    let mut enc = png::Encoder::new(BufWriter::new(file), w as u32, h as u32);
    enc.set_color(png::ColorType::Rgb);
    enc.set_depth(png::BitDepth::Eight);
    let mut writer = enc.write_header().map_err(std::io::Error::other)?;
    writer.write_image_data(rgb).map_err(std::io::Error::other)?;
    Ok(())
}
