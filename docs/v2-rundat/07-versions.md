# RUNDAT.EXE versions: shareware v1.0, registered v1.0, v1.02 — spec of the differences

The Rust engine reproduces the **shareware RUNDAT.EXE v1.0** shipped with the demo ("demo" below; all addresses
in the other documents and in `src/` are its DS offsets). Two other builds of the EDPACK v2 interpreter exist
in other games:

| build | game | version string | opcodes | notes |
|---|---|---|---|---|
| shareware v1.0 | demo (*Peter's Adventure I*) | `RUNDAT.EXE v1.0` + `Version SHAREWARE` | 1..66 | the reference |
| registered v1.0 | *El Dios de Nauru* (`RUNDAT.EXE`, 44173 bytes) | `RUNDAT.EXE v1.0` | 1..66 | shareware limits removed, new archive layout |
| v1.02 | *Dadton Aguinández* (`Dadton.exe` = renamed RUNDAT, 44685 bytes) | `RUNDAT.EXE v1.02` | 1..69 | + 4 opcodes, 8 variables, 12-byte animations, inventory/cursor changes |

Method: the three unpacked images were aligned instruction by instruction (normalised mnemonics, `bindiff.py`),
which gives a code map and a DS map between builds; every unaligned block and every aligned instruction whose
constants differ was then read (`dlt_*.py` in the scratch folder). Everything below is verified by disassembly
unless marked **(?)**. Dadton was also run in the unicorn reference emulator up to level 1 to check the new
animation layout, the new variables' initial values and the 0xfabe counter.

All three keep the demo's code otherwise unchanged (same functions, same order; the many other "differences" in
a raw diff are relocated addresses, register allocation, longer jumps and misdisassembled data tables).

---------------------------------------------------------------------------------------------------
## 1. Telling the builds apart from the game data

The level header is `02 00 00 00` + 26 zeros in all three games, so the header does not say which interpreter
was used. The data does:

1. **Archive table size** (first u32 / 4):
   * 12 entries → shareware layout (levels 0..5, specials at 6..11) → **shareware** level files (with the
     single filler bytes).
   * 1006 entries → registered layout (levels 0..999, specials at 1000..1005) → **registered** level files
     (no filler bytes). This holds for both registered v1.0 and v1.02.
2. **v1.0 vs v1.02** (registered only): parse the levels with a 0x258-byte ANIMS block. For v1.02 data this
   works for every level and **all 50 animation records have `skip == -1`** (word +0x0a; ED.EXE v1.02 writes
   -1 there). Parsing v1.0 data this way fails (the stream runs past its end) — checked on all 66 Nauru levels.
   Do not rely on "the trailing offset tables look sane" alone: parsing Dadton with the 0x1f4 size also ends
   at the right place, because the RLE run that covers the ANIMS block is simply truncated.
   Corroborating signal: the NIVEL.MOU entry holds 72 bytes per cursor set in v1.0 data (6 cursors) and 120
   bytes per set in v1.02 data (10 cursors): `(next_entry_offset - mou_offset - 32) / count` = 72 / 120.

Loose files (RUNFILES-style NIVEL.nnn) of registered games have not been seen; presumably the same two layouts.

---------------------------------------------------------------------------------------------------
## 2. Archive and level file layout

### 2.1 EDGAME.DAT table
Registered builds seek the special files at fixed entries **1000+k** (main() / loaders push 4000, 4004, ...):

| file | shareware entry | registered entry |
|---|---|---|
| levels | 0..5 | 0..999 (unused = 0) |
| NIVEL.INV | 6 | 1000 |
| NIVEL.FNT | 7 | 1001 |
| NIVEL.MOV | 8 | 1002 |
| NIVEL.MOU | 9 | 1003 |
| NIVEL.IAC | 10 | 1004 |
| NIVEL.TIT | 11 | 1005 |

Level n is still at `fseek(4n)`; the level number is a **u16** in both registered builds (byte in the demo).

### 2.2 Level files
Same RLE block sequence as the demo loader 0x24ec (see [`00-overview.md`](00-overview.md)), with two changes:

