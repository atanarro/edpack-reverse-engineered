# RUNVGA.EXE v1.03 (EDPACK v1): bytecode interpreter — spec

Addresses: code offsets are in the single near code segment of the unpacked RUNVGA (image loaded at
segment 0x1000 → CS 0x1000); `DS` is DGROUP (segment 0x1fec in the emulator, linear 0x1fec0). All DS
addresses below are offsets in DGROUP. Verified by disassembly (`runvga.asm`) and by tracing the original
in the unicorn reference emulator (`re/int_trace.py`, hook on the dispatch loop 0x246f); every level's
scripts in Manolo's RESOURCE.DAT decode cleanly with the op/argument table below (`re/int_dis.py`,
0 unknown opcodes in 15 levels).

## 1. Calling convention of `run(0x2464)`

```
u8 run(i16 base,            // [bp+4]  word offset of the script inside the level code
       u8 far *action,      // [bp+6]  only written on an unknown opcode (set to 0)
       u8 far *pc,          // [bp+0xa] program counter: u8 WORD offset relative to base
       u8 far *flag)        // [bp+0xe] compare flag (1 equal, 2 less, 3 greater)
returns al: 0 = script finished (opcode 0 at pc; pc is NOT reset), 1 = yield, 2 = unknown opcode
```

* Code: `i16 code[5000]` (10000 bytes per level) at the far pointer DS:0x2f10. Word at `code[base+pc]` is
  the opcode, `code[base+pc+1..]` the arguments. pc is a byte, so a script spans at most 256 words;
  jump targets are the low byte of the target word.
* Loop (0x246f): fetch op; `op == 0` → return 0. Otherwise **prefetch five arguments a0..a4**
  (`code[base+pc+1..+5]`, bp-2..bp-0xa) — each one that is `>= 25000` (signed compare, so negative
  literals stay literal) is dereferenced as variable `arg-25000` (16-bit word read through the far pointer
  `DS:0x0ea4 + 4*(arg-25000)`). The prefetch is done for every op regardless of its argument count.
  Then a linear `if (op == 1) … else if (op == 2) …` chain (no jump table).
* "Continue" ops write the new pc and jump back to 0x246f; "yield" ops write pc and return 1.
* **There is no runaway limit** (v2 aborts after 51 ops without a yield; v1 loops forever on a script
  that never yields).
* Ops that use *raw* argument words (not the prefetched values): VAR/VARVAR/ADD*/SUB* destination and
  source, CMP/CMPVAR, the jumps. For those, a variable is named by its plain number `n` (not 25000+n)
  and resolved through the same table: `*(i16 far *)ptr[n]` where `ptr[n]` is the far pointer at
  `DS:0x0ea4 + 4*n`.

## 2. Opcodes

`a0..a3` = prefetched (dereferenced) arguments; `r0, r1` = raw argument words; `V[n]` = variable n.
"next k" = pc += k. Names: the v2 name when the command is the same.

