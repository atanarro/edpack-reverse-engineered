# RUNDAT.EXE: level transitions, GOLEVEL, DEFSCR, level load (spec)

All addresses are RUNDAT.bin linear (code) or DS offsets. Verified by disassembly unless marked (?).

## Relevant globals
| DS | meaning |
|----|---------|
| fc9c u8 | requested/current level number (GOLEVEL arg0). main() init 0 |
| fc9d u8 | level actually loaded. main() init 0xff, so the first main-loop pass loads level 0 |
| fcd1 u8 | door index to enter by (GOLEVEL arg1) |
| fcd2 u8 | transition mode (GOLEVEL arg2) |
| fd07 i16 | DEFSCR scroll step in px (main init 10; 0 is turned into 10 at transition time) |
| fd09 i16 | DEFSCR overlap in px (init 0) |
| fd22 i16 | var23, last row of the game area (init 0xbb = 187) |
| fcc9 u8 / fcca u8 / fccb u8 | current ACTION number / its pc / its CMP flag |
| fd46 i16 | mouse-cursor state, handled by the timer ISR (~0x3ded): 2 or 4 = "erase the cursor, then set 3 (hidden)"; 3 = hidden; 5 = "redraw, then set 0"; 0 = normal |
| far ptrs | [3792] work/sprite layer (320x200, 0 = see-through), [37a2] ESCENARIOPCX background, [37a6] VGA A000, [379a] DIBUJOSPCX, [379e] MOVSPCX (persists), [3780] font |
| palettes | 2b80 ESCENARIO pal, 2e80 DIBUJOS pal, 3480 MOVS pal, 3180 INV pal (from NIVEL.INV), 2580 merged pal (0x2210), 2280 working pal (MODPAL output) |
| player | fc01 x (left), fc03 y (top), fc0d w, fc0f h (var2..5), fbf5 frame (var1), fbf6 previous frame, fc1b mirror (var6), fc09/fc0b frame source x/y in MOVSPCX |

## GOLEVEL (op 1, handler 0x2e74)
```
fc9c = a0; fcd1 = a1; fcd2 = a2       // bytes; args already dereferenced (>=25000 = variable)
if fc9c >= 6: fatal(0xc8)             // shareware: "limitada a los niveles 0..5"
pc += 4; return 1 (yield)
```
Nothing is loaded here. The main loop (0x5687) does this each pass:
```
if fc9c > 5: fatal(0xc8)
if fc9c != fc9d: change_level() /*0x1b64*/; fc9d = fc9c
```
After a yield the current action continues next pass as usual. change_level() then sets fcc9/fcca to 0, so the rest of the action that ran GOLEVEL is dropped (it is not run at the old level).

## DEFSCR (op 53, handler 0x3726)
`if a0 >= 0: fd07 = a0;  if a1 >= 0: fd09 = a1;` then pc += 3, no yield. The values persist for the rest of the game (no reset).

