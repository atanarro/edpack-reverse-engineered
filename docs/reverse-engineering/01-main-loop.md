# RUNDAT.EXE (EDPACK v2.0): main loop, scheduling, input dispatch, scene composition — spec

Addresses: code = linear in RUNDAT.bin (near call targets wrap mod 0x10000); data = DS offsets (DS base = linear 0x12a80).
Verified by disassembly unless marked **(?)**. Companion specs written in the same pass (same directory):
* `spec_level.md` — GOLEVEL/DEFSCR, level loader 0x24ec, change_level 0x1b64, ACTION 0 at entry, DOOR, NEWMOVS/NEWMOUSE, transitions.
* `spec_palette.md` — palette buffers, merge 0x2210, MODPAL/RPAL/LPAL/PUTPAL, fades, PCX decoder, SHOWPCX/LOADPCX/LOADRGB.
* `spec_timing_media.md` — PIT/ISR, ticks, SETUP.CFG, PLAYFLI, CMF/VOC, EXIT, nag screen.
* `spec_menu.md` (earlier pass) — keyboard ISR, MENU.
This file covers the main loop and everything else of the task, with short summaries of those specs where they are needed.

---------------------------------------------------------------------------------------------------
## 0. Buffers and key globals

Far buffers (allocated by hw_init 0xa5d5, all 320 wide, offset = y*320+x):

| ptr DS | size | role |
|---|---|---|
| [37a6] | A000:0000 | VGA screen |
| [37a2] | 64000 | **background** = ESCENARIOPCX (DRAWANIM stamps into it) |
| [3792] | 64000 | **overlay / sprite layer**: things + player + texts, colour 0 = empty |
| [379a] | 64000 | DIBUJOSPCX (thing graphics, COORANIM source) |
| [379e] | 64000 | MOVSPCX (player frames **and mouse cursor graphics**) |
| [3796] | 36000 | NIVEL.INV data / INVPCX (inventory object records 0x4aa bytes each, name at +0xfa) |
| [3780] | 0x46b9 | font (2 header bytes -> fd66 space width, fd67 char spacing/height?, then 0x2200 bytes, 0x22 bytes/char; first byte/16 = glyph width) |
| [698a] | 10000 | bytecode (5000 words) |
| [491a] | 2000 | variables 1000..1999 (persist across levels) |

Screen pixel = `cursor pixel (MOVSPCX rect, !=0) ? cursor : overlay!=0 ? overlay : background`.
**There is no other layering**: no code treats colour 255 of ESCENARIOPCX as transparent (searched every
`cmp ..,0xff`; the only 0xff/255 tests are the palette-merge "white = empty colour" rule in 0x2210 and the
MODPAL/RPAL/LPAL colour-255 guards). The manual sentence "en ESCENARIOPCX será el color 255 el transparente" is
not implemented by RUNDAT; it probably refers to the palette merge (unused colours = white, colour 255 = white). (?)

Player globals (variables in brackets):
fc19 visible [var0]; fc17 prev-visible; fbf5 current MOV frame [var1]; fbf6 prev frame; fbf7 saved frame while
speaking; fc01 x(left) [var2]; fc03 y(top) [var3]; fc05/fc07 prev x/y; fc0d w [var4]; fc0f h [var5]; fc11/fc13
prev w/h; fc1b mirror (byte) [var6]; fc1c prev mirror; fc09/fc0b source x,y of current frame in [fbef] buffer;
fbef/fbf1 far source buffer (= [379e] MOVSPCX, or [379a] DIBUJOS while PMOV>=0), fbf3 = 2/3 tag;
fcf2 [var26] / fcf4 [var27] = pixels removed from width/height (scaling); fcf6/fcf8 prev; fbf8 = PMOV active;
fbf9/fbfb/fbfd/fbff = PMOV COORANIM rect; 4334/4336 = frame w/h of the current NIVEL.MOV entry;
cf/d0/d1 = standing frame for back/front/side, d6/d7/d8 = walk cycle lengths (see spec_level NEWMOVS);
DS:200c = 100 x {u16 x,u16 y} frame source coords; fcce = walking flag; fccf = walk step countdown;
fd3c [var41] = fc01+fc0d/2, fd3e [var42] = fc03+fc0f (feet) — recomputed by the walker.

Scheduling globals: fcc9 = current ACTION number (u8; 0 = none, 1..49 = script, 100..106 = internal states);
fcca = its pc (u8 word offset); fccb = its CMP flag; fd36 = pending text id for state 100; fcdf = target thing for
states 102/104; 8a93 = ACINVTH result (u8!); fca9 [var7] = text on screen; fcbf = 1 SPEAK / 0 PTEXT;
fa2e = text timer (ISR ticks); fcd0 = animation countdown; fd2f = verb used for the current click.

Mouse/cursor: fd4a/fd4c mouse x/y (top-left of cursor; read in the ISR from the int33 callback values),
fd5e/fd60 cursor hotspot, fd42/fd44 cursor src x/y in MOVSPCX, fd52/fd54 cursor w/h, fd2c current cursor (verb)
0..4 (5 = inventory-object cursor), fd40 [MMOUSE mode] 0..4, fd46 cursor ISR state (0 normal, 2/4 = erase then 3,
3 hidden, 5 = redraw then 0), fd48 prev state. fd5a/fd5c = click point (mouse+hotspot).
Hover: fc22 = hovered thing+1 (0 none), fc23 previous. Inventory: fd2b inventory screen open, fca7 hovered
inventory slot, 8a8c "object held" (0/1/0xff), 8a8d held object, 8a8e/8a92 the two operands, 8a90 pending
combination flag, 8a91 = 100 when the second operand is a THING.
Info bar ("verb line"): 8a3c string, 8a29 state (0 none, 1 drawn-needed, 2 rebuild), 8a2a last drawn state,
8a2b/8a2d x/y (8a2d = var24, default 189), 8a34/8a36 w/h(=11), 8a2f/8a31/8a38/8a3a prev rect, 8a33 thing+1 in
label, fd01 [var29] bar enabled, fd02 prev, 8a25 [var11] / 8a27 [var12] colours.

---------------------------------------------------------------------------------------------------
## 1. Program start — main() 0xb78c (called by Borland C0 startup with argc, argv)

