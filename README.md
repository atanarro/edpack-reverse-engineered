# EDPACK in Rust

A modern re-implementation of **EDPACK** ("Editor de Aventuras Gráficas", José Luis Serrano
Moreno, 1990s), the Spanish DOS toolkit for point-and-click graphic adventures. It plays the
games made with the DOS versions natively, both EDPACK v1 (`RUNVGA.EXE`) and EDPACK v2.0x
(`RUNDAT.EXE`, shareware and registered runtimes), and it replaces the v2 DOS-only toolchain.

It is based on the shareware versions of the Editor de Aventuras Gráficas (EDPACK), which were
distributed for free on Spanish magazine cover CDs: v2.0 on *Micromanía* CD 35 (folder `ED`), and
v2.02 on *PCManía* issue 67 (May 1998), in the folder `sharewar\varios\edpack20`
([ISO image](https://archive.org/download/PCMCDS/PCMania%20CD67_1.iso)). The EDPACK v1 demo is on
*Micromanía* CD 21 (folder `EDPACK`).

| Original (DOS) | Replacement | What it does |
|---|---|---|
| `RUNDAT.EXE` | `edpack` | Plays a packed `EDGAME.DAT` |
| `RUNFILES.EXE` | `edpack <folder>` | Plays compiled `NIVEL.*` files (or `.ED` sources, compiled on the fly) |
| `ED.EXE` | `edc <src> <out>` | Compiles `.ED` sources to `NIVEL.*` |
| `ED2DAT.EXE` | `edc <src> <out> --pack` | Packs a game into `EDGAME.DAT` |
| `SBFMDRV.COM`, `CT-VOICE.DRV` | `src/audio/` | OPL2 FM emulator for CMF music, VOC playback |
| `RUNVGA.EXE` (EDPACK v1) | `edpack <folder>` | Plays a v1 `RESOURCE.DAT` (or `<base>.nnn` files) |

Games known to work, each played to its ending by the test suite:

| Game | Made with | Interpreter |
|---|---|---|
| *Peter's Adventure I* (the demo) | EDPACK v2.0 shareware | `RUNDAT.EXE` v1.0 shareware |
| *Peter's Adventure I*, *PCManía* 67 build | EDPACK v2.02 shareware | `RUNDAT.EXE` v1.03 shareware |
| *El Dios de Nauru* (BYK Software, 2001) | EDPACK v2.03 | `RUNDAT.EXE` v1.0 registered |
| *Dadton Aguinández* (Coctel Software, 1998) | EDPACK v2.02 | `RUNDAT.EXE` v1.02 |
| *Las Aventuras de Manolo Kabezabolo* (Digital Legion, 1997) | EDPACK v1 | `RUNVGA.EXE` v1.03 |
| The EDPACK v1 demo (Omega Works, 1996) | EDPACK v1 | `RUNVGA.EXE` v1.0 |

The variant is detected from the data. See [`docs/history.md`](docs/history.md)
for the versions; the Windows rewrite EDPACK 2000 is a different program and is not supported.

The engine was built by reverse-engineering the original interpreter. The EDPACK package comes
with a demo game, *Peter's Adventure I*, both compiled (`EDGAME.DAT`) and as `.ED` sources. No
EDPACK files or games are included in this repository; get them from the CD image above.

## Building

You need Rust (stable). On Linux you also need the ALSA development headers for sound
(`sudo apt install libasound2-dev`). The window uses X11 (works under XWayland).

```sh
cargo build --release
```

To make `edpack` play a game when started without arguments, name its `EDGAME.DAT` at build
time:

```sh
EDPACK_DEMO_DAT=/path/to/EDGAME.DAT cargo build --release
```

## Playing

```sh
target/release/edpack game/EDGAME.DAT   # any EDPACK game
target/release/edpack game              # a folder: EDGAME.DAT, NIVEL.* files, .ED sources or a v1 RESOURCE.DAT
target/release/edpack                   # the game embedded at build time, if any
```

Options:

| Option | Meaning |
|---|---|
| `--scale N` | Window size: 320×240×N (4:3) |
| `--speed 0-4` | Game speed (default: the game's SETUP.CFG, else 2) |
| `--text-speed 0-4` | Text speed (default: the game's SETUP.CFG, else 3) |
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

EDPACK v1 games have no verbs. Left click on a thing does what that thing is set up for (look at it,
use it, go there); right click takes it. The inventory is the bar at the bottom: left click on an
object describes it, right click picks it up to use on a thing or on another object. Ctrl+Esc
quits, as in the original.

## Tools

```sh
target/release/edc src out --pack                    # ED.EXE + ED2DAT.EXE
target/release/edtool run EDGAME.DAT shots tests/walkthrough/peter.txt   # headless play + PNGs
target/release/edtool disasm EDGAME.DAT 3            # decompile level 3's scripts
target/release/edtool dump EDGAME.DAT                # summary of every level
target/release/edtool cmf2wav SATAIR.CMF satair.wav
cargo test --release                                 # unit and end-to-end tests
```

The end-to-end tests need the games, which are not part of this repository; keep them elsewhere
and point the tests at them with environment variables. For the demo, give its compiled
`EDGAME.DAT` and the folder with its sources:

```sh
EDPACK_DEMO_DAT=/path/to/DEMO/EDGAME.DAT EDPACK_DEMO_SRC=/path/to/FUENTES cargo test --release
```

They play the demo to the end three ways: from the original `EDGAME.DAT`, from the sources, and
from an `EDGAME.DAT` built by `edc --pack`. They check that all three runs end at the same
simulated moment, and that the compiler's output matches the original ED.EXE's.

The *PCManía* 67 build of the demo is tested with `EDPACK_DEMO103_DAT` (its `EDGAME.DAT`) and
`EDPACK_DEMO103_SRC` (its `FUENTES` folder, compiled and compared with the original ED.EXE v2.02).
Other games are played to their endings when given: `EDPACK_NAURU_DAT` and `EDPACK_DADTON_DAT`
name the `EDGAME.DAT` of *El Dios de Nauru* and *Dadton Aguinández*, and `EDPACK_MANOLO` the folder with the `RESOURCE.DAT` of *Las Aventuras
de Manolo Kabezabolo* (EDPACK v1), and `EDPACK_V1DEMO` the folder of the EDPACK v1 demo. Tests whose variables are not set are skipped.

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
- **Engine threading** (`host.rs`, shared by both engines). The original is a blocking DOS program: a main loop that
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
- **v2 runtime variants** (`level.rs`, `dat.rs`). The registered archives have 1000 level slots,
  the registered level files lack the shareware's filler bytes, and RUNDAT v1.02 has 12-byte
  animation records, a skip key (ASLT/DSLT), PUTXY/SEEANIM, a 200-operation limit and some UI changes. The
  layout is detected by parsing the levels each way. Details: `docs/v2-rundat/07-versions.md`.
- **EDPACK v1** (`v1/`). A separate engine for RUNVGA.EXE: its own data segment layout, the raw
  level tables of `RESOURCE.DAT`, the 41-opcode interpreter, the walker, the verbless click
  logic, the inventory bar, texts, the incremental screen updates and the SPACE menu. Details:
  `docs/v1-runvga/`.

Some quirks of the original are reproduced on purpose, because games were tested against them:

- `SEEINV` always returns 0.
- `NEWMOUSE` uses a 60-byte stride, so only cursor set 0 loads correctly.
- `DELAY(n)` with large counts can end the script early.
- `ACTION(n)` restarts the calling script.
- The info bar picks the first thing in draw order, not the topmost one.

Some things are deliberately different:

- There is no shareware level limit. Shareware archives (6 level slots) do show the shareware
  runtime's start-up notice: 6 seconds, or until a key is pressed.
- Savegames use their own file format.
- The screen is redrawn completely on every pass instead of with dirty rectangles.
- FM music comes from an emulator.
- Fades run at a fixed speed. The original's fade speed depended on the CPU.

The documentation is in [`docs/`](docs/README.md): the EDPACK versions and the games known to use
them ([`docs/history.md`](docs/history.md)), and the reverse-engineering specs of both interpreters
([`docs/v1-runvga/`](docs/v1-runvga/), [`docs/v2-rundat/`](docs/v2-rundat/)).

## License

This code is released under the [MIT License](LICENSE). It does not cover the original EDPACK
programs or the games made with them, such as the demo, which belong to their authors.
