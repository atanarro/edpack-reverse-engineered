//! EDPACK v2.0 in Rust: a modern re-implementation of the 1990s Spanish DOS
//! graphic-adventure toolkit by José Luis Serrano Moreno.
//!
//! * [`engine`] plays games like RUNDAT.EXE / RUNFILES.EXE did,
//! * [`compiler`] replaces ED.EXE (sources -> NIVEL.*) and ED2DAT.EXE (packing),
//! * [`audio`] emulates the Sound Blaster FM chip for CMF music and plays VOC sound.
//!
//! The engine models the original interpreter's data segment byte for byte
//! ([`memory`]) so script variables alias engine state exactly as in DOS.

pub mod audio;
pub mod compiler;
pub mod dat;
pub mod engine;
pub mod fli;
pub mod font;
pub mod game;
pub mod harness;
pub mod host;
pub mod level;
pub mod memory;
pub mod menu;
pub mod pcx;
pub mod player;
pub mod png_out;
pub mod specials;
pub mod ui;
pub mod v1;
pub mod vm;

use std::fmt;

/// Error with a user-facing (Spanish, like the original) message.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Error(pub String);

impl Error {
    pub fn new(msg: impl Into<String>) -> Self {
        Error(msg.into())
    }
}

impl fmt::Display for Error {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.0)
    }
}

impl std::error::Error for Error {}

/// Code page 850 bytes -> Rust string (for messages and tools).
pub fn cp850_to_string(b: &[u8]) -> String {
    const HIGH: &str = "ÇüéâäàåçêëèïîìÄÅÉæÆôöòûùÿÖÜø£Ø×ƒáíóúñÑªº¿®¬½¼¡«»░▒▓│┤ÁÂÀ©╣║╗╝¢¥┐└┴┬├─┼ãÃ╚╔╩╦╠═╬¤ðÐÊËÈıÍÎÏ┘┌█▄¦Ì▀ÓßÔÒõÕµþÞÚÛÙýÝ¯´\u{ad}±‗¾¶§÷¸°¨·¹³²■\u{a0}";
    let high: Vec<char> = HIGH.chars().collect();
    b.iter().map(|&c| if c < 0x80 { c as char } else { high[(c - 0x80) as usize] }).collect()
}

/// Rust char -> code page 850 byte, for text typed by the user.
pub fn char_to_cp850(ch: char) -> Option<u8> {
    let c = ch as u32;
    if (0x20..0x7f).contains(&c) {
        return Some(c as u8);
    }
    (0x80u8..=0xff).find(|&b| cp850_to_string(&[b]).starts_with(ch))
}