```
main(argc, argv):
  fd65 = 2; fd64 = 3; W[dd] = 50                        // dd = timer frequency (Hz) passed to 0xde7 (see timing spec)
  if argc > 1 && strcmp(argv[1], "SETUP") == 0:         // helper for .BAT files
      f = fopen("SETUP.CFG","rb"); if !f: puts(" Fichero SETUP.CFG no encontrado.\n Ejecute SETUP.EXE\n"); exit(0)
      b = fgetc(f); fclose(f); exit(b==1 ? 1 : b==2 ? 2 : 0)     // errorlevel = SETUP.CFG byte 0
  f = fopen("SETUP.CFG","rb")
  if !f: fatal(2000) (0x7d0)   else:
      fgetc(f)                                          // byte 0 (sound card choice; used only by "SETUP" errorlevel)
      s = fgetc(f)                                      // byte 1 = speed  -> walk & animation periods
         s=0: 8a22=15, 8a23=10 | 1: 8, 5 | 2: 4, 2 | 3: 3, 1 | 4: 1, 0   (other: unchanged = 0)
      t = fgetc(f)                                      // byte 2 = text speed -> ticks per character
         t=0: 8a24=16 | 1: 12 | 2: 8 | 3: 4 | 4: 2               (other: unchanged = 0)
      fclose(f)                                         // bytes 3..5 are not read here (see timing spec)
  hw_init()  0xa5d5        // mode 13h, read BIOS DAC -> 3480, black, FM driver detect, hook int 8 (ISR 0x3dc5),
                           // set timer rate W[dd], hook int 9 (keyboard -> DS:2200 key map, DS:dc last scancode),
                           // CT-VOICE.DRV load, malloc buffers (table above; fatal(3) if no memory),
                           // mouse: reset (fatal(5) if none), hide, y range 0..190, x range 0..315, handler, pos 160,100
  datname = argc>=2 ? strncpy(1d80, argv[1], 40) : "EDGAME.DAT"   // NB argv[1] is the data file name
  f = fopen(datname) else fatal(100)
  NIVEL.IAC: fseek(40) read u32 off (0 -> fatal 8); fseek(off); readhdr; type must be 4 else fatal 0xb ("NIVEL.IAC");
             fread(DS:698e, 500 bytes)       // ACINVINV table 50 x 10 bytes, see §5.4
  NIVEL.TIT: fseek(44) u32 (0 -> fatal 0xa); readhdr; type 5 else fatal 0xb; fread(DS:9d, 50) title;
             fread(DS:fd62, 2)               // 2 more bytes (?) unknown use
  NIVEL.FNT: fseek(28) u32 (0 -> fatal 2); fseek; fd66 = fgetc; fd67 = fgetc; readRLE(far[3780], 0x2200)
  fclose
  fd46 = 3 (cursor hidden); fd40 = 2 (mouse off)
  clear(0,0,320,200, VGA, 200)
  shareware nag: 4 centred lines at x = 160 - textwidth/2, y = 60, 73, 95, 108, colours (15, 0) using the BIOS
     palette (DS:3480 = DAC read after the mode set):
        "Esta aventura ha sido creada con" / "la version SHAREWARE del EDPACK v2.0" /
        "Para conseguir la version completa" / "mira el archivo PEDIDO.TXT"
  set_palette(DS:3480)                                   // 0xb7f
  fcdc = W[dd]*6                                         // 6 seconds
  B[dc] = 0; while B[dc]==0 && fcdc>0: ;                 // wait 6 s or any key
  black_palette()                                        // 0xb68
  B[dc]=0; fd40 = 0; fd46 = 0
  title: for each char until NUL: c += 0xa5              // decrypt (i.e. -0x5b)
  gfx names DS:43fa/4407/4414/4421 = ""; W[4338] = -1    // INVPCX table size field? (?)
  -- variable defaults (only those visible to scripts listed with var numbers) --
  fca6=0; fd27=fd29=0 (SHADOW range); fcfa=10; fcfc=4 (walk speed x/y, SPEEDXY); fbf8=0 (no PMOV)
  fbef/fbf1 = [379e]; fbf3 = 2
  menu colours: fbe1..fbe9 [var36..40] = 0xfa,0xfb,0xfc,0xfd,0xfe; fbeb [var34]=0xfe; fbed [var35]=0xfd; fbdf [var33]=0xfa
  fcd4 [var8]=1; fcd6 [var9]=15; fcda [var10]=15
  fc9c = 0 (level); fc9d = 0xff (loaded level, forces loading level 0); fc24 = fc26 = -100 (walk targets)
  fccc = 0; 8a25 [var11]=2; 8a27 [var12]=15; 8a2d [var24]=189
  fce8 [var17]=0; fcea [var18]=0; fd05 [var21]=5; fce0 [var13]=2; fce2 [var14]=15; fce4 [var15]=6; fce6 [var16]=15
  fcf0 = -1 (no STEXT); fcf2 [var26]=0; fcf4 [var27]=0
  fcff [var30]=1; fcfe [var31]=1; fd03 [var32]=1; fd01 [var29]=1; fd04 [var28]=1
  fd07 = 10 (DEFSCR speed); fd09 = 0 (overlap); fd20 [var22]=0; fd22 [var23]=187
  fd30 [var44]=2; fd32 [var45]=0; fd34 [var46]=4
  variable pointer table DS:491e (vars 0..46, see NOTES), vars 1000..1999 -> [491a] (zeroed),
  vars 100+30t+v -> THING fields (see NOTES)
  messages 2000..2004 (DS:3bfa + i*250) = strcpy(DS:c44)  (default text, see timing spec / dump)
  DRAWANIM table: for i<25: B[3a0f+20i] = 0
  fd46 = 3; fd4a = fd4c = 100; fd42 = fd44 = 0; fd52 = fd54 = 10; fd4e = fd50 = 0
  black_palette()           // 0xb68
  main_loop()               // 0x5687
  fatal(0)                  // normal termination path (Ctrl+C) — see timing/media spec for what fatal(0) prints
```
Note: NIVEL.INV (entry 6), NIVEL.MOV (8), NIVEL.MOU (9) are **not** loaded here. INV palette is read by level-0
setup; MOV/MOU only by the NEWMOVS / NEWMOUSE opcodes (normally in ACTION 0 of level 0). See spec_level.md.
Level 0 itself is loaded by the first main-loop pass because fc9d (0xff) != fc9c (0).

---------------------------------------------------------------------------------------------------
## 2. Main loop 0x5687 (exactly in this order, one pass = one "frame")

