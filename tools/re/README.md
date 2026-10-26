# Checking the port against the original, one function at a time

Playing the games to their endings shows the engine works for the paths those games
take. It says nothing about edge cases they never hit. These tools check single functions
of the original interpreter against their Rust ports instead:

1. **Read the function.** `fn.py` prints one function of the unpacked interpreter, so a
   port can be reviewed against its disassembly with nothing else in view.
2. **Test it differentially.** `oracle.py` runs that function alone in an emulator
   (unicorn) on fuzzed data-segment contents and records test vectors. `tests/oracle.rs`
   replays each vector on the Rust port, which must change exactly the same bytes of the
   emulated data segment (`src/memory.rs`) to the same values.
3. **Keep a ledger.** `ledger.py` writes [`docs/validation.md`](../../docs/validation.md):
   every function of the original, whether the Rust code ports it, and whether it is
   checked.

## Setup

Needs Python 3 and the EDPACK zips in `games/` (see the main README). The executables are
extracted to `tools/re/.cache/`, which, like the venv and the generated vectors, is not
committed.

```sh
python3 -m venv .venv-re
.venv-re/bin/pip install capstone unicorn
```

## Use

```sh
# read a function (addresses are offsets in the code segment, as in docs/)
.venv-re/bin/python tools/re/fn.py rundat 5606

# generate vectors; --args gives the range of each stack argument
.venv-re/bin/python tools/re/oracle.py rundat 685f
.venv-re/bin/python tools/re/oracle.py rundat 7fdc --args=-20:340,-20:220 --rounds 3000

# start from hand-made cases when random inputs can't reach a branch (see below)
.venv-re/bin/python tools/re/oracle.py rundat 7fdc --args=-20:340,-20:220 --seeds tools/re/seeds/rundat_7fdc.json

# replay them on the Rust code
cargo test --test oracle

# refresh docs/validation.md
.venv-re/bin/python tools/re/ledger.py
```

To check another function, generate its vectors, add a `check("rundat_<addr>", ...)` test
calling the port, and run the ledger again.

## How the oracle works

- `dosemu.py` loads the EXE and runs it until Borland's start-up asks DOS for its version
  (`int 21h/30h`). By then RUNDAT.EXE's packer has decompressed the program, so the image
  matches the addresses in `docs/` (code at `CS:0`, `DS` = DGROUP).
- To call a function, the oracle restores that image, writes the inputs into DS, pushes the
  arguments and a return address that stops the emulator, and runs it. A call to the
  fatal-error routine (`0xa836`) stops it too and is recorded.
- Every DS byte the function reads before writing it is an input. Fuzzing starts from the
  original's DS, mutates those inputs with edge-biased values, and keeps the mutations that
  reach new instructions. The header of each `.vec` file gives the share of the function's
  instructions the vectors execute; the oracle lists the ones never reached.
- Random mutation rarely sets up branches that need several fields to agree (a whole
  LIMITMOV record, say). When the oracle lists instructions it never executed, read them
  with `fn.py`, write a case that reaches them into `seeds/<prog>_<addr>.json`, and pass it
  with `--seeds`. Aim for full coverage: GOXY's third planning pass was wrong in the port,
  and only a hand-made case reached it.
- A vector holds only the bytes the original read or wrote. The Rust test loads them into
  a zeroed `Memory`, so if the port reads anything the original did not, it reads 0.

## The compiler against ED.EXE (DOSBox)

`ed_dosbox.py` runs the original ED.EXE and ED2DAT.EXE in DOSBox on a kit's sources:

```sh
# every .ED in its own fresh ED.EXE run, then ED2DAT (LETRAS1.FNT as NIVEL.FNT)
python3 tools/re/ed_dosbox.py <kit with ED.EXE, ED.DAT, ED2DAT.EXE> <FUENTES> <out>
# the ROOMn.ED files in one ED.EXE run, in this order
python3 tools/re/ed_dosbox.py <kit> <FUENTES> <out> --chain 4,2,0,1,3,5
```

`edc` reproduces every file byte for byte, for ED.EXE v2.0 and, with `--v202`, v2.02;
`tests/game.rs` holds the lengths and hashes of the original's output.

ED.EXE compiles every file a wildcard matches in one run and only partly resets its memory
between files: the text pool and the bytecode are cleared but filled on from where the previous
file stopped, and most tables are only marked unused, so their records keep the previous file's
data (`LevelState::next_file` in `src/compiler/compile.rs`). The shipped demos were built that way:
rooms 4, 2, 0, 1, 3, 5 in one run for the v2.0 demo, and 4, 2, 0, 1, 3 plus room 5 on its own for
the v2.02 one. `edc --chain` reproduces such a run, and both shipped `EDGAME.DAT` files are rebuilt
from their sources byte for byte.

## Limits

- Only state in the data segment is compared. Far buffers (screen, fonts, level code) are
  reported as "touched outside DS" and are not captured yet, so drawing functions and the
  bytecode interpreter need more work before they can be checked this way.
- Calls that don't return within 2 million instructions are dropped, so where the original
  hangs (GOXY with a cycle of LIMITMOV records) and the port deliberately stops, nothing is
  compared.
- Vectors for a function include its callees: the port is checked as a whole.
- The v1 code (`src/v1/`) cites RUNVGA v1.03 addresses. `games/` only has the v1.0 demo
  runtime, whose code is laid out differently, so v1 is not checked yet.
