//! EDPACK v1: a re-implementation of RUNVGA.EXE v1.03, the interpreter of
//! games made with the first version of the toolkit (e.g. "Las Aventuras de
//! Manolo Kabezabolo"). See docs/v1-runvga/.
//!
//! Like the v2 engine, it models the original's data segment byte for byte
//! (script variables are pointers into it), using RUNVGA's addresses.

pub mod data;
pub mod engine;
mod game;
pub mod harness;
mod menu;
pub mod player;

pub use engine::{Engine, EngineHandle, Options};
