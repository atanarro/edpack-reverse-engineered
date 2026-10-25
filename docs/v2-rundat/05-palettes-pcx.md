# EDPACK v2.0 RUNDAT.EXE – palettes, PCX loading, fades, SHOWPCX/LOADPCX/LOADRGB

All addresses: code = linear in RUNDAT.bin, data = DS offsets. VGA mode 13h, DAC 6-bit.

## 1. Buffers

Palette buffers in DS, 0x300 bytes each, back-to-back:

| DS     | content | value range |
|--------|---------|-------|
| 0x2280 | **ORIGINAL / base palette** (what MODPAL & SHADOWMOV read from; PUTPAL writes into it) | 6-bit |
| 0x2580 | **CURRENT / displayed palette** (what gets uploaded to the DAC; RPAL/LPAL rotate it; MODPAL writes it) | 6-bit |
| 0x2880 | SHOWPCX temporary palette | 8-bit, then /4 by 0x236c |
| 0x2b80 | slot 0 ESCENARIOPCX palette | 8-bit (raw PCX) |
| 0x2e80 | slot 1 DIBUJOSPCX palette | 8-bit |
| 0x3180 | slot 3 INVPCX palette (comes from NIVEL.INV, see 4) | 8-bit |
| 0x3480 | slot 2 MOVSPCX palette. At startup holds BIOS default DAC (read by 0xbb3 after mode set, 6-bit!) used for the shareware nag screen | 8-bit later |

NB: the names in NOTES ("PUTPAL copies 2580->2280") are right: 2580 is the *current* palette and 2280 the
*original*. (MODPAL(first,cnt,amt,src,dst) is called with src=DS:2280, dst=DS:2580 — the push order is
dst first.)

Image buffers (far pointers, allocated in hw-init 0xa5d5):
- [37a6:37a8] = A000:0000 (the VGA screen itself; SHOWPCX decodes straight onto the screen)
- [37a2] ESCENARIO 64000 B, [379a] DIBUJOS 64000, [379e] MOVS 64000, [3796] inventory buffer **0x8ca0 = 36000 B**,
  [3792] work/sprite layer 64000, [3780] font 0x46b9.
- gfx slot k (0 ESC,1 DIB,2 MOVS,3 INV): name DS:43fa+13k, table DS:433a+8k {u32 off,u32 size}; buffer/palette as above.

Palette dirty flag: **DS:fcd3**. Set to 1 by MODPAL, RPAL, LPAL, SHADOW (op), LOADPCX, LOADRGB, SHADOWMOV change.
Main loop (0x5e3c), once per frame after drawing:
```
if fcd3==1: setpal_vsync(DS:2580); fcd3=0      // 0xb7f: waits 2 vretraces
else:       wait_vretrace(); wait_vretrace()    // 0xbed twice
```
So every frame costs 2 vertical retraces either way (palette changes take effect the next frame, never mid-script).

## 2. Low-level DAC routines

- `0xb68 black()`: DAC 0..255 := 0 immediately.
- `0xb7f setpal_vsync(far pal)`: wait_vretrace (0xbdb/0xbed: loop until 3DA bit3 set, then until it clears,
  i.e. returns at the END of a vertical retrace), write first 384 bytes (colours 0..127), wait again, write
  remaining 384 bytes. Values written as is (must be 6-bit).
- `0xbb3 getpal(far dst)`: read 768 bytes from DAC.
- `0xa4b fade_out()`: read DAC into scratch CS:074b; repeat 64 times: every component: if !=0 then -=1; write
  all 256 colours; busy-wait `loop` with CX=0x7d00 (32000 iterations, CPU-speed dependent, NO vsync).
  Result: black after <=63 steps.
- `0xac5 fade_to(far target)`: read DAC into scratch; 64 times: every component moves 1 toward target (+1/-1,
  equal stays); write DAC; same 32000-iteration busy delay. Target must be 6-bit.
  Timing is CPU dependent: ~0.2 s on a fast 486, ~0.7 s on a 386-33 (estimate). A reimplementation should use
  64 steps over roughly 0.5 s (uncertain; pick by taste) — one step per 70 Hz frame (~0.9 s) is also plausible.

## 3. Palette merge 0x2210 (called at level load 0x1ce7, LOADPCX 0x3487, LOADRGB 0x3964)