## change_level() 0x1b64
```
top:
  if fc9c > 5: fatal(0xc8)
  fd46 = 2; while (fd46 == 2) {}        // wait for the timer ISR to erase the mouse cursor (fd46 -> 3)
  fcc9 = 0; fcca = 0
  fd2b = 0; fd2c = 0; fd2e = 0          // inventory-open flag and cursor/verb state (cf. main loop 0x50c8)
  save MOVSPCX name (DS:4414, 13 bytes) and gfx-table entry 2 (DS:434a/434e, offset+size)
  (it also saves INVPCX 4421 / 4352..4358 to locals, but never restores them: dead code)
  if !load_level(): fatal(0x65)         // 0x24ec, see below
  if fc9c == 0:                         // level 0 = setup level, never shown
      level0_setup()                    // 0x238e
      if fc9c != 0: goto top            // level 0 must GOLEVEL somewhere, and the MOVSPCX name it loaded is kept
      fatal(0x69)  "Nivel de comienzo no especificado en el Nivel 0"
  for t in 0..29: THING[t].prevPOSx/y/w/h (+1d,+1f,+25,+27) = POSx/y/w/h (+19,+1b,+21,+23)
                                        // so the dirty-rect pass does not erase old rects
  lookupfile(DIBUJOSPCX name DS:4407 -> far[379a], pal DS:2e80)   else fatal(0x66)
  lookupfile(ESCENARIOPCX DS:43fa  -> far[37a2], pal DS:2b80)     else fatal(0x67)
  restore DS:4414 and 434a/434e         // MOVSPCX from level 0 always wins; a later level's MOVSPCX is ignored
  merge_palettes()                      // 0x2210 -> DS:2580
  -- place the player at the door --
  D = DOOR[fcd1]  (DS:685e + 10*fcd1: +0 used, +2 x, +4 y, +6 dir, +8 action)
  if D.used != 0:
      fbf8 = 0
      fc0d = [4334]; fc0f = [4336]      // player w/h from the current NIVEL.MOV entry
      if D.x != -100: fc01 = D.x - fc0d/2       // door x is the player's centre (C int division)
      if D.y != -100: fc03 = D.y - fc0f         // door y is the player's feet
      if D.dir != -100:
          fbf6 = fbf5
          dir 0 (left):     fbf5 = [d1]; fc1b = 1   // side frame, mirrored
          dir 1 (right):    fbf5 = [d1]; fc1b = 0
          dir 2 (forward):  fbf5 = [d0]; fc1b = 0
          dir 3 (backward): fbf5 = [cf]; fc1b = 0
          (any other value: frame and mirror unchanged)
      fc09 = W[200c + 4*fbf5]; fc0b = W[200e + 4*fbf5]
      fcc9 = (u8)D.action               // door action becomes the current action (0 = none)
  else:                                 // door not defined in this level
      fc0d = [4334]; fc0f = [4336]; fbf5 = [d1]   // position and mirror kept
      fc09 = W[200c+4*fbf5]
      fc09 = W[200e+4*fbf5]             // BUG in the original: writes fc09 twice; fc0b keeps its old value
      fcc9 = 0
  fcca = 0
  saved = (fcc9, fcca=0)
  apply_scale_zone()   0x685f   // SCALEMOV (see below)
  apply_shadow_zone()  0x6930   // SHADOWMOV (see below)
  refresh_inventory_bar() 0x6e9b
  -- ACTION 0 of the level, run synchronously to completion --
  fcc9 = 0; fcca = 0
  if W[6926] != 0:                      // entry word offset of ACTION 0 (0 = not defined)
      while (r = INTERP(base=W[6926], &fcc9, &fcca, &fccb)) != 0:   // also loops on r == 2
          if fcce: fcce = 0
          if fca9: fca9 = 0             // texts are cancelled at once; SPEAK/PTEXT do not wait
  fd3c = fc01 + fc0d/2  (var41);  fd3e = fc03 + fc0f (var42)
  rebuild_sprite_layer()  0x65a0        // clear [3792], draw things + player into it
  fcc9, fcca = saved                    // the door action now runs from pc 0 in the main loop
  transition(fcd2)                      // below
  if fc9c >= 6: fatal(0xc8)
  fd46 = 0                              // cursor back on
  DS:dc = 0                             // forget the last key pressed
```
Things to know about ACTION 0:
* It runs inside one frame: nothing is redrawn between yields, and GOXY / DELAY / RET do not pause. Whether GOXY moves the player at all here is unknown (?); the movement code is in the main loop.
* GOLEVEL inside ACTION 0 of a normal level (not level 0): fc9c changes, but the main loop then sets fc9d = fc9c, so the new level is **never loaded**. The level number changes while the old level's data stays.
* ACTION(n) (op 5, 0x2efb) sets fcc9 = n, fcca = 0 and jumps back into the dispatch loop **with the old script base (di) still in place**, so the *calling* script restarts at pc 0 during this call. In the ACTION 0 loop above the base is always W[6926], so ACTION(n) in ACTION 0 probably loops forever or hits fatal 0x34 (more than 51 ops without a yield). This is interpreter behaviour; the interpreter work should confirm it.
* A door action of 0 means "no action", because fcc9 == 0 means "no current action" (the main loop at 0x7ce6 only runs the action when fcc9 != 0).

