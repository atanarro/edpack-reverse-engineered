//! The EDPACK player: plays EDPACK v2.0 adventures in a window.
//!
//! ```text
//! edpack [EDGAME.DAT | game folder] [--scale N] [--speed 0-4] [--text-speed 0-4] [--mute] [--saves DIR]
//! ```
//! A game folder may contain EDGAME.DAT, compiled NIVEL.* files, or the .ED
//! sources with their graphics and music (compiled on the fly).  Without a
//! game argument the bundled demo, "Peter's Adventure I", is played.
//!
//! Controls are those of the original: left click acts, right click cycles
//! the verb (Ir a, Mirar, Coger, Usar, Hablar), moving the mouse to the top
//! edge opens the inventory, SPACE opens the load/save menu.

use std::path::PathBuf;
use std::process::exit;
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use cpal::traits::{DeviceTrait, HostTrait, StreamTrait};
use cpal::{FromSample, SizedSample};
use edpack::Error;
use edpack::audio::Mixer;
use edpack::engine::{DirStorage, Engine, Input, Key as GameKey, Options};
use edpack::game::Game;
use edpack::host::{Core, EngineHandle};
use minifb::{InputCallback, Key, KeyRepeat, MouseButton, MouseMode, Window, WindowOptions};

struct Args {
    game: Option<PathBuf>,
    scale: usize,
    speed: Option<usize>,
    text_speed: Option<usize>,
    mute: bool,
    saves: Option<PathBuf>,
}

fn parse_args() -> Args {
    let mut a = Args { game: None, scale: 3, speed: None, text_speed: None, mute: false, saves: None };
    let mut it = std::env::args().skip(1);
    while let Some(arg) = it.next() {
        let mut num = |max: usize| it.next().and_then(|v| v.parse::<usize>().ok()).filter(|&v| v <= max);
        match arg.as_str() {
            "--scale" => a.scale = num(8).unwrap_or(3).max(1),
            "--speed" => a.speed = num(4),
            "--text-speed" => a.text_speed = num(4),
            "--mute" => a.mute = true,
            "--saves" => a.saves = it.next().map(PathBuf::from),
            "-h" | "--help" => {
                println!(
                    "uso: edpack [EDGAME.DAT | carpeta del juego] [--scale N] [--speed 0-4] [--text-speed 0-4] [--mute] [--saves DIR]"
                );
                exit(0);
            }
            _ => a.game = Some(PathBuf::from(arg)),
        }
    }
    a
}

fn load_game(path: &Option<PathBuf>) -> Result<Game, Error> {
    if let Some(p) = path {
        return Game::open(p);
    }
    #[cfg(has_demo)]
    {
        static DEMO: &[u8] = include_bytes!(env!("EDPACK_DEMO"));
        Ok(Game::Packed(edpack::dat::GameArchive::new(DEMO.to_vec())))
    }
    #[cfg(not(has_demo))]
    Err(Error::new("indica el EDGAME.DAT o la carpeta del juego (uso: edpack <juego>)"))
}

fn saves_dir(custom: &Option<PathBuf>) -> PathBuf {
    if let Some(d) = custom {
        return d.clone();
    }
    let base = std::env::var_os("XDG_DATA_HOME")
        .map(PathBuf::from)
        .or_else(|| std::env::var_os("HOME").map(|h| PathBuf::from(h).join(".local/share")))
        .unwrap_or_else(|| PathBuf::from("."));
    base.join("edpack")
}

/// PC scan codes (the original exposes the last key pressed in variable 25).
fn scan_code(k: Key) -> u8 {
    use Key::*;
    match k {
        Escape => 1,
        Key1 => 2,
        Key2 => 3,
        Key3 => 4,
        Key4 => 5,
        Key5 => 6,
        Key6 => 7,
        Key7 => 8,
        Key8 => 9,
        Key9 => 10,
        Key0 => 11,
        Minus => 12,
        Equal => 13,
        Backspace => 14,
        Tab => 15,
        Q => 16,
        W => 17,
        E => 18,
        R => 19,
        T => 20,
        Y => 21,
        U => 22,
        I => 23,
        O => 24,
        P => 25,
        LeftBracket => 26,
        RightBracket => 27,
        Enter => 28,
        LeftCtrl => 29,
        A => 30,
        S => 31,
        D => 32,
        F => 33,
        G => 34,
        H => 35,
        J => 36,
        K => 37,
        L => 38,
        Semicolon => 39,
        Apostrophe => 40,
        Backquote => 41,
        LeftShift => 42,
        Backslash => 43,
        Z => 44,
        X => 45,
        C => 46,
        V => 47,
        B => 48,
        N => 49,
        M => 50,
        Comma => 51,
        Period => 52,
        Slash => 53,
        RightShift => 54,
        LeftAlt => 56,
        Space => 57,
        CapsLock => 58,
        F1 => 59,
        F2 => 60,
        F3 => 61,
        F4 => 62,
        F5 => 63,
        F6 => 64,
        F7 => 65,
        F8 => 66,
        F9 => 67,
        F10 => 68,
        Up => 72,
        Left => 75,
        Right => 77,
        Down => 80,
        Insert => 82,
        Delete => 83,
        _ => 0,
    }
}