```
main_loop():
 do {
  // ---- (a) menu key
  if B[dc] == 0x39 (SPACE scancode) && B[fd04](var28) == 1:
      B[98]=1; MENU() /*0x9ae1*/; B[dc]=0; B[98]=0; B[1bbe]=0xff
  // ---- (b) inventory screen
  if B[fd2b] == 1:
      inventory_frame()  /*0xb4e3*/
      if fd2b != 0: continue                              // nothing else runs while the inventory is open
      rebuild_overlay() /*0x65a0*/; compose(0,0,320,200, overlay,bg -> VGA)
      build_label() /*0x52c0*/
      fd46 = 5; while fd46 == 5: ;                        // wait for the ISR to redraw the cursor
      continue
  // ---- (c) STEXT (dialogue choice) active
  if W[fcf0] != -1:
      stext_frame(fcf0) /*0xaefd*/
      if fcf0 != -1: continue                             // everything else frozen (actions, animations, walking)
      rebuild_overlay(); compose(0,0,320,200 -> VGA); continue
  // ---- (d) level change
  if fc9c > 5: fatal(200)                                 // shareware limit (also checked in many other places)
  if fc9c != fc9d: change_level() /*0x1b64*/; fc9d = fc9c
  // ---- (e) logic
  run_animations()   /*0x7f6f*/                           // §3.2
  run_action()       /*0x7937*/                           // §3.1 (incl. walk-then-act states)
  if fca9 == 1: text_frame() /*0x769c*/                   // §6
  input()            /*0x500d*/                           // §4  (mouse, hover, clicks)
  walk_step()        /*0x45d2*/                           // player movement & walk/talk animation (other spec)
  if B[8a29] == 2: build_label() /*0x52c0*/
  update_layers()    /*0x5606*/                           // §7.3
  if (fcc9 in 1..49) || fca9 == 1:                        // hide the info bar while an action/text runs
      B[8a29]=0; B[8a2a]=0xff; B[8a33]=0
  // ---- (f) drawing (dirty rectangles), §7
  B[97] = 1                                               // ISR does not sample the mouse while 97==1
  redraw_drawanims_if_needed()
  erase_changed()          // clear old rects in the overlay
  draw_all_into_overlay()  // things behind, player, things in front, texts, info bar
  B[97] = 0
  if B[fcd3] == 1: set_palette_vsync(DS:2580) /*waits 2 retraces*/; fcd3 = 0
  else:            wait_vretrace(); wait_vretrace()       // 0xbed x2
  copy_changed_to_screen()
 } while (B[2200+0x2e] == 0 || B[2200+0x1d] == 0)          // exit when 'C' (0x2e) and CTRL (0x1d) are both down
 return                                                   // -> main: fatal(0)
```
Timing consequence: **one pass = drawing time + 2 vertical retraces**, i.e. at most 35 passes/s on a 70 Hz VGA
(mode 13h), regardless of SETUP.CFG. Every script yield of an ACTION therefore lasts one pass (~1/35 s).
Animations, walking and text duration are additionally gated by ISR countdowns (fcd0, fccf, fa2e) — §3.2, §6,
timing spec. There is no other sleep in the loop.

---------------------------------------------------------------------------------------------------
## 3. Scripts

Interpreter: `r = INTERP(base, u8 far *idbyte, u8 far *pc, u8 far *cmpflag)` (0x2ca2; cdecl: base word pushed last).
`base` = word offset of the script in the bytecode; `pc` relative u8 word offset; returns 0 = END reached
(word at base+pc == 0), 1 = yielded, 2 = error (unknown opcode: `*idbyte = 0`, fatal(0x6f)). More than 51 ops
in one call -> fatal(0x34). `idbyte` is only written on that error path... and see ACTION(n) quirk below.

### 3.1 run_action() 0x7937 — the current ACTION (fcc9/fcca/fccb)
fcc9 values >= 100 are internal "approach" states driven by fcca as a step counter. `face(t)` below =
```
face(t):  d = THING[t].GOTOdir (+0x44)
          d==0: fbf5 = B[d1]; fc1b = 1      d==1: fbf5 = B[d1]; fc1b = 0
          d==2: fbf5 = B[d0]; fc1b = 0      d==3: fbf5 = B[cf]; fc1b = 0      other: unchanged
          (d0..d1/cf from NEWMOVS. Manual: 0 left, 1 right, 2 "adelante", 3 "atrás")
walkto(t): GOXY(THING[t].GOTOx (+0x40), THING[t].GOTOy (+0x42))     // 0x7fdc, ±10000 / -100 handled by GOXY
```
```
T = B[8a8e]  (thing clicked), V = B[fd2f] (verb at click time)
switch fcc9:
 case 100 (0x64)  "approach then say fd36":          // set by verb clicks 0 / +2000 / +3000 / +5000
    fcca==0: walkto(T); fcca=1; return
    fcca==1: if fcce==0 (arrived): face(T); fcca=2;  return
    fcca==2: if fd36 != 0: SPEAK(fd36);  fcc9 = 0;   return
 case 101 (0x65)  "approach then run action":         // verb value 1..999
    fcca==0: walkto(T); fcca=1
    fcca==1 && !fcce: face(T); fcca=2
    fcca==2: a = THING[T].ACTIONS[V]; if a == 0: return (stuck in this state until another click)
             fcc9 = (u8)a; fcca = 0                    // action starts next pass (re-reads ACTIONS now!)
 case 102 (0x66) / 104 (0x68)  "inventory object used on thing fcdf":   (set by ACINVTH dispatch §5.3)
    fcca==0: walkto(fcdf); fcca=1
    fcca==1 && !fcce: face(fcdf); fcca=3
    fcca==3 && fcc9==102: B[8a8c]=0; if B[8a93]: fcc9 = B[8a93]; fcca = 0   else: SPEAK(2002); fcc9=0
    fcca==3 && fcc9==104: B[8a8c]=0; SPEAK(B[8a93] ? B[8a93] : 2002); fcc9=0
 case 106 (0x6a)  "turn then run action":             // verb value 4000..4999
    fcca==0 && !fcce: face(T); fcca=1
    fcca==1: a = THING[T].ACTIONS[V]; if a == 4000: return (stuck)
             fcc9 = (u8)(a - 4000)  /*low byte + 0x60*/; fcca = 0
 default:   // real script
    if fcc9 != 0 && fca9 == 0 && fcce == 0:            // never while a text is shown or the player walks
        r = INTERP(W[6926 + 2*fcc9], &fcc9, &fcca, &fccb)
        if r == 0: fcc9 = 0; fcca = 0                  // action finished
```
Notes / quirks:
* All conditions in the 100..106 states test `fcce == 0` only where shown (e.g. case 100 fcca==2 does not wait).
  Note fcca==1 of case 100 with +5000/+3000 (no walking) just waits for fcce==0 (already 0) -> turns next pass.
* `fcc9` is a byte; ACTIONS values are truncated with `(u8)`. Action numbers >= 50 are not range-checked
  (entry table has 50 words; 100..106 would be taken as states!).
* A script action does **not** advance while `fca9 != 0` (text on screen) or while walking (`fcce`). So
  `GOXY(...)` followed by more ops: the next op runs only after arrival; `SPEAK` (which yields) followed by ops:
  they run after the text expires. This is the engine's implicit "wait".
* ACTION 0 is never run from here (fcc9==0 means none). It is run synchronously by change_level (spec_level).
  The DOOR action becomes fcc9 with fcca=0 after entry and then runs here.
* **ACTION(n) op (0x2efb)**: `fcc9 = n; fcca = 0; goto dispatch` **without reloading the script base**. Inside
  an ACTION: the *calling* script continues from its pc 0 (because &fcca is the pc pointer) in the same call
  until it yields/ends; the next pass then runs script n from whatever pc that left. If the old script hits END
  in that call, run_action clears fcc9 -> action n is cancelled. Inside an ANIMATION: fcc9=n,fcca=0 starts
  ACTION n (as a "current action" from pc 0, next pass), but the animation's own pc is not advanced, so it
  re-executes ACTION(n) forever in the same call -> fatal(0x34) after 51 ops. **The demo game never uses
  ACTION(n)**, so this cannot be checked against real data; reproduce as decoded or treat as "start action n
  and end the current script" (sane interpretation), flagged.

