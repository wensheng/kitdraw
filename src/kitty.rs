use std::io::{self, Write};
use std::{thread, time::Duration};

use anyhow::Result;
use base64::{Engine, prelude::BASE64_STANDARD};
use flate2::{Compression, write::ZlibEncoder};
use image::RgbaImage;

const KITTY_CHUNK_BYTES: usize = 4096;

pub fn write_frame<W: Write>(
    writer: &mut W,
    image: &RgbaImage,
    cols: u16,
    rows: u16,
    prevent_cursor_move: bool,
) -> Result<()> {
    write_rgba_frame(
        writer,
        image.as_raw(),
        image.width(),
        image.height(),
        u32::from(cols.max(1)),
        u32::from(rows.max(1)),
        prevent_cursor_move,
    )?;
    Ok(())
}

pub fn write_rgba_frame<W: Write>(
    writer: &mut W,
    pixels: &[u8],
    width_px: u32,
    height_px: u32,
    cols: u32,
    rows: u32,
    prevent_cursor_move: bool,
) -> io::Result<()> {
    let mut encoder = ZlibEncoder::new(Vec::new(), Compression::fast());
    encoder.write_all(pixels)?;
    let compressed = encoder.finish()?;
    let encoded = BASE64_STANDARD.encode(&compressed);
    let bytes = encoded.as_bytes();
    let mut offset = 0;

    while offset < bytes.len() {
        let end = (offset + KITTY_CHUNK_BYTES).min(bytes.len());
        let is_last = end == bytes.len();
        let more = if is_last { 0 } else { 1 };
        let mut packet = Vec::new();
        if offset == 0 {
            let cursor_policy = if prevent_cursor_move { ",C=1" } else { "" };
            write!(
                packet,
                "\x1b_Ga=T,f=32,o=z,s={},v={},c={},r={}{},q=2,m={};",
                width_px, height_px, cols, rows, cursor_policy, more
            )?;
        } else {
            write!(packet, "\x1b_Gq=2,m={};", more)?;
        }
        packet.write_all(&bytes[offset..end])?;
        packet.write_all(b"\x1b\\")?;
        write_all_robust(writer, &packet)?;
        offset = end;
    }
    flush_robust(writer)
}

pub fn write_all_robust<W: Write>(writer: &mut W, mut buffer: &[u8]) -> io::Result<()> {
    while !buffer.is_empty() {
        match writer.write(buffer) {
            Ok(0) => {
                return Err(io::Error::new(
                    io::ErrorKind::WriteZero,
                    "failed to write whole buffer",
                ));
            }
            Ok(bytes) => buffer = &buffer[bytes..],
            Err(error) if error.kind() == io::ErrorKind::WouldBlock => {
                thread::sleep(Duration::from_millis(1));
            }
            Err(error) if error.raw_os_error() == Some(35) => {
                thread::sleep(Duration::from_millis(1));
            }
            Err(error) => return Err(error),
        }
    }
    Ok(())
}

pub fn flush_robust<W: Write>(writer: &mut W) -> io::Result<()> {
    loop {
        match writer.flush() {
            Ok(()) => return Ok(()),
            Err(error) if error.kind() == io::ErrorKind::WouldBlock => {
                thread::sleep(Duration::from_millis(1));
            }
            Err(error) if error.raw_os_error() == Some(35) => {
                thread::sleep(Duration::from_millis(1));
            }
            Err(error) => return Err(error),
        }
    }
}

#[cfg(test)]
mod tests {
    use image::{Rgba, RgbaImage};

    use super::*;

    #[test]
    fn emits_rgba_kitty_frame() {
        let image = RgbaImage::from_pixel(2, 2, Rgba([255, 0, 0, 255]));
        let mut out = Vec::new();
        write_frame(&mut out, &image, 8, 4, true).unwrap();
        let text = String::from_utf8_lossy(&out);
        assert!(text.contains("\u{1b}_Ga=T"));
        assert!(text.contains("f=32"));
        assert!(text.contains("o=z"));
        assert!(text.contains("s=2"));
        assert!(text.contains("v=2"));
        assert!(text.contains("c=8"));
        assert!(text.contains("r=4"));
        assert!(text.contains("C=1"));
        assert!(text.contains("q=2"));
    }

    #[test]
    fn chunks_large_frames() {
        let image = RgbaImage::from_fn(128, 128, |x, y| {
            let seed = x
                .wrapping_mul(1_103_515_245)
                .wrapping_add(y.wrapping_mul(12_345));
            Rgba([
                (seed & 0xff) as u8,
                ((seed >> 8) & 0xff) as u8,
                ((seed >> 16) & 0xff) as u8,
                255,
            ])
        });
        let mut out = Vec::new();
        write_frame(&mut out, &image, 64, 32, true).unwrap();
        let text = String::from_utf8_lossy(&out);
        let chunks: Vec<&str> = text.split("\u{1b}_G").skip(1).collect();
        assert!(chunks.len() > 1);
        assert!(chunks.iter().all(|chunk| chunk.contains("q=2")));
        assert!(chunks.first().unwrap().contains("m=1"));
        assert!(chunks.last().unwrap().contains("m=0"));
    }
}