struct Chars(Arc<Mutex<Vec<char>>>);

impl InputCallback for Chars {
    fn add_char(&mut self, c: u32) {
        if let Some(ch) = char::from_u32(c).filter(|c| !c.is_control()) {
            self.0.lock().unwrap().push(ch);
        }
    }
}

fn start_audio(mixer: Arc<Mutex<Mixer>>, muted: bool) -> Option<cpal::Stream> {
    if muted {
        return None;
    }
    let device = cpal::default_host().default_output_device()?;
    let supported = device.default_output_config().ok()?;
    let config = supported.config();
    mixer.lock().unwrap().out_rate = config.sample_rate as f64;
    let stream = match supported.sample_format() {
        cpal::SampleFormat::F32 => build_stream::<f32>(&device, config, mixer),
        cpal::SampleFormat::I16 => build_stream::<i16>(&device, config, mixer),
        cpal::SampleFormat::U16 => build_stream::<u16>(&device, config, mixer),
        cpal::SampleFormat::I32 => build_stream::<i32>(&device, config, mixer),
        _ => None,
    }?;
    stream.play().ok()?;
    Some(stream)
}

fn build_stream<T: SizedSample + FromSample<f32>>(
    device: &cpal::Device,
    config: cpal::StreamConfig,
    mixer: Arc<Mutex<Mixer>>,
) -> Option<cpal::Stream> {
    let channels = config.channels as usize;
    let mut mono = Vec::new();
    device
        .build_output_stream(
            config,
            move |data: &mut [T], _| {
                let frames = data.len() / channels;
                mono.resize(frames, 0.0);
                if let Ok(mut m) = mixer.lock() {
                    m.render(&mut mono);
                } else {
                    mono.fill(0.0);
                }
                for (frame, &s) in data.chunks_mut(channels).zip(&mono) {
                    for out in frame {
                        *out = T::from_sample(s);
                    }
                }
            },
            |e| eprintln!("audio: {e}"),
            None,
        )
        .ok()
}

fn main() {
    let args = parse_args();
    if let Err(e) = run(args) {
        eprintln!("{e}");
        exit(1);
    }
}

/// Speed and text speed: the command line, else the game's SETUP.CFG (bytes
/// 1 and 2, as the original reads them), else the defaults 2 and 3.
fn speeds(args: &Args) -> (usize, usize) {
    let cfg = args.game.as_ref().and_then(|g| {
        let dir = if g.is_dir() { g.as_path() } else { g.parent()? };
        let e = std::fs::read_dir(dir).ok()?.flatten().find(|e| e.file_name().eq_ignore_ascii_case("SETUP.CFG"))?;
        let b = std::fs::read(e.path()).ok()?;
        Some(((*b.get(1)? as usize).min(4), (*b.get(2)? as usize).min(4)))
    });
    (args.speed.or(cfg.map(|c| c.0)).unwrap_or(2), args.text_speed.or(cfg.map(|c| c.1)).unwrap_or(3))
}

fn run(args: Args) -> Result<(), Error> {
    let (speed, text_speed) = speeds(&args);
    let storage = Box::new(DirStorage(saves_dir(&args.saves)));
    // EDPACK v1 games (RUNVGA.EXE: RESOURCE.DAT or <base>.nnn files)
    if let Some(path) = &args.game
        && let Some(src) = edpack::v1::data::Source::open(path)
    {
        let name = path
            .canonicalize()
            .ok()
            .and_then(|p| if p.is_file() { p.parent().map(|d| d.to_path_buf()) } else { Some(p) })
            .and_then(|d| d.file_name().map(|n| n.to_string_lossy().into_owned()))
            .unwrap_or_else(|| "edpack-v1".into());
        let opts = edpack::v1::Options { speed, text_speed };
        let mut engine = edpack::v1::Engine::new(src, storage, opts)?;
        engine.save_prefix = name.clone();
        return play(engine.spawn(), &format!("EDPACK — {name}"), &args);
    }
    let game = load_game(&args.game)?;
    let opts = Options { speed, text_speed, ..Options::default() };
    let engine = Engine::new(game, storage, opts)?;
    let title = format!("EDPACK — {}", edpack::cp850_to_string(&engine.title));
    play(engine.spawn(), &title, &args)
}