### 3.2 run_animations() 0x7f6f — ANIMATION table DS:1dd0, 50 x 10 bytes
```
struct ANIM { i16 active /*+0, runs iff ==1*/; i16 base /*+2 word offset in bytecode*/;
              i16 pc /*+4, interpreter uses the low byte*/; i16 cmp /*+6, low byte used as CMP flag*/;
              i16 unused /*+8, ? — never referenced by code*/ };
run_animations():
  if B[fcd0] != 0: return                               // ISR countdown
  for i in 0..49:
     if A[i].active == 1:
        dummy = ?;  r = INTERP(A[i].base, &dummy /*stack local*/, &A[i].pc, &A[i].cmp)
        if r == 0: A[i].pc = 0; A[i].active = 0          // finished: off, rewound
  B[fcd0] = B[8a23]                                     // SETUP speed: 10,5,2,1,0 ISR ticks
```
* All active animations advance together, by one yield each, once every (8a23 ticks rounded up to a whole
  pass). With speed 4 (8a23=0) they run every pass. They run **before** the current action in the same pass.
* ANIM_ON(n) (op13) only sets `active=1` (pc not reset: an animation stopped with ANIM_OFF resumes where it
  was; one that finished restarts at 0). ANIM_OFF(n) only sets `active=0`. An animation enabled by a
  lower-numbered animation runs in the same pass; by a higher one / by the action: next time.
* The whole table (active flags, pcs) is reloaded from the level file on every level change (spec_level).
* Animations are frozen while the inventory screen, an STEXT, MENU, or PLAYFLI etc. own the loop
  (those return before 0x7f6f is reached).

---------------------------------------------------------------------------------------------------
## 4. Input — input() 0x500d (called every pass after scripts)

`button(b)` (0x688): int 33h fn 5 for button b; returns 1 iff **a press of b happened since the last query AND
the button status word equals b+1 now** (left: status==1, right: status==2 — i.e. still held, and no other button).
A click released before the next poll is lost; a click with both buttons down is ignored.

```
input():
  if fd46 == 2 || fd46 == 3: return                     // cursor being hidden/hidden
  if fca9 != 0 || fd46 != 0:                            // a text is on screen (or cursor busy)
      if fd40 >= 2: return                              // MMOUSE 2/3/4: buttons cannot skip texts
      if button(0) || button(1): clear_texts() /*0x788d: all 5 slots off, fa2e=0 -> text ends now*/
      return
  if fcc9 != 0 && fcc9 < 50: return                     // a script action is running: no input at all
  // open inventory: mouse at the top row
  if fd2b == 0 && fd4c <= 0 && B[fcfe](var31) == 1 && (fd40 < 2 || fd40 == 4) && B[8a90] == 0:
      fd2b = 1; fd2e = fd2c
      if fd2c != 1 && fd2c != 3: MOUSEACTION(1)          // switch cursor to "Mirar" unless Mirar/Usar
      return
  fd46 = 0
  fc23 = fc22; fc22 = 0
  if fd46 == 0 && B[fcff](var30) == 1: hover()          // 0x54ed, §5.1
  if fc23 != fc22: B[8a29] = 2                          // rebuild info bar
  if (fd40 < 2 || fd40 == 4) && button(1) && B[fd03](var32) == 1: MOUSEACTION(100)   // right click: next verb
  if B[8a8c] == 0xff && B[8a90] == 0xff: B[8a8c] = 0; B[8a90] = 0; B[8a29] = 2      // after an IAC combination
  if B[8a8c] == 1 && B[8a90] == 1:                      // a combination is pending (set below or by inventory)
      if B[8a91] == 100: acinvth_dispatch() /*0x7e0b*/; B[8a90] = 0       // object on THING
      else:              acinvinv()        /*0x7d29*/; B[8a8c] = 0xff; B[8a90] = 0xff   // object on object
  if !button(0): return
  if !(fd40 < 2 || fd40 == 4): return                   // MMOUSE 2/3: buttons off
  fd2f = fd2c                                            // verb of this click
  fd5a = fd4a + fd5e; fd5c = fd4c + fd60                 // hotspot
  if fca9 == 0 && fcc9 != 0 && fd46 != 2: fcc9 = 0       // cancels a pending approach state (100..106)
  fc1f = fc1d = fc1e = 0; fc24 = fc26 = -100             // reset walk path state
  if B[8a8c] && fca9 == 0 && fca7 == 0 && fc22 == 0: B[8a8c] = 0     // click on nothing drops the held object
  if B[8a8c] != 1 && fcc9 == 0 && fc22 == 0 && fd5c <= fd22(var23) && fd2c == 0:
      GOXY(fd5a, fd5c)                                   // verb 0 "Ir a" on empty floor (LIMIT applied in GOXY)
  if B[8a8c] && fca9 == 0 && fc22 != 0:                  // held object used on a thing
      B[8a91] = 100; B[8a92] = fc22-1; B[8a90] = 1; B[8a29] = 2   // dispatched on the NEXT pass (above)
  if fc22 != 0 && B[8a8c] != 1 && fca9 == 0 && fd2c < 5:
      verb_click() /*0x8362*/                            // §5.2
```
Notes: with verbs 1..4 a click on empty floor does nothing. Clicking while walking re-targets (GOXY replaces the
path). Clicking an object with verb 0 never calls the floor GOXY (fc22 != 0) — the thing's ACTIONS[0] decides.
A click while a 100..106 state is pending cancels it (fcc9=0) and then starts the new one.

MOUSEACTION(n) 0x53ec (op 44 and internal): `n==100 ? fd2c++ : fd2c = n; if fd2c > 4 && fd2c != 101: fd2c = 0;
if fd2c == 101: fd2c = 5;` then cursor k=fd2c from table DS:1fc4 + 12k (loaded by NEWMOUSE):
`{src x, src y, w, h, hot x, hot y}` -> fd42, fd44, fd52, fd54, fd5e, fd60; the mouse is moved so the hotspot
stays put (`setpos(fd4a + oldhotx - newhotx, fd4c + oldhoty - newhoty)`), fd48 = -1, **B[8a8c] = 0 (drops a held
object)**, build_label(), mouse x range 0..321-w, y range 0..200-h. So right-click cycles 0->1->2->3->4->0.

---------------------------------------------------------------------------------------------------
## 5. Things: hit test, verbs, ACINVTH, ACINVINV

THING struct (92 bytes @ DS:8b67 + 92t) — fields used here (see NOTES for the rest):
+0 AREA x, +2 AREA y, +4 AREA w, +6 AREA h, +9 COOR x, +b COOR y, +d prev COOR x, +f prev COOR y, +15 layer,
+19 POS x, +1b POS y, +1d prev POS x, +1f prev POS y, +21 w, +23 h, +25 prev w, +27 prev h, +29 prev mode,
+2b mode, +2d mirror, +2f prev mirror, +31 name, +40/+42/+44 GOTO x/y/dir, +48 ACTIONS[5] (i16).
Order list DS:8b49: 30 bytes of thing indices (sorted by INDEX by the compiler); an entry 0xff (-1) is **skipped,
not a terminator** (loops always scan all 30). Things absent from the list are never drawn nor clickable.