| op | name | args | semantics (v1) | pc / flow |
|---|---|---|---|---|
| 0 | END | 0 | return 0 (pc unchanged) | finish |
| 1 | GOLEVEL | 2 | `W[0x927c] = a0` (level, word); `B[0x92a3] = a1` (door) | next 3, **yield** |
| 2 | GOXY | 2 | `goxy(a0, a1)` (0x7dab: LIMIT redirection, LIMITMOV, starts the walk, sets walking flag B[0x92a0]=1; see movement spec) | next 3, **yield** |
| 3 | SPEAK | 1 | if `B[0x9282]` (text active) != 0: yield without advancing. Else `say(a0, -1, 0…)` (0x73f5, see below) and next 2 | **yield** always |
| 4 | PTEXT | 4 | if `B[0x9282]` != 0: yield without advancing. Else `ptext(a0=x, a1=y, a2=text, a3=colour)` (0x74c4) and next 5 | **yield** always |
| 5, 6 | — | | not implemented (→ unknown opcode). v1 has no ACTION(n). | |
| 7 | VAR | 2 | `V[r0] = r1` | next 3 |
| 8 | VARVAR | 2 | `V[r0] = V[r1]` | next 3 |
| 9 | ADD | 2 | `V[r0] += r1` | next 3 |
| 10 | ADDVAR | 2 | `V[r0] += V[r1]` | next 3 |
| 11 | SUB | 2 | `V[r0] -= r1` | next 3 |
| 12 | SUBVAR | 2 | `V[r0] -= V[r1]` | next 3 |
| 13 | ANIM_ON | 1 | `W[0x94b0 + a0*10] = 1` | next 2 |
| 14 | ANIM_OFF | 1 | `W[0x94b0 + a0*10] = 0` | next 2 |
| 15 | RET | 0 | — | next 1, **yield** |
| 16 | JMP | 1 | `pc = lo8(r0)` | continue |
| 17 | CMP | 2 | `x = V[r0]` (**r0 is a variable number**), `y = r1` (**literal, never dereferenced**); flag = 1 if x==y, 2 if x<y, 3 if x>y (signed) | next 3 |
| 18 | CMPVAR | 2 | as CMP with `y = V[r1]` | next 3 |
| 19 | JE | 1 | if flag==1: pc = lo8(r0) else next 2 | continue |
| 20 | JNE | 1 | flag != 1 | continue |
| 21 | JL | 1 | flag == 2 | continue |
| 22 | JA | 1 | flag == 3 | continue |
| 23 | JLE | 1 | flag == 2 or 1 | continue |
| 24 | JAE | 1 | flag == 3 or 1 | continue |
| 25 | ADDINV | 1 | `addinv(a0)` (0x689a) | next 2 |
| 26 | SUBINV | 1 | `subinv(a0)` (0x6ba3) | next 2 |
| 27 | (BRIGHTEN) | 3 | 0xf80(first=a0, count=a1, amount=a2): see palette ops | next 4, `B[0x92a4]=1` |
| 28 | (DARKEN) | 3 | 0xefe(first=a0, count=a1, amount=a2) | next 4, `B[0x92a4]=1` |
| 29 | RPAL | 2 | 0xff9(first=a0, count=a1): rotate towards higher indices | next 3, `B[0x92a4]=1` |
| 30 | LPAL | 2 | 0x10e1(first=a0, count=a1): rotate towards lower indices | next 3, `B[0x92a4]=1` |
| 31 | NEWTHING | 2 | thing a0 ← sprite rect a1: `T+0 = W[0xb80+a1*8]`, `T+2 = W[0xb82+a1*8]`, `T+0x18 = W[0xb84+a1*8]`, `T+0x1a = W[0xb86+a1*8]` with `T = 0x39a5 + a0*0x45`. No range checks. | next 3 |
| 32 | SHOWPCX | 3 | blocking full-screen picture (below) | next 4, continue |
| 33 | LOADPCX | 2 | load file a0 into graphics slot a1 (below) | next 3 |
| 34 | DRAW | 0 | compose the scene (0x61df) and copy the back buffer to VRAM (0x3cb5(0,0,320,200,…)). **No palette upload.** | next 1 |
| 35 | SPEEDXY | 2 | if a0 >= 0: `B[0x987] = a0`; if a1 >= 0: `B[0x986] = a1` (player step x / y, bytes; defaults 10 / 4) | next 3 |
| 36 | PLAYCMF | 1 | only if a sound card is present (`B[0xddf2]==1`): load file a0 into the music buffer (far DS:0xddfb), stop music (SBFMDRV fn 8), set the music pointer, start it (SBFMDRV 6-sequence), re-program the timer to `B[0x97]` Hz (the driver changes it), `B[0xddf1]=1` (music on, looped by the timer ISR), `W[0x984] = a0`. File load failure → fatal 0x68. | next 2 |
| 37 | STOPCMF | 0 | if sound: `B[0xddf1]=0`, stop music, timer back to `B[0x97]` Hz. Always `W[0x984] = -1`. | next 1 |
| 38 | PLAYVOC | 0 | if sound: CT-VOICE fn 6 (play the voice loaded by GETVOC, data at `ptr + W[ptr+0x14]`) | next 1 |
| 39 | PPOS | 1 | player pose: 0 → frame `B[0x91da] = B[0x9c]`, mirror `B[0x91f7] = 1`; 1 → `B[0x9c]`, mirror 0; 2 → `B[0x9b]`, mirror 0; 3 → `B[0x9a]`, mirror 0; other values: nothing | next 2 |
| 40 | GETVOC | 1 | if sound: CT-VOICE fn 8 (stop voice), load file a0 into the voice buffer (far DS:0xddf7), set it as the current voice. Load failure → fatal 0x68 | next 2 |
| 41 | EXIT | 0 | fade out (0xccab), `shutdown(0)` (0xb9c9: stop sound, restore timer/keyboard/mouse, text mode, banner, exit to DOS) | never returns |
| ≥42 / 5 / 6 | unknown | | `*action = 0`, `shutdown(0x6f)` "Commando no valida." (does not return; the `return 2` after it is dead code) | |