fn play<E: Core>(mut handle: EngineHandle<E>, title: &str, args: &Args) -> Result<(), Error> {
    let _audio = start_audio(handle.mixer.clone(), args.mute);

    let (w, h) = (320 * args.scale, 240 * args.scale); // 4:3, like a 320x200 VGA mode on a CRT
    let mut window =
        Window::new(title, w, h, WindowOptions::default()).map_err(|e| Error::new(format!("ventana: {e}")))?;
    window.set_target_fps(60);
    window.set_cursor_visibility(false);
    let chars = Arc::new(Mutex::new(Vec::new()));
    window.set_input_callback(Box::new(Chars(chars.clone())));

    let xmap: Vec<usize> = (0..w).map(|x| x * 320 / w).collect();
    let ymap: Vec<usize> = (0..h).map(|y| y * 200 / h).collect();
    let mut buffer = vec![0u32; w * h];
    let start = Instant::now();
    let mut buttons = [false; 2];
    let mut ended_at: Option<Instant> = None;
    // Debug aid: EDPACK_SNAPSHOT=<seconds>:<file.png> saves the window and quits.
    let snapshot = std::env::var("EDPACK_SNAPSHOT").ok().and_then(|v| {
        let (t, path) = v.split_once(':')?;
        Some((t.parse::<f64>().ok()?, PathBuf::from(path)))
    });

    while window.is_open() {
        let mut events = Vec::new();
        if let Some((mx, my)) = window.get_mouse_pos(MouseMode::Clamp) {
            let gx = ((mx as usize).min(w - 1) * 320 / w) as i32;
            let gy = ((my as usize).min(h - 1) * 200 / h) as i32;
            events.push(Input::MouseMove(gx, gy));
        }
        for (i, b) in [MouseButton::Left, MouseButton::Right].into_iter().enumerate() {
            let down = window.get_mouse_down(b);
            if down != buttons[i] {
                buttons[i] = down;
                events.push(Input::Button(i, down));
            }
        }
        for k in window.get_keys_pressed(KeyRepeat::No) {
            let special = match k {
                Key::Enter | Key::NumPadEnter => Some(GameKey::Enter),
                Key::Escape => Some(GameKey::Escape),
                Key::Backspace => Some(GameKey::Backspace),
                _ => None,
            };
            events.push(Input::KeyDown(scan_code(k), special));
        }
        for c in chars.lock().unwrap().drain(..) {
            events.push(Input::KeyDown(0, Some(GameKey::Char(c))));
        }
        for k in window.get_keys_released() {
            events.push(Input::KeyUp(scan_code(k)));
        }
        if window.is_key_down(Key::LeftCtrl) && window.is_key_down(Key::C) {
            break; // Ctrl+C ends the game, like the original
        }

        let now = start.elapsed().as_secs_f64() * 1000.0;
        let Some(frame) = handle.advance(now, events) else { break };
        let mut lut = [0u32; 256];
        for (i, c) in lut.iter_mut().enumerate() {
            let p = &frame.palette[i * 3..i * 3 + 3];
            *c = (p[0] as u32) << 16 | (p[1] as u32) << 8 | p[2] as u32;
        }
        for (y, row) in buffer.chunks_mut(w).enumerate() {
            let src = &frame.screen[ymap[y] * 320..ymap[y] * 320 + 320];
            for (x, px) in row.iter_mut().enumerate() {
                *px = lut[src[xmap[x]] as usize];
            }
        }
        window.update_with_buffer(&buffer, w, h).map_err(|e| Error::new(format!("ventana: {e}")))?;
        if let Some((t, path)) = &snapshot
            && now >= t * 1000.0
        {
            let rgb: Vec<u8> = buffer.iter().flat_map(|p| [(p >> 16) as u8, (p >> 8) as u8, *p as u8]).collect();
            let _ = edpack::png_out::save_rgb(path, w, h, &rgb);
            break;
        }

        if frame.exited {
            if let Some(err) = &frame.error {
                window.set_title(&format!("EDPACK — error: {err}"));
                eprintln!("{err}");
                // keep the window open so the player can read where it stopped
                while window.is_open() && !window.is_key_down(Key::Escape) {
                    window.update();
                    std::thread::sleep(Duration::from_millis(30));
                }
                break;
            }
            let t = *ended_at.get_or_insert_with(Instant::now);
            if t.elapsed() > Duration::from_millis(800) {
                break;
            }
        }
    }
    Ok(())
}