* **registered (v1.0 and v1.02): none of the single-byte skips** (`fgetc` after gfx names, after LIMITMOV,
  after THING_ORDER, after the text pool, after ANIMS, after SELECTEXT) — the six skip calls are gone from the
  loader.
* **v1.02: ANIMS is 0x258 bytes** (50 x 12) instead of 0x1f4 (50 x 10). See §4.1.

So the registered sequence is: gfx 0x34, files 0x104, LIMITMOV 0x1b80, LIMIT 0x230, THINGS 0xac8, order 0x1e,
DOORS 0xc8, text pool 0x61a8, text offsets 0x1f4, code 0x2710, action offsets 0x64, ANIMS 0x1f4|0x258,
ACINVTH 0xf0, COORANIM 0x3e8, SELECTEXT 0x258, 5 x message 0xfa, SCALEMOV 0x168, SHADOWMOV 0xf0,
DEFFRAMES 0xa28, then the raw FILE table 0xa0 and gfx table 0x20. Everything else in the loader (decryption,
header check, message copy rule) is unchanged.

### 2.3 NIVEL.MOU (v1.02)
NEWMOUSE (demo 0x2aae) still seeks `off + 32 + n*60` but reads **0x78 bytes (10 cursors)** instead of 0x48
(6). The cursor table grows in place (demo 0x1fc4..0x2003 → v1.02 0x1e88..0x1eff, 10 x {x,y,w,h,hotx,hoty});
the player frame table that follows it moved accordingly, so v1.02 has no over-read into the frame table.
With Dadton's data (one set, 120 bytes): cursors 0..4 = verbs, cursor 5 = menu/inventory-arrow cursor,
6..9 = zero.

---------------------------------------------------------------------------------------------------
## 3. Registered v1.0 (Nauru) vs shareware v1.0

Behavioural differences, complete list:

1. Archive layout and level-file filler bytes (§2).
2. **No level limit**: all `if level > 5 / >= 6: fatal(200)` checks are removed (main loop, GOLEVEL,
   change_level, the text and inventory functions, MENU — 16 places in the demo).
3. **Level number is u16**: GOLEVEL stores `W[level] = a0` (demo: byte), and the loaded-level copy, the loader
   (`fseek(level*4)`), the main-loop comparison and MENU use words. Scripts cannot see it (not a variable).
4. **No nag screen** at start (main() 0xbb0e..: the four "version SHAREWARE" lines, the 6 s wait) and no
   shareware lines in the exit/fatal printout.
5. Savegame block 2 ("globals") is 0x174 bytes instead of 0x172 (the wider level number); file header bytes
   are unchanged (`00 FF 02 03`), so original savegames of different builds are not distinguishable but also
   not compatible. Irrelevant for the Rust engine, which uses its own format.

Everything else is identical, including the 51-op limit, all opcode semantics, RAND, the UI and the 16-bit
NIVEL.INV record offset (see §6).

---------------------------------------------------------------------------------------------------
## 4. v1.02 (Dadton) vs registered v1.0

### 4.1 Animation records: 12 bytes
```
struct ANIM (v1.02) { i16 active /*+0*/; i16 base /*+2*/; i16 pc /*+4*/; i16 cmp /*+6*/; i16 unused /*+8*/;
                      i16 skip /*+0x0a: skip target (word offset in the script), -1 = none*/ };
```
Table: DS 0x1c30 in v1.02 (demo layout: 0x1dd0, 50 x 10). Every user of the table uses the 12-byte stride
(ANIM_ON/ANIM_OFF, the runner, op 67/69, LOAD/SAVE). The compiler writes `skip = -1` in every record.
Verified in the emulator after Dadton level 1 loaded: records `(1,249,0,0,0,-1)`, `(1,341,0,0,0,-1)`, ...

### 4.2 The interpreter (demo 0x2ca2)
* **5th argument**: `INTERP(base, &id, &pc, &cmp, i8 anim)` — `anim` is the animation index when called from
  the animation runner, **-1** from everywhere else (ACTION 0 at level entry 0x1eb1, the second synchronous
  call 0x23ce, run_action 0x7d14).