### 5.1 hover() 0x54ed
```
if fd40 == 1 || fd40 == 2: return                   // MMOUSE 1 "no marca objetos", 2 off
px = fd4a + fd5e; py = fd4c + fd60
for k in 0..29:
   t = (i8)B[8b49+k]; if t == -1: continue
   if AREAx <= px <= AREAx+AREAw && AREAy <= py <= AREAy+AREAh (inclusive) && (mode == 1 || mode == 3):
       fc22 = t+1; if B[8a33] != t+1: B[8a29] = 2; return      // first match in list order wins
if B[8a8c] == 0 && fca7 == 0: B[8a29] = 0
if B[8a8c] != 0 && B[8a33] != 0: B[8a29] = 2
B[8a33] = 0
```
Hit test uses AREA only (not the drawn POS rect) and ignores var0/layers; requires var30 == 1 (caller).

### 5.2 verb_click() 0x8362 — THING.ACTIONS[verb] decoding
```
T = fc22-1 -> B[8a8e]; V = fd2c -> B[fd2f]; a = THING[T].ACTIONS[V]  (i16)
a >= 5000: fd36 = a-5000; fcc9 = 100; fcca = 1      // stay, turn to GOTO dir, say text a-5000
a >= 4000: fcc9 = 106; fcca = 0                      // stay, turn, run action a-4000
a >= 3000: fd36 = a-3000; fcc9 = 100; fcca = 2      // UNDOCUMENTED: stay, don't turn, say text a-3000
a >= 2000: fd36 = a-2000; fcc9 = 100; fcca = 0      // walk to GOTO, turn, say text a-2000
a >= 1000: fcc9 = (u8)(a-1000); fcca = 0             // stay, run action a-1000 at once (next pass)
a >  0   : fcc9 = 101; fcca = 0                      // walk to GOTO, turn, run action a
a == 0 && V == 0: fd36 = 0; fcc9 = 100; fcca = 0     // "Ir a" with 0: just walk to GOTO and turn
otherwise (a==0 with V!=0, a<0): nothing
```
Texts: SPEAK(n) with n < 1000 = level text, 1000..1999 = inventory text (see §6), >= 2000 = message n-2000.
So "+2000 + 1000" style values (e.g. 3xxx) are NOT inventory texts; 3000..3999 is the undocumented immediate-say.
With +5000 the turn happens on the next pass (fcca=1 waits for fcce==0) and the text one pass later.

### 5.3 acinvth_dispatch() 0x7e0b — inventory object 8a8e used on THING 8a92
ACINVTH table DS:6b82, 30 x 8 bytes {i16 mode, i16 inv, i16 thing, i16 action_or_text}; mode 0 = unused entry
(skipped, not a terminator).
```
for i in 0..29 (table order, first match wins):
   E = ACINVTH[i]; if E.mode == 0: continue
   if (E.inv == O && E.thing == T) || (E.inv == -100 && E.thing == T) ||
      (E.inv == O && E.thing == -100) || (E.inv == -100 && E.thing == -100):    (O = B[8a8e], T = B[8a92])
       B[8a93] = (u8)E.action_or_text; fcca = 0
       mode 1: fcc9 = 102                 // walk, turn, run action
       mode 2: fcc9 = 102; fcca = 3       // run action at once (no walk, no turn)
       mode 3: fcc9 = 102; fcca = 1       // turn (waits fcce==0), run action
       mode 4: fcc9 = 104                 // walk, turn, say text
       mode 5: fcc9 = 104; fcca = 3       // say at once
       mode 6: fcc9 = 104; fcca = 1       // turn, say
       (mode >= 7: fcc9 unchanged!, fcca=0)
       goto done
// no entry matched:
B[8a93] = 0; fcc9 = 102; fcca = 0        // walk to the thing, turn, then SPEAK(2002)
done: B[fcdf] = B[8a92]; B[8a90] = 0
```
Messages (level block 16, DS:3bfa + i*250, SPEAK id 2000+i): verified against the demo level data:
i=1 NOACINVINV ("No creo que^esos objetos^se utilizen asi."), i=2 NOACINVOBJ ("Parece que eso^no funciona."),
i=4 YESACINVINV ("Vale,ya esta hecho."); i=0 and i=3 unused by the compiler and keep main()'s default
"¡¡¡ ATENCION !!!^Este texto no esta definido." (DS:c44). i=0 is also what layout() shows for an empty text.
So 2001 = no IAC combination, 2002 = no ACINVTH match (NOACINVOBJ), 2004 = combination done.
In state 102/104 the thing walked to is fcdf (the TARGET), not 8a8e.
Note B[8a93] is a byte: actions/texts > 255 are truncated; value 0 means "none" -> message 2002.
After 102 runs the action, 8a8c (object held) = 0, i.e. the object cursor is dropped.

### 5.4 acinvinv() 0x7d29 — combine two inventory objects (NIVEL.IAC, DS:698e, 50 x 10 bytes)
`{i16 used(==1), i16 objA, i16 objB, i16 mode, i16 result}`
```
for i in 0..49:
   E = IAC[i]; if E.used != 1: continue
   if (B[8a8e]==E.objA && B[8a92]==E.objB) || (B[8a8e]==E.objB && B[8a92]==E.objA):   // either order
       mode 0: SUBINV(objA); SUBINV(objB)    mode 1: SUBINV(objA)    mode 2: SUBINV(objB)   (3: none)
       ADDINV(E.result); SPEAK(2004); return
SPEAK(2001)
```
(The pending flag 8a90 with 8a91 != 100 is set by the inventory screen code 0xb4e3, not analysed here.)

---------------------------------------------------------------------------------------------------
## 6. Texts (SPEAK / PTEXT / timers)

SPEAK(n) 0x75af: `fcab = n; fcc1 = 0; fcc3 = -1; fca9 = 1; fcbf = 1; fc1f=fc1d=fc1e=0; fc24=fc26=-100`
(stops walking); if fbf5 is inside a walk cycle (d1..d1+d8-1, cf..cf+d6-1, d0..d0+d7-1) it is reset to the
standing frame of that cycle; `fbf7 = fbf5; fbf5 = 0` (frame 0 = talking animation start (?)).
PTEXT(x,y,n,colour) 0x7667: `fcc5=x; fcc7=y; fcd8=colour; fcab=n; fcc1=0; fca9=1; fcbf=0; fcc3=-1`.
Interpreter: SPEAK/PTEXT ops first check `fca9 != 0 -> yield without advancing` (wait for a previous text).