### level0_setup() 0x238e
```
fcc9 = fcca = 0
run ACTION 0 to completion exactly as above (same loop, same fcce/fca9 clearing)
if DS:4414 (MOVSPCX name) is empty: fatal(0x6a) "Fichero de los MOVIMIENTOS no especificado en el nivel 0"
lookupfile(DS:4414 -> far[379e], pal DS:3480)   else fatal(0x6c)
fp = fopen(DATFILE DS:1d80); fseek(0x18); read u32 off (0 -> fatal 1); fseek(off)
readhdr; if type != 1: fatal(0xb) ("NIVEL.INV")
fseek(+2, SEEK_CUR); read 0x300 bytes -> DS:3180 (INV palette); fclose
```
So level 0's ACTION 0 must call GOLEVEL, and normally also NEWMOVS/NEWMOUSE; see the load section. The door (fcd1) and mode (fcd2) it passes are used for the first real level.

### load_level() 0x24ec
* Opens EDGAME.DAT and reads the u32 at fc9c*4 (0 -> fatal 0x65). Seeks there and reads the 30-byte header (0x2c49: fd70, fd6f, fd6e = version bytes, fd6d = type).
* Version check against DS:94..96 = {2,0,0}: fatal 0x78 ("requiere ED.EXE v%d.%d%d") if fd70 < 2, or if the minor byte is lower.
* Type must be 0, else fatal 0xb ("NIVEL.???").
* Then the RLE blocks listed in [00-overview.md](00-overview.md), in that order.
* The 5 messages (DS:3bfa + i*250) are copied **only if the decoded string is non-empty**. Empty messages keep the previous level's text.
* Then raw reads of the FILE table (DS:435a, 0xa0) and gfx table (DS:433a, 0x20), then fclose.
* 0xc2b4(1): decodes the thing names (each char -13, up to NUL) for the 30 things whose name byte != 0.
* DRAWANIM table: `for i in 0..24: B[3a0f + 20*i] = 0`, so all DRAWANIMs are forgotten.
* Returns 1. Its argument (a stack buffer) is not used.

## State on a level change
Reloaded from the level file, so it takes the compiled values: LIMIT, LIMITMOV, THINGS (modes, positions, etc.), order list, DOOR, texts, bytecode, ACTION entries, **ANIMATION table DS:1dd0** (active flags and pc as compiled), ACINVTH, COORANIM, SELECTEXT, SCALEMOV, SHADOWMOV, DEFFRAMES, file tables. The per-level graphics DIBUJOS and ESCENARIO are reloaded too.

Reset explicitly:
* fcc9/fcca, which then become the door action at pc 0
* fd2b, fd2c, fd2e
* DRAWANIM flags (+0xd)
* fbf8, only when the door is defined
* fcce and fca9, but only if ACTION 0 yields
* DS:dc (last key)
* fd46, through the cursor hide/show cycle

NOT reset (persist across levels):
* DEFSCR fd07/fd09
* SPEEDXY / SCALEXY values fcf2/fcf4/fcfa/fcfc, unless a SCALEMOV zone of the new level matches (below)
* SHADOW range fd27/fd29, and the last shade fd25
* PMOV and FREC state (not touched here; see the main-loop spec)
* MOVSPCX graphics and MOV/MOU data, INV palette
* the inventory
* variables 0..46 and 1000..1999 (except the player fields recomputed above)
* the text timer fca9 when ACTION 0 does not yield