* **Runaway limit 200** (`> 0xc8` ops → fatal 0x34; the message still says 50). Demo/v1.0: 51.
* 69 opcodes: the jump table has 69 entries; **op 18** (invalid in v1.0) and **ops 67, 68, 69** are new.
  47..51 and 59 remain invalid (fatal 0x6f).
* All other opcode handlers are unchanged except op 54 RAND (below) and the 12-byte stride in ANIM_ON/OFF.

New / changed opcodes (pc in words; "continue" = no yield, next op in the same call):

| op | proposed name | args | semantics |
|---|---|---|---|
| 18 | `SKIPTO(label)` | 1, **raw** (re-read from the code, not dereferenced) | `B[KEY 0xdc] = 0; if anim != -1: ANIM[anim].skip = label else W[SKIP_TARGET] = label; pc += 2; continue` |
| 54 | `RAND(var, n)` | 2 | `var = trunc(f32(c * f32(n / 100.0f)))` with `c = B[RAND_COUNTER]` (0..100, §4.4); pc += 3; continue. (v1.0: `var = (n % 100) * hundredths_of_DOS_time`, see §6) |
| 67 | `NOSKIP` **(?)** | 0 | `ANIM[(i8)anim + 1].active = -1` (word 0xffff); pc += 1; continue — see note |
| 68 | `PXY(x, y)` | 2 (deref'd) | `if x != -100: W[fc01] = x - W[fc0d]/2` (player left = x − width/2, signed division); `if y != -100: W[fc03] = y - W[fc0f]` (top = y − height); pc += 3; continue. Places the player's feet at (x, y) without walking; does not touch the walk state or fd3c/fd3e |
| 69 | `ISANIM(n)` | 1 (deref'd) | `var48 = (ANIM[n].active == 1) ? 1 : 0`; pc += 2; continue |

Op 67 note: the code is `mov al,[bp+12h]; cbw; imul ax,12; mov word [bx+1c3ch], -1`, i.e. offset +0x0c of
record `anim` = `active` of record `anim+1` (and of record 0 when called from an action, `anim = -1`). Setting
`active = -1` stops that animation (the runner only runs `active == 1`). The probable intent was "cancel the
skip target of this script" (+0x0a), but the faithful behaviour is the one above. None of the four new opcodes
(nor variables 47..54) is used by Dadton's scripts, so none of this is exercised by that game.

### 4.3 Skip targets (SKIPTO): run_action (demo 0x7937) and the animation runner (demo 0x7f6f)
v1.02 adds a "skip the rest of this cut-scene" mechanism driven by a key (variable 47, default 1 = Esc scan
code). B[0xdc] is the last scan code delivered by the keyboard ISR (unchanged), the same byte the main loop
tests for SPACE.

run_action, default branch (real scripts, `fcc9` not in 100..106):
```
if fcc9 == 0: return
if W[SKIP_TARGET] != -1:                               // set by SKIPTO in an action (or ACTION 0)
    if B[0xdc] == W[var47]:  fcca = (u8)W[SKIP_TARGET]; B[0xdc] = 0
    if (i16)(u16)fcca >= W[SKIP_TARGET]:               // reached or jumped past the label
        W[SKIP_TARGET] = -1
        clear_texts()                                  // demo 0x788d: 5 text slots off, fa2e = 0
        fc24 = fc26 = -100                             // drop the pending walk target
        fcce = 0                                       // stop walking
if fca9 != 0 || fcce != 0: return                      // unchanged condition, now evaluated after the skip
r = INTERP(W[6926 + 2*fcc9], &fcc9, &fcca, &fccb, -1)
if r == 0: fcc9 = 0; fcca = 0
```
* SKIP_TARGET is **global**: it is not cleared when an action ends or another starts, so a stale target keeps
  being compared with whatever action runs next until some pc reaches it or Esc is pressed. Reproduce as is.
* After a jump the text is only marked to end (fca9 stays set until text_frame runs later in the same pass),
  so the script resumes on the next pass.

Animation runner:
```
if B[fcd0] > 0: return
for i in 0..49:
    if A[i].active == 1:
        r = INTERP(A[i].base, &dummy, &A[i].pc, &A[i].cmp, i)
        if r == 0: A[i].pc = 0; A[i].active = 0
    if A[i].skip != -1:                                // checked for every record, active or not
        if B[0xdc] == W[var47]: A[i].pc = A[i].skip; B[0xdc] = 0
        if A[i].pc >= A[i].skip: A[i].skip = -1; clear_texts()
B[fcd0] = B[8a23]
```
(No walk reset here.) Note a finished animation has pc 0, so its pending skip stays until Esc or a later run.

### 4.4 Random counter B[RAND_COUNTER] (v1.02 0xfabe)
* hw_init (demo 0xa5d5) seeds it with the DOS time's hundredths (`int 21h/2Ch`, DL).
* The timer ISR (demo 0x3dc5, 50 Hz), right after chaining the old int 8 and before anything else:
  `if ++B[RAND_COUNTER] > 100: B[RAND_COUNTER] = 0` — so it cycles 0..100 every 101 ticks (verified: +83 per
  83 ticks, mod 101).
* Only RAND reads it. Saved in savegames (block 2 starts at it).

### 4.5 Variables: the pointer table (demo DS:491e) in v1.02
Variables 0..46 point at the same globals as in the demo (relocated), **except 28..32, which become separate
16-bit words** (demo: overlapping byte flags fcfe..fd04, see §4.6). 47..54 are new. Thing variables and
1000..1999 are unchanged.

| var | demo address | v1.02 address | v1.02 meaning | initial |
|---|---|---|---|---|
| 28 | fd04 (byte) | fc0e (word) | MENU key enabled | 1 |
| 29 | fd01 (byte) | fc08 (word) | info bar enabled | 1 |
| 30 | fcff (byte) | fc04 (word) | hover / labels enabled | 1 |
| 31 | fcfe (byte) | fc02 (word) | inventory opening enabled | 1 |
| 32 | fd03 (byte) | fc0c (word) | right-click verb change enabled | 1 |
| 47 | — | face | scan code of the skip key (§4.3) | 1 (Esc) |
| 48 | — | faca | result of ISANIM (op 69) | 0 |
| 49 | (= fcce) | fbd2 | player walking flag (the demo's fcce, now visible) | — |
| 50 | — | fabf | number of verbs cycled by right-click (§4.7) | 5 |
| 51 | — | fac3 | inventory "look" verb (demo hard-codes 1) | 1 |
| 52 | — | fac5 | inventory "use / pick up" verb (demo hard-codes 3) | 3 |
| 53 | — | fac8 | cursor shown by MENU (demo MOUSEACTION(101) → 5) | 5 |
| 54 | — | fac1 | cursor shown over the inventory scroll arrows | 5 |

Not visible to scripts: SKIP_TARGET (facc, word, init -1), RAND_COUNTER (fabe, byte), INV_VERB (fac7, byte,
init 0xff: the verb last used inside the inventory panel).

### 4.6 Byte flags → words (fcfe..fd04)
In v1.02 all seven bytes fcfe..fd04 (var31, var30, the "previous var30" copy fd00, var29, the "previous var29"
copy fd02, var32, var28) are **independent words**, and every test is a word compare `== 1`
(`cmp word [x], 1`), including the previous-value comparisons of the info bar. Consequences for scripts:
* the demo quirk "writing var31 also clears var30" (and similar overlaps among 28..32) does not exist;
* a value like 257 counts as "off" (demo: low byte 1 → "on").
The places that test them are the same as in the demo: main loop (var28 SPACE → MENU), input() (var31 inventory,
var30 hover, var32 right-click), build_label/info bar (var29, fd02), inventory_frame (var29 info bar,
var32 right-click).

### 4.7 MOUSEACTION (demo 0x53ec)
```
v1.02:  if n == 100: fd2c++; if fd2c >= W[var50]: fd2c = 0
        else:        fd2c = n                       // no clamping at all
demo:   if n == 100: fd2c++ else fd2c = n
        if fd2c > 4 && fd2c != 101: fd2c = 0;  if fd2c == 101: fd2c = 5
```
(rest unchanged: cursor k from the cursor table, hotspot kept, held object dropped, label rebuilt, ranges).
MENU (demo 0x9ae1) calls `MOUSEACTION(W[var53])` where the demo calls `MOUSEACTION(101)`.

### 4.8 input() (demo 0x500d)
1. **Opening the inventory** (mouse at the top): unchanged conditions (var31 now a word), then
   ```
   fd2b = 1; fd2e = fd2c
   if INV_VERB != W[var51] && INV_VERB != W[var52]: INV_VERB = (u8)W[var51]
   MOUSEACTION(INV_VERB)                     // demo: if fd2c != 1 && fd2c != 3: MOUSEACTION(1)
   ```
   i.e. the panel always opens with the verb last used inside it (initially var51).
2. hover() only if `W[var30] == 1`; right-click → MOUSEACTION(100) only if `W[var32] == 1`.
3. **Dropping a held object** moved and changed. Demo, before the floor GOXY:
   `if 8a8c && fca9 == 0 && fca7 == 0 && fc22 == 0: 8a8c = 0` (and then the same click walks).
   v1.02, after the floor-GOXY test:
   ```
   if B[8a8c] != 0 && fca9 == 0 && fca7 == 0 && fc22 == 0:
       MOUSEACTION(fd2e)        // back to the verb that was active before the inventory was opened
       B[8a8c] = 0
   ```
   So a click on empty floor while holding an object only drops it (the GOXY test still sees `8a8c == 1`, so
   the player does not walk), and the cursor returns to the pre-inventory verb.

### 4.9 Inventory panel (inventory_frame, demo 0xb4e3)
Every hard-coded verb becomes a variable:
* click on a slot with `fd2c == W[var51]` → SPEAK(999 + slot) (demo: `fd2c == 1`);
* click with `fd2c == W[var52]` → pick up / combine (demo: `fd2c == 3`);
* right-click (if `W[var32] == 1`): `MOUSEACTION(fd2c == W[var51] ? W[var52] : W[var51])`, then
  `INV_VERB = fd2c` (demo: toggles 1 ↔ 3);
* the info bar is drawn if `W[var29] == 1`.
New arrow-cursor logic, every frame after the slot hover test (hx = mouse x + hotspot x = fd4a + fd5e):
```
if hx <= 13 && fccc > 0 && fd2c != W[var54]:                                   MOUSEACTION(W[var54])
if hx >= 307 && inv[fccc+8].used && fccc+8 < 30 && fd2c != W[var54]:          MOUSEACTION(W[var54])
if fd2c == W[var54] && 13 < hx < 307:                                          MOUSEACTION(INV_VERB)
```
(the left/right scroll zones and conditions are the demo's own click tests for scrolling.)
Closing the panel (mouse y > 30) still restores `fd2e` unless an object is held; after a "look" click the panel
closes with MOUSEACTION(fd2e) as in the demo.

### 4.10 Object use / combination
* acinvth_dispatch (demo 0x7e0b): `MOUSEACTION(fd2e)` is called for every ACINVTH entry with `mode != 0`
  that is examined (before its match test) and once more if nothing matched — in practice: the cursor always
  returns to the pre-inventory verb. Matching and states 102/104 are unchanged.
* acinvinv (demo 0x7d29): `MOUSEACTION(fd2e)` for every `used == 1` IAC entry examined (before its match
  test); not on the "no entry used" path. Rest unchanged.

### 4.11 ADDINV: NIVEL.INV record offset
v1.02 computes the record position with a 32-bit multiply: `entry + 0x20 + 0x300 + obj * 0x4aa`. Demo and
registered v1.0: `entry + 0x20 + sext16((obj * 0x4aa + 0x300) & 0xffff)`, wrong from object 27 on (see §6).
(v1.02 also copies the decrypted object name into an unused local buffer — no effect.)

### 4.12 Transition mode 5
`clear(0,0,320, W[var23], VGA, maxy)` uses `maxy = 199` instead of `var23`. No visible difference (the rows
cleared are < var23 either way).

### 4.13 Savegames (v1.02)
Same header (`00 FF 02 03` + title check) and block order as the demo ([`04-text-ui-inventory-menu.md`](04-text-ui-inventory-menu.md) §5), with:
block 2 = 0x18d bytes from RAND_COUNTER (covers 0xfabe..0xfc4a: the new variables and all globals), block 16
ANIMS 0x258, block 31 cursor table 0x78. Not compatible with v1.0 savegames, and there is no version check
that would detect it. The Rust engine's own save format must add: the 12-byte animation records, variables
47..54, SKIP_TARGET, RAND_COUNTER, INV_VERB and the 10-cursor table.

---------------------------------------------------------------------------------------------------
## 5. DS address map (selected)

Demo address → same object in the other builds, taken from the loaders, the savegame block lists and the
aligned code (exact). Useful to read the original code; the Rust engine keeps the demo layout.

| object | demo | Nauru | Dadton |
|---|---|---|---|
| gfx names / FILE names | 43fa / 442e | 424a / 427e | 42ea / 431e |
| LIMITMOV / LIMIT | 6c72 / 87f2 | 6ac2 / 8642 | 6b62 / 86e2 |
| THINGS / order | 8b67 / 8b49 | 89b7 / 8999 | 8a57 / 8a39 |
| DOORS | 685e | 66ae | 674e |
| text pool / offsets | 9823 / 962f | 9673 / 947f | 9713 / 951f |
| action offsets | 6926 | 6776 | 6816 |
| ANIMS | 1dd0 (50 x 10) | 1c20 (50 x 10) | 1c30 (50 x 12) |
| ACINVTH / COORANIM / SELECTEXT | 6b82 / 4532 / 40dc | 69d2 / 4382 / 3f2c | 6a72 / 4422 / 3fcc |
| SCALEMOV / SHADOWMOV / DEFFRAMES | 389a / 37aa / 1194 | 36ea / 35fa / 0fe4 | 378e / 369e / 0ff4 |
| FILE / gfx offset tables | 435a / 433a | 41aa / 418a | 424a / 422a |
| cursor table (NEWMOUSE) | 1fc4 (0x48) | 1e14 (0x48) | 1e88 (0x78) |
| player frame table | 200c | 1e5c | 1f00 |
| inventory | 8a95 | 88e5 | 8985 |
| variable pointer table | 491e | 476e | 480e |
| palettes orig / cur / MOVS | 2280 / 2580 / 3480 | 20d0 / 23d0 / 32d0 | 2174 / 2474 / 3374 |
| level / loaded level | fc9c / fc9d (u8) | faec / faee (u16) | fb9e / fba0 (u16) |
| next door / mode | fcd1 / fcd2 | fb23 / fb24 | fbd5 / fbd6 |
| fcc9 / fcca / fccb (action, pc, cmp) | fcc9.. | fb1b.. | fbcd.. |
| fcce walking / fcd0 anim wait | fcce / fcd0 | fb20 / fb22 | fbd2 / fbd4 |
| fca9 text on (var7) | fca9 | fafb | fbad |
| fd2b inv open / fd2c verb / fd2e verb before inv | fd2b.. | fb7d.. | fc36 / fc37 / fc39 |
| fd40 MMOUSE / fd46 cursor state | fd40 / fd46 | fb92 / fb98 | fc4b / fc51 |
| fd4a / fd4c mouse | fd4a / fd4c | fb9c / fb9e | fc55 / fc57 |
| fc22 hover / fca7 inv hover / fccc inv scroll | fc22 / fca7 / fccc | fa72 / faf9 / fb1e | fb24 / fbab / fbd0 |
| 8a8c held / 8a90 pending combine | 8a8c / 8a90 | 88dc / 88e0 | 897c / 8980 |
| fcfe..fd04 (var31,30,–,29,–,32,28) | bytes fcfe..fd04 | bytes fb50..fb56 | words fc02, fc04, fc06, fc08, fc0a, fc0c, fc0e |
| fd07 / fd09 DEFSCR | fd07 / fd09 | fb59 / fb5b | fc12 / fc14 |
| fd22 var23 | fd22 | fb74 | fc2d |
| savegame block 2 | fbce, 0x172 | fa1e, 0x174 | fabe, 0x18d |
| new in v1.02 | — | — | fabe RAND_COUNTER, fabf var50, fac1 var54, fac3 var51, fac5 var52, fac7 INV_VERB, fac8 var53, faca var48, facc SKIP_TARGET, face var47 |

Key code addresses (demo → Nauru → Dadton): interpreter 0x2ca2 → 0x2c32 → 0x2c35; jump table 0x3c11 →
0x3b8d → 0x3c59; level loader 0x24ec → 0x24c3 → 0x24c6; main loop 0x5687 → 0x5603 → 0x56ec; input 0x500d →
0x4f89 → 0x506b; MOUSEACTION 0x53ec → 0x5368 → 0x5460; run_action 0x7937 → 0x77fb → 0x7906; animations
0x7f6f → 0x7e33 → 0x7fb9; acinvinv 0x7d29 → 0x7bed → 0x7d59; acinvth 0x7e0b → 0x7ccf → 0x7e43;
inventory_frame 0xb4e3 → 0xb38c → 0xb599; MENU 0x9ae1 → 0x99a5 → 0x9b99; main 0xb78c → 0xb635 → 0xb8d0;
hw_init 0xa5d5 → 0xa499 → 0xa6a4; ISR 0x3dc5 → 0x3d41 → 0x3e13; savegame blocks 0x9693 → 0x9557 → 0x974b.

---------------------------------------------------------------------------------------------------
## 6. Implementation notes for the Rust engine (proposals)

* **Variant flag** in the engine (`Shareware | Registered | V102`), chosen as in §1. Shareware and Registered
  behave the same at run time (the engine never enforced the shareware limits); only the loader differs
  (filler bytes, archive entries 1000+k).
* **Level number**: keep the byte at fc9c for the existing games (Nauru's highest level is 70, Dadton's 9);
  supporting GOLEVEL ≥ 256 would need the level and loaded-level kept as i16 outside the demo DS layout
  (fc9d is adjacent).
* **ANIMS (V102)**: the demo layout has no room to grow 0x1dd0 in place (DEFFRAMES / others follow); store the
  extra `skip` words in a separate 50 x i16 array (initialised from the +0x0a words of the 12-byte records), or
  move the whole table to a free area. `active/base/pc/cmp/unused` stay at the demo offsets.
* **New state (V102)**: put SKIP_TARGET, RAND_COUNTER, INV_VERB and variables 47..54 in an extension area after
  the user variables (grow `MEMORY_SIZE`), pointing var 47..54 there; var 49 should point at the walking flag
  fcce (a word read of fcce/fccf — in v1.02 the flag is still a byte at fbd2 followed by another byte, so the
  word read has the same overlap as reading fcce/fccf in the demo layout).
* **Vars 28..32 (V102)**: point them at five separate words in the extension area and make every engine test
  of fcfe/fcff/fd01/fd03/fd04 (and the fd00/fd02 "previous" copies) read the word when the variant is V102.
* **Cursor table (V102)**: 10 entries; in the demo layout entries 6..9 would overwrite the player frame table
  at 200c, so keep entries 6..9 elsewhere (or relocate the table) and make MOUSEACTION/NEWMOUSE use it.
* **Interpreter**: op limit 200 for V102; ops 18/67/68/69; RAND per variant; pass the animation index.

Side findings about the demo build (outside this document's scope, for whoever owns those parts):
* **RAND (op 54) in v1.0** is `var = (n % 100) * hundredths` where `hundredths` is DL of DOS `int 21h/2Ch`
  read at that moment (gettime 0xce98 into DS:3bf6, byte 3bf8), not a value in 0..n−1. The Rust engine
  currently uses `floor(random * (n % 100))`.
* **NIVEL.INV record offset in v1.0**: `entry + 0x20 + sext16((obj*0x4aa + 0x300) & 0xffff)` — from object 27
  on the offset becomes negative (reads before the entry). `specials.rs` models it as `(obj*0x4aa) & 0xffff`
  after the palette, which differs for objects ≥ 27 (no shipped game reaches it: demo 11 objects, Nauru 27,
  i.e. 0..26).

## 7. Uncertain
* Names of ops 18/67/68/69 and the intent of op 67 (the registered ED.EXE is not available).
* Whether registered ED.EXE versions also produce loose NIVEL.nnn files without filler bytes (not seen).