```
for i in 0..767: P2580[i] = 0xff            // redundant
for i in 0..767: P2580[i] = PAL_ESC[i]     // 2b80
for c in 0..255 (i=3c):
   if P2580[i..i+2] == (255,255,255):      // ESC colour is "empty" (pure white, 8-bit)
       if PAL_INV[c] != white: P2580[c] = PAL_INV[c]   // 3180
       if PAL_DIB[c] != white: P2580[c] = PAL_DIB[c]   // 2e80 (overrides INV)
       if PAL_MOV[c] != white: P2580[c] = PAL_MOV[c]   // 3480 (overrides both)
for i in 0..767: P2580[i] = P2580[i] / 4 ; P2280[i] = P2580[i]
```
- Priority: ESCENARIO non-white entries always win; for white ESC entries: MOVS > DIBUJOS > INV.
- "white" = exactly 0xff,0xff,0xff in the 8-bit PCX palette. A colour white in all four stays white (63,63,63).
- Consequences: every merge **discards** all MODPAL/SHADOWMOV/RPAL/LPAL/PUTPAL modifications (both 2280 and
  2580 rebuilt). Merge does NOT upload to DAC by itself (fcd3 set by the callers LOADPCX/LOADRGB; level load
  uses its own fade/set).
- The SHOWPCX palette (2880) is never merged.

## 4. Where each palette comes from

- Level load (0x1b64, fc9c = level): DIBUJOSPCX (slot1 name 4407) -> 379a + pal 2e80 (fatal 0x66 if missing);
  ESCENARIOPCX (slot0 43fa) -> 37a2 + pal 2b80 (fatal 0x67). MOVSPCX name+table (slot 2) is saved before
  loadlevel and restored after, so a level's own MOVSPCX entry is ignored after level 0 — the MOVS image/palette
  persist across levels (only LOADPCX/LOADRGB type 2 change them). Then merge 0x2210.
- Level 0 only (0x238e, run after loading level 0): ACTION 0 is run to completion (all yields ignored), then
  MOVSPCX (slot2 name 4414, fatal 0x6a if empty, 0x6c if not found) -> 379e + pal **3480**; then the INV palette
  is read from EDGAME.DAT entry 6 (NIVEL.INV, file offset table at 0x18): 30-byte header (type must be 1, else
  fatal 0xb "NIVEL.INV"), skip 2 bytes, read 0x300 raw bytes (no +5 key) -> 3180. Level 0's ACTION 0 must set
  a new level (fatal 0x69 otherwise), then the real level is loaded (merge happens there).
- Startup (0xa5d5 hw init): mode 13h, getpal(3480) = BIOS default palette, fade_out. Nag screen (main 0xbc0d):
  text drawn in colour 15 on screen, setpal_vsync(3480) (= default VGA palette), wait 6*dd ticks or key,
  black() (0xb68). Then 0xc1df black() again before the game loop.

## 5. PCX decoder (0x1798 lookupfile(char *name, far dst, far pal768))

```
strcpy(errname 219c, name); key = 0
for i in 0..19: if strcmp(FILE name 442e+13i, name)==0: off = FILEtab[435a+8i]; key=5; break
for i in 0..3:  if strcmp(gfx name 43fa+13i, name)==0:  off = GFXtab[433a+8i]; key=5; break   // overrides
if key!=5: fatal(0x63)
fp=fopen(datafile DS:1d80,"rb") (fatal 0x64); fseek(fp,off,SEEK_SET)
if getc-5 != 10: fatal(0x62)          // manufacturer
fseek(+2,SEEK_CUR) -> offset 3: if getc-5 != 8: fatal(6)   // bits per pixel
fseek(+4,SEEK_CUR) -> offset 8: xmax=u16; if xmax+1 != 320 fatal(6); ymax=u16; if ymax+1 != 200 fatal(6)
fseek(+0x74,SEEK_CUR) -> offset 128
for row in 0..199:
   x=0
   while x < 320:
      c = getc-5
      if c > 0xC0: n=c&0x3f; v=getc-5; write v n times; x+=n     // run may cross row end (no clipping)
      else: write c; x+=1                                        // NB 0xC0 itself is a literal pixel 0xC0
getc (palette marker 0x0C, not checked)
for i in 0..767: pal[i] = getc-5        // 8-bit values
return 1
```
- Not checked: encoding, xmin/ymin, planes, bytes-per-line (assumed 320). Header offsets are absolute in the
  embedded file. Writes exactly 64000 bytes for well-formed files (runs crossing line end overflow).