### apply_scale_zone() 0x685f (SCALEMOV DS:389a, 20 x 18 bytes)
`{+0 used, +2 x1, +4 y1, +6 x2, +8 y2, +0a sx, +0c sy, +0e spdx, +10 spdy}`
```
cx = fc01 + fc0d/2; fy = fc03 + fc0f
for i in 0..19:
   if E[i].used != 1: return          // the first unused entry ends the scan
   if x1 <= cx <= x2 and y1 <= fy <= y2:
       fcf2 = sx; fcf4 = sy           // var26/var27 (SCALEXY)
       if spdx != -100: fcfa = spdx   // main() init 10
       if spdy != -100: fcfc = spdy   // main() init 4
       return
```

### apply_shadow_zone() 0x6930 (SHADOWMOV DS:37aa, 20 x 12 bytes)
`{+0 used, +2 x1, +4 y1, +6 x2, +8 y2, +0a shade}`
The scan is the same: the first entry with used != 1 ends it. On the first matching zone:
```
if shade != fd25:
   fd25 = shade
   if 0 <= fd27 < 256 and fd29 > 0 and fd27+fd29 < 256:
       MODPAL(fd27, fd29, fd25, dst DS:2280, src DS:2580)
   fcd3 = 1                            // palette dirty flag
return
```
fd27/fd29 are 0 at start, so nothing happens until the SHADOW op sets them.

## transition(fcd2) (end of 0x1b64)
Helpers:
* `fade_out()` 0xa4b: reads the DAC. 64 steps; each step decrements every component that is not 0, then busy-waits `loop 0x7d00`. CPU-speed dependent, no vsync.
* `fade_to(pal)` 0xac5: from the current DAC to pal. 64 steps of ±1 per component, same busy wait.
* `set_pal(pal)` 0xb7f: instant, two 384-byte halves, each after a vertical-retrace wait.
* `compose(x,y,w,h, A,B,dst)` 0x4149: `dst = A!=0 ? A : B`. Columns x..x+w (**w+1 columns**), rows y..y+h-1, clipped to 320x200. It skips pixels under the mouse cursor unless fd46==3 or fd40==2; here fd46==3.
* `blit(sx,sy,dx,dy,w,h,src,dst,mirror,maxy)` 0x34a: colour 0 is transparent. Pixels are skipped when the src or dst x is outside 0..319 or the y is outside 0..maxy.
* `clear(x,y,w,h,dst,maxy)` 0x450: fills (w+1) x h with 0, clipped to x<=319 and y<=maxy.
* `hscroll(a,b,c,A,B)` 0x2bd1: writes straight to VGA, rows 0..fd22 inclusive:
  `x<c: VGA[y][x]=A[y][x+a];  x>=c: VGA[y][x]=B[y][x+b-c]` (opaque copy).

