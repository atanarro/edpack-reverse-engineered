# RUNDAT.EXE (EDPACK v2.0): overview — files, functions, EDGAME.DAT, levels, things, variables, bytecode

The first map of the interpreter, written at the start of the work. The other documents in this
folder go into each area in detail and supersede it where they disagree.

> **About these documents.** They were produced while reverse-engineering RUNDAT.EXE (unpacked
> with an emulator and disassembled) and checked against the demo game, whose sources (FUENTES)
> and compiled form (DEMO/EDGAME.DAT) are both available. Paths like `SCRATCH/...` refer to the
> temporary analysis workspace and are not part of this repository. The engine in `src/` is the
> authoritative, tested implementation of what is described here.
>
> **Errata for this overview** (corrected in the later documents and in the code):
> * FILE names are stored with chars +0x19 (gfx names +0x2D, thing names +0x0D, texts +0x0F).
> * Message slots: 1 = NOACINVINV (text 2001), 2 = NOACINVOBJ (2002), 4 = YESACINVINV (2004);
>   0 and 3 keep the default "¡¡¡ ATENCION !!!" text.
> * The in-memory inventory is at DS:8a95 (30 x {u8 used, u8 flag, u16 obj, u16 cache}).
> * The engine timer runs at 50 Hz (DS:dd = 50); one main-loop pass is 2 VGA retraces (~28.5 ms).
> * SELECTEXT = {i16 text[5], i16 used}; SCALEMOV = {used,x1,y1,x2,y2,redW,redH,speedX,speedY};
>   SHADOWMOV = {used,x1,y1,x2,y2,shade}; ACINVTH = {mode,inv,thing,action}.

## Files and tools