- No transparency handling in the decoder; pixels stored verbatim. (Transparency: sprites colour 0; the
  ESCENARIO/255 rule is handled by the compositor, area 4 — not covered here. The low-level compositor 0x4d5
  copies src->dst but takes the pixel from a 3rd buffer where src==0.)
- `0xc240 loadraw(u32 off, u32 size, far dst)`: reads `size` bytes from data file at off, each byte -5.

## 6. Script ops

Args: a0=[bp-2], a1=[bp-4], a2=[bp-6] (already dereferenced). pc advance shown as "+n"; "continue" means
the interpreter continues with the next op in the same tick (no yield).

### MODPAL(first,count,amount)  op 28, handler 0x317d -> 0x587
```
if first>=0 && first<256 && count>0 && first+count<256:      // colour 255 can never be touched
    n = count (if count>255 unsigned: n=256-first)
    for k in 3*first .. 3*(first+n)-1:
        v = P2280[k] + amount
        P2580[k] = amount>=0 ? min(v,63) : max(v,0)
pc+=4; fcd3=1; continue
```
amount==0 restores original (2280) values. Always relative to 2280, never cumulative.

### RPAL(first,count) op 29, 0x1a16  /  LPAL(first,count) op 30, 0x1abc  (both on P2580 only)
```
if first+count > 255: count = 256-first
RPAL: last=first+count-1; save=P[last]; for c=last down to first+1: P[c]=P[c-1]; P[first]=save   (right: colours move up)
LPAL: save=P[first]; for c=first .. first+count-2: P[c]=P[c+1]; P[first+count-1]=save
pc+=3; fcd3=1; continue
```
No validity check of first (negative values corrupt memory). Rotation is on the current palette, so a later
MODPAL/SHADOWMOV (computed from 2280) undoes it for the affected colours; PUTPAL makes it permanent.

### SHADOW(first,count) op 27, 0x316f
`fd27=first; fd29=count; pc+=3; fcd3=1; continue.` (fd27/fd29 init 0 in main.)

### SHADOWMOV application 0x6930 (called from the main loop when the player position changed, after 0x685f)
```
for i in 0..19:
    S = DS:37aa + 12*i  {used,x1,y1,x2,y2,shade}
    if S.used != 1: return                       // stops at first unused entry
    fx = px(fc01) + pw(fc0d)/2 ; fy = py(fc03) + ph(fc0f)
    if x1<=fx<=x2 && y1<=fy<=y2:
        if S.shade == fd25: return
        fd25 = S.shade
        if 0<=fd27<256 && fd29>0 && fd27+fd29<256: MODPAL(fd27,fd29,shade)
        fcd3 = 1; return
// no region matched: nothing changes (last shade persists)
```
fd25 (current applied shade) is never reset (not on level change, not by SHADOW/merge) → after a merge
(level change / LOADPCX) re-entering a region with the same shade does not re-apply it. Quirk worth
reproducing only if exactness matters.

### PUTPAL() op 60, 0x396f
`P2280 = P2580 (768 bytes); pc+=1; continue.` No DAC change.

### LOADPCX(file,type) op 33, 0x33df
```
type 0: dst=[37a2] pal=2b80 ; 1: [379a],2e80 ; 2: [379e],3480 ; 3: [3796],3180 (undocumented)
other types: dst/pal uninitialised stack garbage (crash)
lookupfile(FILE name 442e+13*file, dst, pal) else fatal(0x6e)
merge 0x2210 ; fcd3=1
strcpy(gfxname[type], FILEname[file]); gfxtab[type] = filetab[file]   // so it persists (e.g. MOVS across levels)
pc+=3 ; continue
```
Type 3 decodes 64000 bytes into the 36000-byte inventory buffer (heap overflow in the original).
The new image is not redrawn until the normal scene redraw / DRAW().