```
P = DS:2580 (merged palette)
mode 0 (fade through black):
    fade_out(); compose(0,0,320,200, [3792],[37a2] -> VGA); fade_to(P)
mode 1..4 (scroll):
    compose(0,0,320,200, [3792],[37a2] -> [3792])                // full new image in the work buffer
    blit(0,0,0,0,320,199, VGA -> [37a2], 0, fd22)                // old screen copied over the background buffer (transparent!)
    set_pal(P)                                                    // new palette at once: the old image is shown in the new palette while it scrolls
    if fd07 == 0: fd07 = 10
    s = fd07 (each loop below starts here and steps by fd07, with no delay or vsync)
    mode 1 "from the right": while s < 319-fd09: hscroll(s, fd09, 320-s, old=[37a2], new=[3792])
    mode 2 "from the left":  while s < 319-fd09: hscroll(320-s-fd09, 0, s, new=[3792], old=[37a2])
    mode 3 "from above":     while s < fd22+1-fd09:
                                blit(0, fd22-s-fd09, 0,0, 320, s, new -> VGA, 0, fd22)
                                blit(0,0, 0,s, 320, fd22-s+1, old -> VGA, 0, fd22)
    mode 4 "from below":     while s < fd22+1-fd09:
                                blit(0,s, 0,0, 320, fd22+1-s, old -> VGA, 0, fd22)
                                blit(0,fd09, 0,fd22+1-s, 320, s, new -> VGA, 0, fd22)
    blit(0,0,0,0,320,199, new [3792] -> VGA, 0, fd22)              // final frame (transparent: colour-0 pixels keep what was there)
    lookupfile(ESCENARIOPCX -> [37a2], pal 2b80)  else fatal(0x67)   // restore the background that was used as scratch
    clear(0,0,320,fd22, [3792], fd22); rebuild_sprite_layer()
mode 5 (black, no fade):
    rebuild_sprite_layer(); clear(0,0,320,fd22, VGA, fd22); set_pal(P)
    compose(0,0,320,200, [3792],[37a2] -> [3792])
mode 6 (cut):
    rebuild_sprite_layer(); set_pal(P); compose(0,0,320,200,[3792],[37a2] -> [3792])
mode >= 7: nothing. The palette is not uploaded and the screen is not redrawn here.
```
Notes:
* **Modes 5/6 compose into [3792], not into VGA (exactly as in the code).** This bakes the background into the work buffer. The image reaches the screen only through the main loop's normal drawing. In mode 5 the screen is cleared to colour 0 with the new palette until then.
* The scroll loops end before the offset reaches 320 (or fd22+1), less the overlap. The final frame is drawn with the transparent blit.
* The "overlap" fd09 makes the incoming image start fd09 pixels into the new picture (modes 1/4) or end fd09 pixels before its edge (mode 2/3).
* The scroll speed depends on the CPU: each step is one full-screen software copy, with no timing.
* Faithful reproduction of the colour-0 artefacts is optional. They come from the transparent blits:
  - the copy of the old screen over the background buffer;
  - the step blits of modes 3/4;
  - the final frame.

## NIVEL.INV / NIVEL.MOV / NIVEL.MOU loading
* NIVEL.INV (DAT entry 6, offset 0x18): only its palette is read here, 0x300 bytes after hdr+2 into DS:3180, once, in level0_setup. Inventory objects are handled elsewhere (0x6b95 references the "NIVEL.INV" string; not analysed here).
* **NIVEL.MOV and NIVEL.MOU are never loaded automatically.** They are loaded only by the ops NEWMOVS(n) (op 56 -> 0x2877) and NEWMOUSE(n) (op 61 -> 0x2aae). Each op advances pc by 2 and does not yield. In practice level 0's ACTION 0 must call them.
* NEWMOVS(n) 0x2877: DAT entry 8 (offset 0x20); header type 2 (else fatal 0xb "NIVEL.MOV"); u16 count; n >= count -> fatal 0x75 (n formatted into DS:219c). Entry at off + 32 + n*0x1a0:
  `u16 w -> DS:4334; u16 h -> DS:4336; u8 -> cf, d0, d1, d3, d4, d2, d6, d7, d8, d9, da, db (in this order); 400 bytes -> DS:200c (100 x {u16 x, u16 y} frame source coords in MOVSPCX)`.
  - cf / d0 / d1 are the standing frames back / front / side used at door entry. The others are frames used by movement (not analysed here). Before any NEWMOVS, DS:cf..d5 = 0xff.
  - It does not update fc0d/fc0f/fc09/fc0b itself; the next level entry or the movement code does.
* NEWMOUSE(n) 0x2aae: DAT entry 9 (offset 0x24); type must be 3, else fatal 0xb. The error message wrongly names "NIVEL.MOV" (DS:114).
  - u16 count; n >= count -> fatal 0x74.
  - Entry stride is 0x3c (60), but it reads **0x48 (72) bytes** to DS:1fc4 at off + 32 + n*60: an over-read of 12 bytes from the next entry, which land in DS:2000..200b.
  - Then calls MOUSEACTION(0) (0x53ec).