text_frame() 0x769c (only while fca9==1):
```
if fcc1 != fcc3:                               // a new text index to show (only index 0 exists)
   if W[fcab + 2*fcc1] == -1 || fcc1 > 0:      // end of text
       fca9 = 0
       if fcbf == 1: fcbf = 0; fbf5 = fbf7; fc17 = -1 (force player redraw)
                     fc09/fc0b = frame coords of fbf5; fc0d=fc11=[4334]; fc0f=fc13=[4336]
       (skip the rest)
   else:
       n = text id; s = n<1000: pool[9823 + W[962f+2n]] | n<2000: far[3796] + (n-1000)*0x4aa (inventory
           object text, starts at its record +0) | else DS:9ada + 250*n  (= 3bfa + 250*(n-2000))
       SPEAK: layout(x = fc01 + fc0d/2, y = fc03 + fcf4, s, fcd4 [var8], fcd6 [var9])
       PTEXT: layout(fcc5, fcc7, s, fcd8, fcda [var10])
       fcc3 = fcc1
if W[fa2e] <= 0 && fca9 == 1: clear_texts(); fcc1++       // timer expired -> next pass ends the text
```
layout() 0x726d: empty string -> message 2000 text (DS:3bfa); splits at '^' into up to 5 slots of max 56 chars
(slots filled from 4 down to 0), centres each line on x, stacks lines above y (11 px per line, base y-11),
clamps x to [5, 315-w] and y to [5, fd22-12]; **fa2e = 8a24 * (n < 10 ? n+2 : n)** where n = number of
characters (spaces included, '^' excluded) — ISR ticks.
Text slots: 5 x 0x67 bytes at DS:f9cb: +0 u8 on, +1 u8 prev on, +2 x, +4 y, +0xa w, +0xc h (=11), +0x12 char[57],
+0x65 colour, +0x66 border colour (fa2e = slot0+0x63 is an unrelated global inside slot 0's spare bytes). clear_texts 0x788d: all +0 = 0, fa2e = 0.
ISR: `if !B[98] && fca9 == 1 && fa2e > 0: fa2e--`. So duration = 8a24*(n...) ticks; skipping by mouse click
(input(), unless MMOUSE >= 2). The player's talk animation is driven by walk_step (fccf period 2*8a22 while
fcbf != 0).

---------------------------------------------------------------------------------------------------
## 7. Scene composition

### 7.1 Primitives
* `clear(x,y,w,h, dst, maxy)` 0x450: dst[y..y+h-1][x..x+w] = 0 (**w+1 columns**), clipped x 0..319, y 0..maxy.
* `blit(sx,sy,dx,dy,w,h, src,dst, mirror(u8), maxy)` 0x34a: rows r<h, cols c<w; src col = sx+c, or with
  mirror==1 src col = sx+w-1-c; pixel copied iff src != 0 and dst x in 0..319, dst y in 0..maxy, src x in
  0..319, src y in 0..maxy (src y also limited by maxy — quirk). w or h == 0 -> 65536 iterations (bug; avoid).
* `compose(x,y,w,h, ovl, bg, dst)` 0x4149: for rows y..y+h-1, cols x..x+w (**w+1**), clipped to 320x200:
  if the pixel is inside the cursor rect (fd4a <= px <= fd4a+fd52, fd4c <= py <= fd4c+fd54) and fd46 != 3 and
  fd40 != 2 and the cursor pixel MOVS[(py-fd4c+fd44)*320 + px-fd4a+fd42] != 0: skip (cursor stays);
  else dst = ovl != 0 ? ovl : bg. Rejected entirely if x+w<0, y+h<0, x>319, y>199.
* `compose_ovl(...)` 0x4004: same but writes only where ovl != 0 (used for "new" rects after the old rect was
  restored with compose).
* `draw_player(sx,sy,dx,dy,w,h, (float)rw,(float)rh, src,dst, mirror)` 0x429e, called with
  sx,sy = fc09,fc0b; dx = fc01 + fcf2/2 (C int division); dy = fc03 + fcf4; w,h = fc0d,fc0f; rw,rh = fcf2,fcf4;
  src = [fbef]; dst = overlay; mirror = fc1b:
  ```
  if rh >= h: rh = h-1;  if rh < 0: rh = 0;  same for rw/w
  ky = (h-rh)/h; kx = (w-rw)/w            (float32)
  non-mirrored: fy = 0; lastY = -1
     for r in 0..h-1:  Y = ftol(fy) (trunc)
        if Y != lastY:
           fx = 0; lastX = -1
           for c in 0..w-1: X = ftol(fx)
              if X != lastX:
                 p = src[(sy+r)*320 + sx + c]
                 if p && 0 <= dy+Y <= fd22 && 0 <= dx+X < 320: dst[(dy+Y)*320 + dx+X] = p
                 lastX = X
              fx += kx
           lastY = Y
        fy += ky
  mirrored: same but c runs 1..w-1 and src col = sx-1 + w - c (cols sx+w-2 .. sx: the rightmost source column
     is never drawn, image is w-1 wide), and the y clip is dy+Y < fd22 (strict) — quirks of the original.
  ```
  i.e. nearest-neighbour downscale where the FIRST source row/column mapping to a destination row/column wins
  (even if transparent), anchored so the shrunk sprite stays centred horizontally (x+rw/2) and bottom-aligned
  (y+rh). Player clipping bottom = var23 (fd22, default 187).

### 7.2 Dirty detection and draw order (main loop part f, 0x57c9..0x6580)
"changed(thing)" = mode != prevmode || B[fcff] != B[fd00] || mirror != prevmirror || COOR x/y != prev ||
POS x/y != prev || w/h != prev. "changed(player)" = fc01/fc03 != fc05/fc07 || fcf2/fcf4 != fcf6/fcf8 ||
fc0d/fc0f != fc11/fc13 || fc1b != fc1c || fbf5 != fbf6 || fc19 != fc17.
```
// DRAWANIM refresh
if B[fd24] == 1:
   for i in 0..24: if DA[i].state == 2: DA[i].state = 1; compose(DA[i].x, DA[i].y, CA[c].w, CA[c].h -> VGA)
   fd24 = 0
// erase old rects in the overlay
for t in 0..29: if changed(t): clear(prevPOSx, prevPOSy, prevw, prevh, ovl, 200)
for s in 0..4: if slot[s].on == 0 && slot[s].prevon != 0: clear(slot x,y,w,h, ovl, 200)
if changed(player):
   if fc01 != fc05 || fc03 != fc07: scale_zone() /*0x685f*/; shadow_zone() /*0x6930*/   (spec_level)
   clear(fc05, fc07, fc11, fc13, ovl, fd22)
if B[8a29] != B[8a2a] || B[fd02] != B[fd01]: clear(8a2f, 8a31, 8a38, 8a3a, ovl, 200)     // info bar
// draw everything into the overlay (every pass, not only dirty ones)
if B[fcff](var30) == 1:
   for k in 0..29: t = list[k]; skip -1; if mode != 0 && mode != 3 && (layer == 0 || layer == 2):
       blit(COORx, COORy, POSx, POSy, w, h, DIBUJOS, ovl, (u8)mirror, fd22)
if W[fc19](var0) != 0: draw_player(...)
if B[fcff] == 1:
   for k in 0..29: same with (layer == 1 || layer == 3)
for s in 0..4: if slot.on && slot.text[0]: draw_text(slot.x, slot.y, slot.text, slot.col, slot.border, font, ovl)
if B[8a29] != B[8a2a] && B[fd01](var29) == 1 && B[8a29] != 0:
   draw_text(8a2b, 8a2d, 8a3c, B[8a25], B[8a27], font, ovl)
... palette/vretrace (§2) ...
// copy to the screen
if changed(player): compose(fc05, fc07, fc11, fc13 -> VGA)   // old rect
for t: if changed(t): compose(prevPOS rect -> VGA)
for s: if slot.prevon && !slot.on: compose(slot rect)        // vanished text: restore
if B[8a29] != B[8a2a] || B[fd02] != B[fd01]: compose(old info-bar rect)
for t: if changed(t): if mode != 0 && mode != 3: compose_ovl(POS rect); copy cur -> prev fields; fd00 = fcff
if changed(player): if fc19 == 1: compose_ovl(fc01, fc03, fcbf ? [4334] : fc0d, fcbf ? [4336] : fc0f)
                    prev player fields = current
for s: if slot.on != slot.prevon: if slot.on == 1: compose_ovl(slot rect); slot.prevon = slot.on
if B[8a29] != B[8a2a] && B[fd01] == 1: compose_ovl(8a2b, 8a2d, 8a34, 8a36); prev bar = current; 8a2a = 8a29; fd02 = fd01
```
**Equivalent modern rendering:** redraw the full frame each pass as background, then overlay in this order:
things with layer 0/2 in list order, player, things with layer 1/3 in list order, text slots, info bar,
cursor — all with colour 0 transparent, things/player clipped at y <= var23. The dirty-rect machinery gives the
same picture except in these (reproducible-if-wanted) cases:
* Things are drawn into the overlay every pass, but the screen is only updated in changed rects. LOADPCX
  (any type, incl. the background) does **not** redraw: the new pixels appear only inside rects that later
  become dirty (moving sprites "paint" the new image) or after DRAW() / any full recompose. A faithful port that
  redraws everything would show the new image at once (use of DRAW() after LOADPCX is the intended idiom (?); the demo never uses LOADPCX).