### LOADRGB(file,type) op 58, 0x38e5
```
pal = {0:2b80, 1:2e80, 2:3480, 3:3180}[type]
loadraw(filetab[file].off, filetab[file].size, pal) else fatal(0x6e)   // whole file, bytes -5
merge 0x2210 ; fcd3=1 ; pc+=3 ; continue
```
The file is copied raw: expected to be exactly 768 bytes of 8-bit RGB (merge divides by 4). A larger file
overflows into following buffers. (No sample .RGB in FUENTES; format inferred.) gfx table not updated, so
the palette reverts when the slot is reloaded (e.g. next level for ESC/DIB).

### DRAW() op 34, 0x34e6
`0x65a0(); setpal_vsync(2580); compose full screen (4149 0,0,320,200, 3792,37a2 -> screen); pc+=1 (via 3982); continue.`

### SHOWPCX(file,mode,secs) op 32, 0x31f5
```
[98]=1; saved=fd46; fd46=2; while fd46==2: ;      // handshake with timer ISR (mouse cursor hide), area 1/2
if mode<10:            fade_out(); fx=0                       // includes negative modes
if 10<=mode<20:        clear screen A000 to colour 0 (0x450 0,0,320,200); fx=1; mode-=10
if 20<=mode<30:        fx=2; mode-=20                         // no fade, no clear
(mode>=30: fx uninitialised)
lookupfile(FILE[file], A000:0, DS:2880) else fatal(0x68)      // decoded directly onto the screen
P2880 /= 4 (0x236c)
fade_to(2880)            // all variants: fade from whatever DAC holds now to the PCX palette
dc=0                     // last key
mode 0: wait until btn(0)||btn(1)||dc
mode 1: fcdc=secs*dd; wait fcdc==0                (dd=50 ticks/s at DS:dd; ISR decrements fcdc)
mode 2: fcdc=secs*dd; wait btn||dc||fcdc==0
mode 3: fcdc=secs*dd; wait fcdc==0; then wait btn||dc
other: no wait
0x65a0()
fx==0: fade_out()
fx==1: clear screen to 0
compose full scene to screen
fx==0: fade_to(2580)
fd46=5; setpal_vsync(2580)            // fx 1/2: game palette snaps back instantly
dc=0; [98]=0; fd46=saved
pc+=4; continue (no yield)
```
So: mode 0-3 = fade out / fade in PCX / fade out / fade in game.
+10 = instant black screen (palette still old), PCX drawn, palette fades old->PCX; at the end screen
cleared, scene drawn and palette set instantly.
+20 = PCX drawn over the scene with the old palette, palette fades to PCX; at the end scene redrawn and
palette set instantly.
btn(b) = 0x688: int33 fn5 for button b; true iff press count>0 AND current button status == b+1 (i.e. the
button is still held and is the only one held). dc = last keyboard scancode (set by keyboard ISR).

## 7. Other palette users (for completeness)
- MENU (0x9ae1): saves P2580/P2280 on stack, P2280=P2580, MODPAL(0, var33 [fbdf]=250, -20) darkens colours
  0..249 by 20 (if 0<fbdf<255), menu uses colours 250..254 (vars 33..40 hold menu colours fa..fe);
  on exit both palettes restored and setpal_vsync(2580) (0xa20a).
- Save game (0x98xx) writes P2280 then P2580 (0x300 each) into the save file; load restores both, then
  fade_out (0x8f95) ... compose ... fade_to(2580) (0x9400).
- GOLEVEL (0x1b64..): mode 0 (fcd2==0): fade_out() from old DAC, compose new level, fade_to(2580).
  Modes 1..4 (scroll): setpal_vsync(2580) snap. Modes 5/6: screen cleared then setpal_vsync(2580) (0x21c6).
  (Details belong to area 6.)
- PLAYFLI end (0x1275): clear screen, 0x65a0, setpal_vsync(2580), compose. EXIT (0x365c): fade_out().
- PLAYFLI uses 2b80 (0xe54/0xfbb region) — FLI palette chunks presumably written there/to DAC (area 7).

## Uncertainties
- Fade speed (CPU-bound busy loop).
- LOADRGB file format (assumed 768 bytes 8-bit).
- SHOWPCX modes >= 30 use an uninitialised flag.
- The exact semantics of fd46 / [98] handshake (timer ISR side) not analysed here.
