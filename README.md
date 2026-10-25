# EDPACK in Rust

A modern re-implementation of **EDPACK v2.0** ("Editor de Aventuras Gráficas", José Luis
Serrano Moreno, 1990s), the Spanish DOS toolkit for point-and-click graphic adventures in this
repository. It plays the games made with it natively, and it also replaces the DOS-only
toolchain.

It is based on the shareware version of the Editor de Aventuras Gráficas (EDPACK) v2.02. That
version was distributed for free on the CD-ROM included with *PCManía* magazine issue 67 (May
1998), in the folder `sharewar\varios\edpack20\JUEGO`
([ISO image](https://archive.org/download/PCMCDS/PCMania%20CD67_1.iso)).

| Original (DOS) | Replacement | What it does |
|---|---|---|
| `RUNDAT.EXE` | `edpack` | Plays a packed `EDGAME.DAT` |
| `RUNFILES.EXE` | `edpack <folder>` | Plays compiled `NIVEL.*` files (or `.ED` sources, compiled on the fly) |
| `ED.EXE` | `edc <src> <out>` | Compiles `.ED` sources to `NIVEL.*` |
| `ED2DAT.EXE` | `edc <src> <out> --pack` | Packs a game into `EDGAME.DAT` |
| `SBFMDRV.COM`, `CT-VOICE.DRV` | `src/audio/` | OPL2 FM emulator for CMF music, VOC playback |

The engine was built by reverse-engineering the original interpreter. The included demo,
*Peter's Adventure I*, exists both compiled (`../DEMO/EDGAME.DAT`) and as sources
(`../FUENTES`). The test suite plays it to the end from both, and from an `EDGAME.DAT` built by
`edc --pack`, and checks that all three runs end at the same simulated moment.

## Building

You need Rust (stable). On Linux you also need the ALSA development headers for sound
(`sudo apt install libasound2-dev`). The window uses X11 (works under XWayland).

```sh
cargo build --release
```

## Playing

```sh
target/release/edpack                    # the bundled demo
target/release/edpack ../DEMO/EDGAME.DAT # any EDPACK game
target/release/edpack ../FUENTES         # a folder: EDGAME.DAT, NIVEL.* files or .ED sources
```

Options:

| Option | Meaning |
|---|---|
| `--scale N` | Window size: 320×240×N (4:3) |
| `--speed 0-4` | Game speed (SETUP.CFG speed; default 2) |
| `--text-speed 0-4` | Text speed (SETUP.CFG text speed; default 3) |
| `--mute` | No sound |
| `--saves DIR` | Where savegames go (default `~/.local/share/edpack/<title>/SAVEGAME._N_`) |

The controls are those of the original:

| Input | Action |
|---|---|
| Left click | Act |
| Right click | Cycle the verb: *Ir a*, *Mirar*, *Coger*, *Usar*, *Hablar* |
| Mouse to the top edge | Open the inventory. *Usar* on an object picks it up, then use it on something. |
| Space | Load / save / quit menu |
| Ctrl+C | Quit |

## Tools

```sh
target/release/edc ../FUENTES out --pack                 # ED.EXE + ED2DAT.EXE
target/release/edtool run ../DEMO/EDGAME.DAT shots tests/walkthrough/peter.txt   # headless play + PNGs
target/release/edtool disasm ../DEMO/EDGAME.DAT 3        # decompile level 3's scripts
target/release/edtool dump ../DEMO/EDGAME.DAT            # summary of every level
target/release/edtool cmf2wav ../FUENTES/SATAIR.CMF satair.wav
cargo test --release                                     # unit and end-to-end tests
```

For debugging, `EDPACK_SNAPSHOT=<seconds>:<file.png> edpack ...` saves the window after that
many seconds and quits.

## How it works

- **Memory model** (`memory.rs`). Script variables are pointers into the interpreter's global
  data, and many of them overlap byte fields (writing variable 31 also clears variable 30, and
  so on). The engine keeps its state in a 64 KiB array laid out like the original DS segment, so
  these side effects happen exactly as in DOS.
- **Formats** (`dat.rs`, `level.rs`, `specials.rs`, `font.rs`, `pcx.rs`, `fli.rs`). Supported:
  - `EDGAME.DAT`: offset table; resources are stored +5.
  - Compiled levels: a 30-byte header followed by RLE blocks of fixed-size tables.
  - NIVEL.INV, NIVEL.MOV, NIVEL.MOU, NIVEL.IAC, NIVEL.TIT.
  - EDLETRAS fonts, PCX, Autodesk FLI, CMF, VOC.

  Strings are obfuscated with a different key per kind of string.
- **Engine threading** (`engine.rs`). The original is a blocking DOS program: a main loop that
  waits two VGA retraces per pass (about 28.5 ms), a 50 Hz timer interrupt, and routines with
  their own loops (fades, FLI playback, menus). The engine keeps that structure and runs on its
  own thread. Whenever time must pass it hands the frame to the host and waits for the clock to
  advance. Host and engine never run at the same time, so play is deterministic for a given
  input sequence.
- **Interpreter** (`Engine::run_script`). A port of the bytecode interpreter: 66 opcodes, an
  8-bit program counter, and arguments ≥ 25000 dereferenced as variables. `DELAY` keeps its
  counter inside the bytecode, and the 51-operation runaway check is kept.
- **Player** (`player.rs`). Path planning follows the original: LIMIT redirection, then the
  three-pass LIMITMOV rule search. It also covers the walker with its frame rules, the float32
  nearest-neighbour downscaler, SCALEMOV/SHADOWMOV, and layer swapping.
- **UI** (`ui.rs`, `menu.rs`). Speech layout and timing, the info bar, STEXT dialogue choices,
  the inventory panel, object combination (ACINVINV) and use-on-thing (ACINVTH), and the
  load/save menu.
- **Compiler** (`compiler/`). The `.ED` parser, script lowering, and the LIMITMOV rule
  generator. Scripts, LIMITMOV tables, texts, doors and the MOV/MOU/IAC/TIT files are identical
  to what the original ED.EXE produced for the demo.

Some quirks of the original are reproduced on purpose, because games were tested against them:

- `SEEINV` always returns 0.
- `NEWMOUSE` uses a 60-byte stride, so only cursor set 0 loads correctly.
- `DELAY(n)` with large counts can end the script early.
- `ACTION(n)` restarts the calling script.
- The info bar picks the first thing in draw order, not the topmost one.

Some things are deliberately different:

- There is no shareware level limit and no start-up nag screen.
- Savegames use their own file format.
- The screen is redrawn completely on every pass instead of with dirty rectangles.
- FM music comes from an emulator.
- Fades run at a fixed speed. The original's fade speed depended on the CPU.

The full reverse-engineering notes are in [`docs/reverse-engineering/`](docs/reverse-engineering/).

## License

This code is released under the [MIT License](LICENSE). It does not cover the original EDPACK
programs or the games made with them, such as the demo, which belong to their authors.
