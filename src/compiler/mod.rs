//! Replacements for ED.EXE (compiler) and ED2DAT.EXE (packer).

pub mod codegen;
pub mod compile;
pub mod limitmov;
pub mod pack;
pub mod parser;

pub use compile::{CompileOptions, Target, compile_project, compile_project_with, rle_encode};
pub use pack::pack_game;