Files of the original EDPACK v2.0 kit (not part of this repository):
- MANUAL.TXT (Spanish, cp1252) — language manual. UTF-8 copy: SCRATCH/manual.txt
- FUENTES/*.ED — source of the demo game "Peter's Adventure I"; FUENTES/*.PCX/*.CMF/*.FNT/*.FLI assets
- DEMO/EDGAME.DAT — packed compiled game (same game as FUENTES). RUNDAT.EXE = interpreter for EDGAME.DAT.
- ED.DAT — compiler command table: 24-byte records {char name[20]; u16 opcode; u16 argc}

SCRATCH = the temporary analysis workspace (not kept)
- SCRATCH/RUNDAT.bin, RUNFILES.bin, ED.bin, ED2DAT.bin : unpacked memory images (image loaded at linear 0;
  i.e. file offset == linear address relative to load segment). Borland C++ 1991, 80186 instructions.
- CODE: single near code segment at linear 0 (CS-relative offset == linear address for code < 0x10000).
- DATA: DGROUP (DS) starts at linear 0x12a80 in RUNDAT.bin. "DS:xxxx" below = RUNDAT.bin[0x12a80 + xxxx].
- Disassembler: `SCRATCH/venv/bin/python SCRATCH/fn.py SCRATCH/RUNDAT.bin <hexaddr> <ninsns> [<hexstart>]`
  (prints linear addresses; near call targets are correct; if start omitted it searches back for a prologue).
  capstone + unicorn are installed in SCRATCH/venv. Borland FP emulator uses int 34h-3Dh (capstone garbles those).
- Level decoder prototype: SCRATCH/re/lvl.py, SCRATCH/re/rle.py (python3, run from SCRATCH/re).

## Known runtime functions (RUNDAT.bin linear addresses)
- 0xa836 fatal error(code) — prints message by code
- 0x24ec load level ([DS:fc9c] = level number) from EDGAME.DAT
- 0x1709 read RLE block(dst far, u32 size, fp): byte c>0xC0 -> run of (c&0x3f) copies of next byte; else literal
- 0x2c49 read 30-byte file header: ver bytes -> fd70,fd6f,fd6e ; type byte -> fd6d ; 26 bytes skipped
- 0x2877 NEWMOVS loader (reads .MOV entry, 0x1a0 bytes each, after 30-byte hdr + u16 count)
- 0x2ca2 bytecode interpreter: run(script base di, u8 far *pc, ..., u8 far *cmpflag). Returns 0=finished (opcode 0
  at pc), 1=yield, 2=error. Jump table at 0x3c11, handler = table[op-1], op 1..66.
- 0x1798 lookup file name -> (offset,size) in level FILE table / gfx table
- 0x7fdc GOXY(x,y); 0x75af SPEAK(text); 0x7667 PTEXT(x,y,text,color); 0xabf8 STEXT(sel,mode,mask)
- 0x6ae7 ADDINV, 0x6d3c SUBINV, 0x3c95 NEWTHING(thing,cooranim), 0x587 MODPAL, 0x1a16 RPAL, 0x1abc LPAL
- 0x53ec MOUSEACTION, 0x9ae1 MENU, 0x2aae NEWMOUSE, 0xe4c PLAYFLI(name,mode,time), 0x34a DRAWANIM blit,
  0x3ce8 PLAYCMF(off,size), 0xc240 load file from DAT into buffer, 0x2210 merge palettes?, 0x4149 compose/blit screen,
  0x65a0 ?, 0xa4b ?, 0x688(btn) mouse button?, 0xac5/0xb7f fade in/out palette, 0x450 ?

## EDGAME.DAT container
- Header: u32 table; entry i at file offset 4*i. In the shareware demo: [0..5]=levels 0..5,
  [6]=NIVEL.INV, [7]=NIVEL.FNT (raw font file, copied verbatim), [8]=NIVEL.MOV, [9]=NIVEL.MOU, [10]=NIVEL.IAC,
  [11]=NIVEL.TIT. Level n loaded via fseek(n*4) -> u32 offset.
- Each compiled file starts with 30-byte header: 02 00 00 <type> + 26 zero bytes. type 0=level,1=INV,2=MOV,3=MOU,
  4=IAC,5=TIT, 10+k = LOADDATA data file of kind k.
- Embedded resource files (PCX/CMF/FLI/VOC): stored verbatim but every byte +5 (mod 256). Located through
  absolute (u32 offset, u32 size) tables inside the level.

## Level layout (order of RLE blocks after header; DS destination, size)
 1 DS:43fa  0x34   gfx names: 4 x char[13] (0 ESCENARIOPCX,1 DIBUJOSPCX,2 MOVSPCX,3 INVPCX), chars +0x2D
   (skip 1 byte)
 2 DS:442e  0x104  FILE names: 20 x char[13], chars +0x2D
 3 DS:6c72  0x1b80 LIMITMOV (compiler-processed form, layout unknown yet)
   (skip 1)
 4 DS:87f2  0x230  LIMIT: 40 x {i16 used,x1,y1,x2,y2,tx,ty} (14 bytes)
 5 DS:8b67  0xac8  THINGS: 30 x 92 bytes (see below)
 6 DS:8b49  0x1e   draw/use order: 30 bytes thing indices sorted by INDEX, 0xff = end
   (skip 1)
 7 DS:685e  0xc8   DOOR: 20 x {i16 used,x,y,dir,action}
 8 DS:9823  0x61a8 text pool (25000 bytes), every byte +0x0F (incl. NULs). First 567 bytes reserved/zero.
   (skip 1)
 9 DS:962f  0x1f4  text offsets: 250 x i16 offset into pool (-1 = undefined)
10 far[DS:698a] 0x2710 bytecode: 5000 x i16 words
11 DS:6926  0x64   ACTION entry: 50 x i16 word offset into bytecode
12 DS:1dd0  0x1f4  ANIMATION: 50 x 10 bytes {i16 active, ...} (offset field etc.)
   (skip 1)
13 DS:6b82  0xf0   ACINVTH: 30 x {i16 mode, inv, thing, action/text} (8 bytes)
14 DS:4532  0x3e8  COORANIM: 100 x {i16 x,y,w,h,used}
15 DS:40dc  0x258  SELECTEXT: 50 x 12 bytes (5 text ids + ?)
   (skip 1)
16 5 x RLE(250) messages -> DS:3bfa+i*250, decrypt each char -0x0F until 0 (NOACINVINV, NOACINVOBJ, YESACINVINV, ?, ?)
17 DS:389a  0x168  SCALEMOV: 20 x 18 bytes
18 DS:37aa  0xf0   SHADOWMOV: 20 x {i16 used?, x1,y1,x2,y2,shade} (12 bytes)
19 DS:1194  0xa28  DEFFRAMES: 50 x 52 bytes {u8 ?; u8 items[50] (0xff end); u8 cur}
20 raw DS:435a 0xa0 FILE table 20 x {u32 abs offset in EDGAME.DAT, u32 size}
21 raw DS:433a 0x20 gfx table 4 x {u32 offset, u32 size}
NB: compiler does not clear memory between source files, so blocks for features a level doesn't use can contain
stale data from the previously compiled level (flags usually 0).

## THING struct (92 bytes, base DS:8b67 + t*0x5c), byte-packed
 +0 i16 AREA x, +2 AREA y, +4 AREA w, +6 AREA h, +8 u8 defined(1)
 +9 i16 COOR x (src in DIBUJOSPCX), +0xb i16 COOR y, +0x15 layer (POS com5), +0x19 i16 POS x, +0x1b i16 POS y,
 +0x21 i16 width, +0x23 i16 height, +0x2b mode (POS com4), +0x2d mirror (POS com3),
 +0x31 name (chars +0x0D until NUL, ~15 bytes), +0x40 i16 GOTO x, +0x42 GOTO y, +0x44 GOTO dir,
 +0x48..+0x51 i16 ACTIONS[5]
 Thing var 100+30t+v -> v: 0 +2b,1 +15,2 +21,3 +23,5 +19,6 +1b,7 +2d,8 +40,9 +42,10 +44,11 +9,12 +b,
   15..19 +48,+4a,+4c,+4e,+50, 20..23 +0,+2,+4,+6
## Variables: var n -> far pointer table DS:491e + 4n; ALL accesses are 16-bit words (may overlap bytes!)
 0 fc19,1 fbf5,2 fc01,3 fc03,4 fc0d,5 fc0f,6 fc1b,7 fca9,8 fcd4,9 fcd6,10 fcda,11 8a25,12 8a27,13 fce0,14 fce2,
 15 fce4,16 fce6,17 fce8,18 fcea,19 fcec,20 fcee,21 fd05,22 fd20,23 fd22,24 8a2d,25 00dc,26 fcf2,27 fcf4,
 28 fd04,29 fd01,30 fcff,31 fcfe,32 fd03,33 fbdf,34 fbeb,35 fbed,36 fbe1,37 fbe3,38 fbe5,39 fbe7,40 fbe9,
 41 fd3c,42 fd3e,43 fbdd,44 fd30,45 fd32,46 fd34 ; 1000..1999 -> separate far buffer (persist across levels)
 init function at 0xb78c (sets defaults e.g. fd22=0xbb).
## Bytecode
 Script = word stream; pc is a u8 word offset relative to script base (scripts <= 256 words). Opcode 0 = end.
 Arg >= 25000 => value of variable (arg-25000) (pre-dereferenced for args 0..3). VAR/ADD/... take raw var numbers.
 IF(a .E. b) compiles to CMP a,b ; JE <then> ; JMP <else/end>  (targets = u8 word offsets rel. to script base).
 Ops: 1 GOLEVEL(l,door,mode) 2 GOXY 3 SPEAK 4 PTEXT 5 ACTION(n)=jump to action n 7 VAR 8 VARVAR 9 ADD 10 ADDVAR
 11 SUB 12 SUBVAR 13 ANIM_ON 14 ANIM_OFF 15 RET 16 JMP 17 CMP 19 JE 20 JNE 21 JL 22 JA 23 JLE 24 JAE 25 ADDINV
 26 SUBINV 27 SHADOW 28 MODPAL 29 RPAL 30 LPAL 31 NEWTHING 32 SHOWPCX 33 LOADPCX 34 DRAW 35 SPEEDXY 36 PLAYCMF
 37 STOPCMF 38 PLAYVOC 39 PPOS 40 GETVOC 41 EXIT 42 STEXT 43 SCALEXY 44 MOUSEACTION 45 PMOV 46 MMOUSE 52 MENU
 53 DEFSCR 54 RAND 55 SEEINV(bugged: always sets var22=0) 56 NEWMOVS 57 DRAWANIM 58 LOADRGB 60 PUTPAL
 61 NEWMOUSE 62 PLAYFLI 63 FIRSTFRAME 64 NEXTFRAME 65 DELAY(n,counter-in-code) 66 LOADDATA
 CMP flag: 1 eq, 2 lt, 3 gt. GOXY/SPEAK/PTEXT/RET/GOLEVEL/STEXT yield after executing. SPEAK/PTEXT wait while
 text active ([fca9]!=0). >51 ops without yield => fatal error.
