# Documentation

| Path | What it is |
|---|---|
| [`history.md`](history.md) | The EDPACK versions (v1, v2.0x, EDPACK 2000), their interpreters, and the games known to use them. Start here. |
| [`v1-runvga/`](v1-runvga/) | Reverse-engineering spec of **RUNVGA.EXE v1.03**, the EDPACK v1 interpreter. Implemented in `src/v1/`. |
| [`v2-rundat/`](v2-rundat/) | Reverse-engineering spec of **RUNDAT.EXE**, the EDPACK v2.0x interpreter (shareware v1.0 build). Implemented in `src/engine.rs` and the modules around it. |

## EDPACK v1 — `v1-runvga/`

| Document | Covers |
|---|---|
| [`01-formats.md`](v1-runvga/01-formats.md) | RESOURCE.DAT, level layout, records, string keys, PCX, font, inventory, start-up, the variable table, savegames, fatal errors |
| [`02-interpreter.md`](v1-runvga/02-interpreter.md) | The bytecode interpreter: calling convention, all 41 opcodes, helpers, who runs scripts |
| [`03-mainloop-ui.md`](v1-runvga/03-mainloop-ui.md) | Timing, main loop, input, click actions, label line, texts, composition, level change, SPACE menu, save/load |
| [`04-player-render.md`](v1-runvga/04-player-render.md) | GOXY and the walker, player animation data, the thing record, buffers and incremental drawing, hover, palette ops, cursor |
| `img/` | Reference screenshots of the original's UI |

## EDPACK v2.0x — `v2-rundat/`

| Document | Covers |
|---|---|
| [`00-overview.md`](v2-rundat/00-overview.md) | First map of the interpreter: function addresses, EDGAME.DAT, level block order, THING record, variables, opcode list |
| [`01-main-loop.md`](v2-rundat/01-main-loop.md) | Main loop, scheduling, input dispatch, scene composition |
| [`02-levels-transitions.md`](v2-rundat/02-levels-transitions.md) | GOLEVEL, DEFSCR, the level loader, transitions |
| [`03-player-movement.md`](v2-rundat/03-player-movement.md) | Path planning (LIMIT/LIMITMOV), walking, frames, scaling, shading |
| [`04-text-ui-inventory-menu.md`](v2-rundat/04-text-ui-inventory-menu.md) | Fonts, speech, info bar, STEXT, inventory (appendix A), menu and savegames (appendix B) |
| [`05-palettes-pcx.md`](v2-rundat/05-palettes-pcx.md) | Palettes, PCX loading, fades, SHOWPCX/LOADPCX/LOADRGB |
| [`06-timing-media.md`](v2-rundat/06-timing-media.md) | Timer, input, FLI, CMF/VOC sound, EXIT, the shareware nag |
| [`07-versions.md`](v2-rundat/07-versions.md) | Differences between the shareware v1.0, registered v1.0 and v1.02 runtimes, and how the data tells them apart |

## Conventions

- Code addresses are offsets in the unpacked executable's single near code segment; data
  addresses (`DS:xxxx`, or bare hex in tables) are offsets in its data segment. The v2 documents
  use the shareware demo build's addresses (the ones `src/memory.rs` models);
  `v2-rundat/07-versions.md` maps them to the other builds.
- The specs were written by disassembling the unpacked programs and checking the results against
  the originals running in an emulator (unicorn, with a minimal DOS/BIOS/VGA/mouse layer). The
  analysis workspace (called `SCRATCH` in some notes) is not part of this repository.
- Where a spec and the code disagree, the code (and its tests, which play the games to their
  endings) is the reference; uncertain points are marked as such in each document.