* The info bar is drawn into the overlay only on the pass in which it changes; after `rebuild_overlay()`
  (0x65a0: inventory close, STEXT end, DRAW op, DRAWANIM, level entry) it disappears until the next hover
  change. Overlapping sprites erase parts of it (their clear rect uses maxy 200).
* Things not changed but overlapped by a changed rect are correctly refreshed (overlay is complete).
Overlay full rebuild `rebuild_overlay()` 0x65a0: clear(0,0,320,200,ovl,200); B[97]=1; things behind; player;
things front; texts (slot.on only); info bar if 8a29 != 8a2a && 8a29 && var29; B[97]=0. (No screen copy.)
DRAW op (34, 0x34e6): rebuild_overlay(); set_palette_vsync(2580); compose(0,0,320,200 -> VGA); no yield.

### 7.3 Layers (update_layers 0x5606, every pass after input)
```
for t in 0..29: if mode != 0 && layer != 2 && layer != 3:
   layer = (POSy + h >= fc03 + [4336]) ? 1 /*in front*/ : 0 /*behind*/
```
Compares the thing's bottom with the player's bottom using the **unscaled** MOV frame height 4336 (not fc0f,
not the scaled height). Layers 0/1 are therefore recomputed every pass (a script setting 0/1 is overwritten),
2 = always behind, 3 = always in front; values > 3 are never drawn.

### 7.4 THING modes / flags summary
* mode (+2b, var v0): drawn iff mode != 0 && mode != 3 (1, 2 and any other value are drawn); clickable iff
  mode == 1 || mode == 3; layer auto-update iff mode != 0.
* mirror (+2d, var v7): exactly 1 = mirrored (blit compares the byte with 1).
* var30 (fcff): 0 hides all things and disables hover/click on things; toggling marks all things dirty.
* var0 (fc19): player drawn iff != 0; new-rect copy only if == 1.
* var23 (fd22): bottom clip line of things/player/texts and the "Ir a" floor limit (click y > var23 ignored).
* NEWTHING(t, c) 0x3c95 (op 31): `COORx = CA[c].x; COORy = CA[c].y; w = CA[c].w; h = CA[c].h` (fields +9, +b,
  +21, +23). POS, AREA, mode, mirror unchanged; COORANIM.used is ignored. No yield.
* COORANIM DS:4532: 100 x {i16 x, y, w, h, used}.

### 7.5 DRAWANIM (op 57, 0x37ae) — permanent stamp onto the background
Table DS:3a02, 25 x 20 bytes: `{char dibname[13] /*+0*/; u8 state /*+0xd: 0 free,1 shown,2 needs copy*/;
i16 cooranim /*+0xe*/; i16 x /*+0x10*/; i16 y /*+0x12*/}`.
```
DRAWANIM(c, x, y):
  blit(CA[c].x, CA[c].y, x, y, CA[c].w, CA[c].h, DIBUJOS, BACKGROUND [37a2], 0, 200)   // colour 0 transparent
  for i in 0..24: if DA[i].state && DA[i].cooranim==c && DA[i].x==x && DA[i].y==y && strcmp(DA[i].dibname, DS:4407 /*current DIBUJOSPCX*/)==0:
        DA[i].state = 2; fd24 = 1; goto done
  for i in 0..24: if DA[i].state == 0:
        rebuild_overlay(); strcpy(DA[i].dibname, DS:4407); DA[i].state=2; DA[i].cooranim=c; DA[i].x=x; DA[i].y=y; fd24=1; goto done
  fatal(0x72)                         // table full
done: pc += 4 (no yield)
```
The stamp is shown at the next pass (main loop copies the rect). The background is reloaded at level change and
the table is cleared by load_level (spec_level), so DRAWANIMs do not persist across levels; within a level the
stamp persists until the background is reloaded (LOADPCX type 0 / scroll transitions reload ESCENARIO).
(The name field looks intended for re-stamping on re-entry, but no such code was found. (?))

---------------------------------------------------------------------------------------------------
## 8. Timing, ISR, SETUP.CFG — summary (details in spec_timing_media.md)
* PIT channel 0 is reprogrammed by `set_timer_rate(hz)` 0xde7 (divisor 1193180/hz via port helper 0xe3c, so no
  literal `out 43h` exists) with W[dd] = 50 -> **1 tick = 20 ms** (50 Hz). Restored to 18.2 Hz in fatal().
  (spec_menu.md says "100 Hz": that is wrong — the old int 8 handler is chained twice per tick, so BIOS time
  advances 100 counts/s, but the engine tick is 50 Hz.)
* INT 8 handler 0x3dc5 per tick: chains old int 8 (also again at the end); if B[97] != 1 samples the mouse
  position into fd4a/fd4c; runs the cursor state machine (fd46: 2/4 erase -> 3 hidden; 5 redraw -> 0; else redraw
  when moved; cursor drawn straight to VGA from MOVSPCX rect fd42,fd44,fd52,fd54 with colour 0 transparent);
  if B[98] == 0 (not in MENU): u32 118c-- , u32 1190-- (PLAYFLI timers), fa2e-- (only while fca9 == 1),
  fccf-- (player step), fcd0-- (animations), all saturating at 0; always fcdc-- (seconds waits); restarts CMF
  music when it stopped (looping).