### Helpers

* **say(t0..t9) 0x73f5** (SPEAK passes `(a0, -1, 0, …)`): text queue `W[0x9283 + 2k] = tk` (10 words,
  −1 ends it), `B[0x9298] = 0` (queue index), `B[0x9282] = 1` (text active), `B[0x9297] = 1` (spoken by
  the player), `B[0x9299] = 0xff` (last shown index). Player frame: if `B[0x91da]` lies strictly inside a
  walking sequence (`start < f < start+len-1`, checked for (start 0x9c, len 0xa3), (0x9a, 0xa1),
  (0x9b, 0xa2) in that order; lengths are signed bytes) it is set to that sequence's start; then
  `B[0x91dc] = B[0x91da]` (saved pose) and `B[0x91da] = 0` (talking frame). Does **not** stop a walk
  (v2's speak calls `stop`). The per-pass text driver is 0x7513 (UI spec).
* **ptext(x, y, text, colour) 0x74c4**: `B[0x929a] = x`, `B[0x929b] = y`, `B[0x92a7] = colour` (bytes),
  queue = `[text, -1, -1, -1, -1, -1]`, `B[0x9298] = 0`, `B[0x9282] = 1`, `B[0x9297] = 0`,
  `B[0x9299] = 0xff`.
* **goxy 0x7dab**: walk the player to (x, y); LIMIT table DS:0x3392 (30 × 14 bytes) redirection, then the
  LIMITMOV table DS:0x30fe (30 × 0x16) — see the movement spec.
* **addinv(obj) 0x689a**: first free inventory slot `s` in 0..29 (record `R = 0x35a9 + s*0x22`:
  `R+0..0x1d` name, `R+0x1e` used, `R+0x1f` = 0, `R+0x20` object number): opens the inventory file
  (`-n` mode: `<name>.999`, record at `obj*0x118`; `-f` mode: RESOURCE.DAT, table entry 999 read from file
  offset 3996, offset ≤ 0 → fatal 1, record at `entry + obj*0x118`), reads 250 bytes of description into
  `0x728f + s*250` and 30 bytes of name into `R`, decodes them (description −0x1E, name −0x20, up to the
  first NUL). No duplicate check; if every slot is used, nothing is added (0xeab3 = fcloseall/cleanup).
* **subinv(obj) 0x6ba3**: first slot with `R+0x20 == obj`: `used = 0`, `obj = 0`, then compacts
  (0x6beb): for s from that slot while slot s+1 is used: copies used/object bytes and the name from slot
  s+1, **but copies the description strings of the level text table (`0x41bb + (s+1)*250 → 0x41bb +
  s*250`) instead of the inventory descriptions at 0x728f** — QUIRK (original bug): inventory
  descriptions stay at their old slots and level texts s.. get shifted. Must be reproduced if the game
  depends on it (descriptions/texts shown after a SUBINV).
* **Palette ops** (all on the single working palette DS:0x9924, 256×3 6-bit values; dirty flag
  `B[0x92a4]` → the main loop uploads it to the DAC, 0x57ec→0xcddf). For all four: if
  `first + count > 255` then `count = 256 - first`.
  * 27 (0xf80): for each channel byte c of colours first..first+count-1: if `c > 0` then
    `c = (c + amount) & 0xff` (no clamp; the DAC uses the low 6 bits).
  * 28 (0xefe): for each channel: if `c - amount > 0` then `c -= amount`, otherwise **unchanged** (a
    channel never reaches 0 by darkening; it stops once `c <= amount`).
  * 29 RPAL: `saved = pal[last]`; `pal[i] = pal[i-1]` for i = last..first+1; `pal[first] = saved`.
  * 30 LPAL: `saved = pal[first]`; `pal[i] = pal[i+1]` for i = first..last-1; `pal[last] = saved`.
  * Unlike v2 there is no separate original palette: changes are cumulative and in place.
* **SHOWPCX(file=a0, mode=a1, secs=a2)**: `B[0x95] = 1` (pauses the timer-ISR countdowns of the game),
  saves `W[0x92b0]` (var 10) and sets it to 2; fade out (0xccab, 64 DAC steps); load PCX FILE a0 (name
  `0xa7c + a0*13`) straight into VRAM (A000:0000, far ptr DS:0xde13) with its palette into DS:0x9c24
  (0xa3e; failure → fatal 0x68 "No encuentro archivo"); palette /4 (0x17bc); fade in to it (0xcd25);
  `B[0x96] = 0` (key). Wait:
  * 0: until mouse button 0 or 1 is pressed (0xced7: int33 fn 5 press count > 0 and current status ==
    button+1) or a key (`B[0x96] != 0`).
  * 1: `W[0x92a8] = secs * B[0x97]` (timer ticks; ISR decrements it), wait until 0. Input ignored.
  * 2: same countdown, until it expires or button/key.
  * 3: full countdown, then wait for button/key.
  * other: no wait.
  Then fade out, compose the scene (0x61df), copy it to the screen (0x3cb5), fade in to the game palette
  0x9924, `B[0x96] = 0`, `B[0x95] = 0`, restore var 10. No mode ≥ 10 effects (v2 has 10/20 variants).
* **LOADPCX(file=a0, slot=a1)**: slot 0 → pixels far DS:0xde0f, palette DS:0x9f24; 1 → 0xde07 / 0xa224;
  2 → 0xde0b / 0xa824; 3 → 0xde03 / 0xa524 (fatal 0x6e "No encuentro archivo de graficos" on failure).
  Then palette merge 0x162d (DS:0x9924 = slot 0 palette, entries equal to (0xff,0xff,0xff) are filled from
  slot 3, then slot 1, then slot 2 — rendering spec), `B[0x92a4] = 1`, copies the FILE name
  (0xa7c + a0*13) to the graphics name slot (0xa48 + a1*13) and the FILE table entry (offset, size: 8 bytes
  at 0x9a8 + a0*8) to the graphics table (0x988 + a1*8). For a1 outside 0..3 nothing is loaded but the name
  and table copies still happen (out of bounds).
* Sound helpers: 0xc2b1(name, buf) loads a loose file (`-n` mode); 0xc31d(offset, size, buf) reads `size`
  raw bytes from RESOURCE.DAT at `offset` (`-f` mode, FILE table 0x9a8: **no +5 decoding**, unlike v2).
  0xc644(n) calls SBFMDRV (n=6: set status/instruments/clock/play from the CMF header; 8 stop), 0xc560(n)
  calls CT-VOICE (6 play, 8 stop), 0xc54a sets the voice pointer, 0xc5ce the music pointer.

## 3. Callers

| site | script | base | action / pc / flag | yield handling |
|---|---|---|---|---|
| 0x17de (level 0 start, called from change_level when the level is 0) | ACTION 0 of level 0 | `W[0x2eac]` | B[0x929c] / B[0x929d] / B[0x929e] (after setting 0x929c = 0x929d = 0) | loop until it returns 0; before every re-call clears `B[0x92a0]` (walking) and `B[0x9282]` (text active), so GOXY/SPEAK/RET/GOLEVEL do not stop it. Then checks graphics slots 2/3 are named (fatal 0x6a / 0x6b), loads them, and if `W[0x927c]` (the GOLEVEL target) != 0 re-enters change_level for it, else fatal 0x69. |
| 0x144d (change_level 0x11c9, every other level, after loading the level and fading out) | ACTION 0 | `W[0x2eac]` (only if non-zero) | same globals, 0x929c = 0x929d = 0 first | same run-to-completion loop. Then the player is placed at door `B[0x92a3]` (door table DS:0x2de4, 10 bytes each) and `B[0x929c] = door action (byte +8)`, `B[0x929d] = 0`; no door → start position W[0x980]/W[0x982], action 0. |
| 0x7a82 (in the action state machine 0x764e, every main-loop pass) | the current action | `W[0x2eac + 2*B[0x929c]]` | B[0x929c] / B[0x929d] / B[0x929e] | runs one slice only if `B[0x929c] != 0` and not a pseudo action 100..104, **`B[0x9282] == 0` (no text on screen)** and **`B[0x92a0] == 0` (player not walking)**. Returns 0 → `B[0x929c] = B[0x929d] = 0`. |
| 0x7d72 (animations 0x7d26, every main-loop pass) | animation i = 0..49 | `W[0x94b2 + i*10]` | dummy local / `B[0x94b4 + i*10]` / `B[0x94b6 + i*10]` | only when the countdown `B[0x92a2]` is 0 (then reloaded from `B[0x3537]` after the loop; the 50 Hz ISR decrements it while `B[0x95] == 0`). Runs every entry with `W[0x94b0+i*10] == 1`; returns 0 → `W[pc] = 0`, `W[active] = 0`. No text/walk guard (SPEAK/PTEXT in an animation just retry while a text is shown). |

Animation record (DS:0x94b0, 50 × 10 bytes, loaded with the level): `+0 active (word), +2 base,
+4 pc (word, used as byte), +6 flag, +8 unused`.

Order per main-loop pass (0x5037): level change if `W[0x927c] != W[0x927e]` (change_level, which runs
ACTION 0) → animations (0x7d26) → action state machine incl. the action script slice (0x764e) → text
driver (0x7513, if a text is active) → … (main-loop spec).

How `B[0x929c]` (current action) gets set (summary; details in the main-loop spec):
* 1..49: a script (thing click mode 3, door action, the action field of a thing, the
  object-on-thing table, or scripts setting var… — v1 scripts cannot start an action directly).
* 100 (0x64): walk to the thing (`T+0x35`, `T+0x37`), face (`T+0x39`: 0 frame 0x9c mirrored, 1 0x9c,
  2 0x9b, 3 0x9a), say the thing's text `T+0x43`. Click mode (`T+0x3f`) 0 starts at pc 0, mode 1 at pc 2
  (no walk).
* 104 (0x68): walk + face, then `B[0x929c] = T+0x41` (the thing's action). Mode 2. Mode 3 sets
  `T+0x41` directly.
* 101 (0x65): walk + face, then action `T+0x3d` or, if 0, say fixed text 1.
* 102 (0x66) / 103 (0x67): inventory object used on a thing; table DS:0x300e (30 × 8:
  `{mode, thing, object, action}`), mode 1 → 102 (walk first), 2 → 103 (no walk), action in
  `B[0x35a7]`, none → say fixed text 2.
* Not interpreter calls but related: combining two inventory objects (0x7a9a) uses the per-level table
  DS:0x2f14 (50 × 5 bytes `{used, obj1, obj2, new, mode}`; mode 0 removes both, 1 obj1, 2 obj2; then
  ADDINV new and say fixed text 4; no match → fixed text 3).

## 4. Variables and script-visible state

Far pointer table `ptr[n]` at DS:0x0ea4 (n = 0..1999), built at start-up (0xc00c..0xc25f):

| n | address | notes |
|---|---|---|
| 0 | 0x91f5 | (player; see movement spec) |
| 1 | 0x91da | player frame (byte; word access also touches 0x91db) |
| 2 | 0x91dd | player x |
| 3 | 0x91df | player y |
| 4 | 0x91e9 | player width |
| 5 | 0x91eb | player height |
| 6 | 0x91f7 | mirror flag (byte) |
| 7 | 0x9282 | text active (byte; word write also clobbers 0x9283 = low byte of the first text-queue entry) |
| 8 | 0x92a5 | player speech colour (byte; word write also sets 0x92a6 = var 9) |
| 9 | 0x92a6 | (byte; word write also sets 0x92a7 = PTEXT colour) |
| 10 | 0x92b0 | (word; SHOWPCX saves/forces it to 2) |
| 11 | 0x3539 | word (init 2) |
| 12 | 0x353b | word (init 15) |
| 13..29 | 0000:0000 | **unmapped: null far pointer** — reads/writes hit the real-mode IVT word at 0000:0000 (INT 0 vector offset). All unmapped variables alias that one word. |
| 30 + 20t + k, t = 0..24 | thing t (`T = 0x39a5 + t*0x45`) | k: 0 → T+0x22, 1 → T+0x0c, 2 → T+0x3f (click mode), 3 → T+0x41 (action), 4 → T+0x3d (default action), 5 → T+0x10, 6 → T+0x12, 7 → T+0x24, 8 → T+0x35 (walk-to x), 9 → T+0x37 (walk-to y), 10 → T+0x39 (facing), 11 → T+0x3b, 12 → T+0x43 (text); k 13..19 unmapped. Only things 0..24 are mapped although 30 are loaded. |
| 530..999 | unmapped | null pointer as above |
| 1000..1999 | far buffer `*(DS:0x0ea0)` + 2*(n-1000) | 1000 user words, zeroed at start, kept across levels |

Variable numbers seen in Manolo's scripts: 0..36, 50, 90, 97, 115, 170..430 (things), 1044, 1300..1401.

Other state written directly by ops: W[0x927c] level, B[0x92a3] door, text block 0x9282..0x9299 /
0x929a/0x929b/0x92a7, anim table 0x94b0, inventory 0x35a9.. (30 × 0x22) and descriptions 0x728f
(30 × 250), level texts 0x41bb (SUBINV quirk), palette 0x9924 + dirty B[0x92a4], thing records
0x39a5 (30 × 0x45), graphics slots / names 0xa48 / table 0x988, B[0x986]/B[0x987] speeds,
W[0x984] current music, B[0xddf1] music-on, player frame/mirror 0x91da/0x91f7/0x91dc, B[0x95], B[0x96],
W[0x92a8].

## 5. Differences from v2 (RUNDAT 0x2ca2) at a glance

* No 51-op limit; five prefetched (dereferenced) args instead of four.
* No ACTION (5), STEXT, MOUSEACTION, PMOV, MMOUSE, MENU, DEFSCR, RAND, SEEINV, NEWMOVS, DRAWANIM,
  LOADRGB, PUTPAL, NEWMOUSE, PLAYFLI, FIRST/NEXTFRAME, DELAY, LOADDATA, SCALEXY (ops ≥ 42 are invalid).
* GOLEVEL has 2 arguments (no transition mode), pc += 3.
* CMP compares variable r0 with the literal r1 (v2: two dereferenced values); op 18 CMPVAR exists only
  in v1.
* Ops 27/28 are brighten/darken with the quirks above (v2: SHADOW / MODPAL on an original palette).
* NEWTHING copies from the 100 × 8-byte table at DS:0xb80 into thing fields +0/+2/+0x18/+0x1a
  (v2: COORANIM, fields +9/+0xb/+0x21/+0x23) and has no range check.
* DRAW does not upload the palette. SHOWPCX has only modes 0..3 and uses CPU-speed fades.
* SPEAK/PTEXT guard on the byte B[0x9282]; SPEAK does not stop the player.
* SUBINV's description-compaction bug (level texts shifted).
* Sound ops are no-ops without a sound card; resources are raw (no +5).
* Unknown op → fatal 0x6f (same code as v2).

Fatal codes reachable from the interpreter: 0x68 file not found (SHOWPCX, PLAYCMF, GETVOC), 0x6e graphics
file not found (LOADPCX), 0x6f invalid command, 1 inventory file (ADDINV), 0 (EXIT, normal exit).

Uncertain / not verified at runtime: the exact meaning of vars 0, 9..12 and thing fields k = 0, 1, 5, 6,
7, 11 (owned by the main-loop / movement specs); whether the LOADPCX palettes are 8-bit or 6-bit when
merged (rendering spec); sound paths (the emulator runs without a sound card, B[0xddf2] = 0).