* Main loop pacing: no tick wait, **2 vertical retraces per pass (~28.5 ms, 35 Hz)** — one ACTION yield per pass.
  Animations: every pass on which fcd0 reached 0, then fcd0 = 8a23 (ticks). Walking: a step when fccf == 0,
  then fccf = 8a22 (walking) or 2*8a22 (while fcbf != 0, i.e. during SPEAK: the talk animation).
  Text: fa2e = 8a24 * (nchars < 10 ? nchars+2 : nchars) ticks.
* SETUP.CFG: byte0 = sound choice (only used by `RUNDAT SETUP` errorlevel for DEMO.BAT), byte1 = speed
  (0..4 -> 8a22 = 15/8/4/3/1, 8a23 = 10/5/2/1/0), byte2 = text speed (0..4 -> 8a24 = 16/12/8/4/2); bytes 3..5
  unused by RUNDAT. Repo SETUP.CFG 02 04 04: speed 4 (8a22=1: a walk step every pass; 8a23=0: animations
  every pass), text 4 (8a24=2: 40 ms/char). DEMO 02 02 03: speed 2 (step every 4 ticks = 80 ms, rounded up to
  passes; animations every 2 ticks -> every 2nd pass ~57 ms), text 3 (4 ticks = 80 ms/char).
* DELAY(n) op 65 keeps its counter inside the bytecode word after the op and yields n-1 times (counts script
  calls, not time: n-1 passes for an ACTION, n-1 animation steps inside an ANIMATION).
* SHOWPCX/startup waits use fcdc = secs*50. Fades (0xa4b/0xac5) are CPU-speed busy loops (64 steps).
* Keyboard ISR (int 9 0xc42d, BIOS not chained): key map DS:2200[scancode] (word writes), DS:dc = last make
  scancode = var25. SPACE (0x39) opens MENU (if var28 == 1); Ctrl+C held ends the program.

---------------------------------------------------------------------------------------------------
## 9. Level transitions — summary (details in spec_level.md)
* GOLEVEL(l, door, mode) only stores fc9c/fcd1/fcd2 (fatal 200 if l >= 6) and yields; the main loop calls
  change_level() at the start of the next pass ((d) in §2), before animations and actions.
* change_level: cursor hidden, fcc9/fcca = 0, inventory/verb state reset (fd2b=fd2c=fd2e=0 — the cursor table is
  not reloaded, so the cursor graphic stays (?)), load level, graphics, palette merge, player placed at DOOR
  (x = centre, y = feet, -100 keeps), facing, scale/shadow zones, **ACTION 0 run synchronously to completion**
  (yields ignored; fcce and fca9 cleared at each yield: SPEAK/GOXY in ACTION 0 have no visible effect (?)),
  overlay rebuilt, then **fcc9 = DOOR.action, fcca = 0** (runs from the next pass in the main loop), transition
  mode 0..6 (0 fade, 1-4 scroll using DEFSCR fd07/fd09, 5 black+cut, 6 cut), cursor shown, last key cleared.
* Reloaded with the level: THINGS, order list, ANIMATION table (compiled initial active flags/pcs), ACINVTH,
  texts, DOORs, LIMITs, SCALEMOV/SHADOWMOV, COORANIM, DEFFRAMES; DRAWANIM table cleared.
  Not reset: DEFSCR, SCALEXY values fcf2/fcf4 (unless a SCALEMOV zone matches), SPEEDXY fcfa/fcfc, SHADOW range,
  PMOV state (fbf8 is reset only if the door is defined), vars 0..46 and 1000..1999, inventory, MOVSPCX/MOV/MOU.
* FREC: documented in the manual but **not present in the compiler's command table ED.DAT** (no opcode) —
  cannot appear in compiled games; nothing to implement. Walking speed = SETUP byte 1 (8a22) and SPEEDXY.
* Shareware: level numbers >= 6 -> fatal(200) (checked in GOLEVEL and in many drawing/text routines).

---------------------------------------------------------------------------------------------------
## 10. Palettes, SHOWPCX, LOADPCX — summary (details in spec_palette.md)
* DS:2580 = current (uploaded) palette, DS:2280 = original/base. MODPAL reads 2280 -> writes 2580; PUTPAL copies
  2580 -> 2280; RPAL/LPAL rotate 2580. Any change sets fcd3=1 -> uploaded at the end of the pass (§2).
* Merge 0x2210 (level load, LOADPCX, LOADRGB): start from ESCENARIO palette; every colour that is pure white
  (255,255,255) there is taken from INV, then DIBUJOS, then MOVS (later non-white wins); /4 -> 2580; copy -> 2280.
* SHOWPCX modes 0..3 (+10 instant black then fade in, +20 no fade-out): see palette spec; no yield.

---------------------------------------------------------------------------------------------------
## 11. Media / EXIT — summary (details in spec_timing_media.md)
* PLAYFLI(file, mode, secs) op 62 -> 0xe4c: blocking (own loop; animations/actions frozen); standard Autodesk FLI
  (magic 0xAF11 only; FLC rejected, fatal 0x33), chunks 4 COLOR256, 11 COLOR, 12 LC, 13 BLACK, 15 BRUN, 16 COPY,
  others skipped; frame delay = trunc(speed*50/70)+1 ticks; modes 0..11 as in the manual (once / abort on any
  key, ESC, SPACE / loop until key / loop for secs (time checked after a complete pass) / 11 hold last frame
  until secs since start). Screen cleared before and after, palette 2580 restored, full recompose, no yield.
  Known bugs in COLOR multi-packet index, BLACK/COPY sizes (see media spec).
* FIRSTFRAME/NEXTFRAME(def, thing): NEWTHING(thing, DEFFRAMES[def].items[cur]) with cur reset/advanced (wraps at 0xff).
* PLAYCMF(file): same tune already playing -> nothing; else load to 40000-byte buffer, SBFMDRV fn 8,1,2,4,6;
  loops forever (ISR restarts it). STOPCMF: fn 8. Requires SBFMDRV resident ("FMDRV" signature at int 80h..BFh);
  if absent, no sound at all (VOC too). GETVOC(file) loads a VOC (CT-VOICE.DRV fn 8 stop + set buffer);
  PLAYVOC() plays it once (fn 6), no wait. Sound files are limited to 40000 bytes (manual says 30000).
* EXIT op 41: fade_out() then fatal(0): text mode 3, banner "ED PACK v2.0 ... Por Jose Luis Serrano Moreno /
  RUNDAT.EXE v1.0 / Version SHAREWARE", restore vectors, exit. **There is no graphic nag at exit**; the
  "Esta aventura ha sido creada con / la version SHAREWARE del EDPACK v2.0 / Para conseguir la version completa /
  mira el archivo PEDIDO.TXT" screen is shown only at startup (§1), 6 s or a key, BIOS default palette, colour 15.
* Level >= 6 (signed byte compare: 6..127) -> fatal(0xc8) with the "limitada a los niveles 0,1,2,3,4 y 5" text.
