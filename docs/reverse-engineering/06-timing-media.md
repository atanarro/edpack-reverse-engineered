# RUNDAT.EXE – timing, input, FLI, sound, EXIT/shareware (areas 2, 7, 8)

All addresses: code = linear in RUNDAT.bin; data = DS:xxxx (DS linear 0x12a80; the ISRs hard-load DS=0x22a8).
Tools used: SCRATCH/d.py (FP-emulator fix + string annotation).

## 1. Timer (PIT / INT 8)

### PIT programming – `set_timer_rate(hz)` @0x0de7
```
divisor = (hz != 0) ? (u16) ldiv(0x1234DC /*1193180*/, hz) : 0      ; 0 => 65536 (18.2 Hz)
cli; out 0x43,0x36; out 0x40,lo(divisor); out 0x40,hi(divisor); sti
```
(uses port helper 0x0e3c `outb(port,val)`, so there is no literal `out 0x43`).
Called with DS:dd (=50, set at top of main 0xb78c, never changed anywhere) at startup (0xa62d),
after every SBFMDRV start/stop (PLAYCMF 0x3dab, STOPCMF 0x3571, ISR music-loop 0x3fed, savegame load 0x9366),
and with 0 in fatal() (restores 18.2 Hz).
=> **1 timer tick = 1/50 s = 20 ms** (divisor 23863 -> 50.0003 Hz).

### INT 8 install (inside video/mem init 0xa5d5)
`old8 = getvect(8)` -> DS:fd69/fd6b ; `setvect(8, 1000:3dc5)` ; `set_timer_rate([dd])`.
INT 9 installed right after: `kbd_install(&DS:2200 /*key table*/, &DS:00dc /*last key*/)` (0xc3d0).

### INT 8 handler @0x3dc5 (runs at 50 Hz)
```
call old8                        ; QUIRK: old handler is chained TWICE per tick (start and end),
                                 ;   so BIOS time-of-day runs 100 counts/s (and whatever is behind, e.g. SBFMDRV)
if byte[97] != 1: read_mouse(&fd4a /*x*/, &fd4c /*y*/)      ; 0x710; [97]=1 while main loop draws
--- mouse cursor state machine, fd46 = request/state, fd48 = previous state, fd40 = 2 -> cursor hidden mode
if fd46 in {2,4}:   restore_rect(fd4e,fd50,w=fd52,h=fd54+1, from work buf 3792 to VRAM 37a6) (0x4d5)
                    fd4e=fd4a; fd50=fd4c; fd46=fd48=3          ; 3 = hidden (callers set 2 and spin until !=2)
elif fd46 != 3 and (pos changed (fd4e!=fd4a||fd50!=fd4c) or fd48!=fd46 or fd46==5):
    if fd40 != 2 or fd46 == 5: restore_rect(fd4e,fd50,fd56,fd58+1)  (old cursor size)
    if fd46 == 5: fd46 = 0                                      ; 5 = "redraw/show" request
    fd4e,fd50 = fd4a,fd4c ; fd56,fd58 = fd52,fd54
    if fd40 != 2: draw_cursor(src buf 379e at (fd42,fd44), size fd52 x fd54, colour0 transparent, to VRAM at (fd4a,fd4c)) (0xcae)
    fd48 = fd46
--- timers (only when byte[98]==0; [98]=1 while MENU / some blocking ops)
if u32[118c] > 0: u32[118c]--          ; FLI frame-delay timer
if u32[1190] > 0: u32[1190]--          ; FLI total-time timer (PLAYFLI com3*dd)
if w[fca9]==1 and (i16)w[fa2e] > 0: w[fa2e]--   ; text-on-screen timer
if b[fccf] > 0: b[fccf]--              ; player-step timer
if b[fcd0] > 0: b[fcd0]--              ; ANIMATION timer
--- always
if w[fcdc] > 0: w[fcdc]--              ; generic "seconds" wait (SHOWPCX time, startup nag)
--- CMF looping
if b[3785]==1 /*sound*/ and w[3788]==0 /*SBFMDRV status: stopped*/ and b[3784]==1 /*loop flag*/:
    fm_call(6) /*restart music*/ ; set_timer_rate([dd])
call old8 ; iret
```

### Keyboard INT 9 handler @0xc42d (does NOT chain to BIOS; no BIOS key buffer, Ctrl-Break etc. inactive)
```
al = in(0x60)
if (signed)al >= 0: *(u16*)(DS:2200 + al) = 1 ; byte[DS:dc] = al      ; make code; NB word write at byte offset
else:               *(u16*)(DS:2200 + al-0x80) = 0                   ; break code (0xE0 prefix => "release of 0x60")
out 0x20,0x20; iret
```
DS:2200[sc] = key-held flags (byte, but written as words: writing sc also clears sc+1's flag byte – quirk; sc 0x7f
spills into DS:2280 palette byte 0). DS:dc = last pressed scancode (= variable 25). Engine code polls `[dc]!=0`
and clears it to 0 to "consume" a key; Esc = 1, Space = 0x39, 'S' = 0x1f.
Main loop exit: loop continues while `!(key[0x2e] && key[0x1d])` -> **Ctrl+C held = quit** (0x6580) ->
main calls fatal(0) (no fade).

### Main-loop pacing (what the loop waits on)
The main loop (0x5690..0x659f) has **no tick wait**. Its only pacing is in the screen-update part (0x5e3c):
```
[97]=0
if b[fcd3]==1: set_palette(DS:2580) /*0xb7f: waits 1 vretrace before each 384-byte half*/ ; fcd3=0
else:          wait_vretrace() ; wait_vretrace()        ; 0xbed: wait for start of vertical retrace (3DA bit3)
```
=> one main-loop iteration = 2 VGA retraces = 2/70.086 s ~= **28.5 ms (35 Hz)** on a fast enough CPU.
The current ACTION interpreter runs once per loop iteration (0x7937 called unconditionally), so **one ACTION
yield (RET/GOXY/SPEAK.. or DELAY step) ~= 1 loop ~= 28.5 ms**. ANIMATIONs and player steps are additionally
gated by the 20 ms timers below, i.e. quantised to loop iterations.

### SETUP.CFG (6 bytes; repo: 02 04 04 00 01 01, DEMO: 02 02 03 00 01 01)
main 0xb78c:
* `RUNDAT SETUP` (argv[1]=="SETUP", strcmp): open SETUP.CFG, read byte0, exit(1) if 1, exit(2) if 2, else exit(0).
  Used by DEMO.BAT as ERRORLEVEL: 1 -> "SBFMDRV /U" then RUNDAT (=> no FM driver => no sound),
  2 -> load SBFMDRV, RUNDAT, unload. (0 or >=3 -> batch runs nothing.) So byte0 = sound choice (1 none, 2 SB).
* normal run: open SETUP.CFG (missing -> fatal(2000): message + exit, before any vectors are hooked),
  byte0 read & ignored, byte1 = game speed, byte2 = text speed; bytes 3..5 never read by RUNDAT (unknown,
  probably SETUP.EXE's own settings – SB port/IRQ/DMA? not passed to CT-VOICE, which therefore uses its defaults).

| byte1 | DS:8a22 (player-step ticks) | DS:8a23 (animation ticks) |
|---|---|---|
| 0 | 15 | 10 |
| 1 | 8 | 5 |
| 2 | 4 | 2 |
| 3 | 3 | 1 |
| 4 | 1 | 0 |
other values: left at 0 (static init) – not set.

| byte2 | DS:8a24 (text ticks per character) |
|---|---|
| 0 | 16 |
| 1 | 12 |
| 2 | 8 |
| 3 | 4 |
| 4 | 2 |

Usage:
* ANIMATIONs (0x7f6f, once per main loop): `if b[fcd0]==0 { run all active animations; fcd0 = [8a23] }`.
  So animations step every max(1 loop, [8a23]*20 ms rounded up to the next loop): speed 3/4 -> every loop (~28.5 ms),
  2 -> ~57 ms, 1 -> ~114 ms, 0 -> ~200-228 ms.
* Player walking (0x4b5c): a step is taken only when `[fbf4]==1 && [fccf]==0`; after the step
  `fccf = (w[fcbf]==0) ? [8a22] : 2*[8a22]` (fcbf != 0 seems to be the PMOV-substituted-sprite mode; uncertain).
* Text (0x7588, end of text layout 0x726d): `fa2e = [8a24] * (nchars < 10 ? nchars+2 : nchars)` ticks of 20 ms;
  when fa2e<=0 and fca9==1 the text lines are cleared (0x788d) and the page counter fcc1++ (0x7877).

### FREC
FREC is documented in the manual but **does not exist** in ED.DAT's command table nor as a string in any
binary; no interpreter op implements it. Unimplemented / cannot be compiled. Speed only comes from SETUP.CFG.

### DELAY(n, counter) op 65 @0x3a0f (timing quirk)
```
code[pc+2]++                        ; counter stored in the bytecode itself
if code[pc+2] < n: return (u8)code[pc+2]   ; yield WITHOUT advancing pc (counter&0xff as return code!)
code[pc+2] = 0 ; pc += 3 ; continue executing (no yield)
```
=> DELAY(n) costs n-1 yields (n-1 loop iterations for an ACTION; n-1 animation steps inside an ANIMATION).
Quirk: return value is the counter's low byte: counter 256,512.. -> returns 0 = "script finished";
counter 2 -> return 2 (= error code value for the caller?) – how callers treat 2 belongs to the caller analysis
(the ANIMATION caller 0x7f6f only tests `al==0`, so for animations anything !=0 = continue).

### Other second-based waits
`fcdc = secs * [dd]` then spin while fcdc>0 (SHOWPCX modes 1..3 @0x32d9.., startup nag 6 s). Fades
(0xa4b fade-to-black, 0xac5 fade-to-palette: 64 steps, each = one DAC update + `loop 0x7d00` busy delay, no vsync)
are CPU-speed dependent.

## 2. PLAYFLI(file, mode, secs) – op 62 @0x3997 -> 0x0e4c(name=FILE[file].name (DS:442e+13*file), mode, secs)

```
fd46 = 2; while fd46==2 {}                      ; hide mouse cursor (ISR -> 3)
clear_rect(0,0,320,200 -> VRAM)                 ; 0x450: fills with colour 0 (palette untouched)
strcpy(DS:219c, name)
for i in 0..19: if strcmp(name, FILE[i].name)==0: { dec=5; fp=fopen(DS:1d80 /*EDGAME.DAT*/,"rb") (fail->fatal 0x68);
                                                     fseek(fp, FILE[i].offset (DS:435a+8i)); break }
if dec != 5: fatal(0x67)                         ; QUIRK: message talks about "graficos(0)" file
read(DS:110c, 128 bytes, fp, sub=5)             ; 0x12b2: every byte -5
if w[110c+4] != 0xAF11: fatal(0x33) ("no es un .FLI")   ; FLC (0xAF12) rejected; size/width/height/depth/flags not checked
start = ftell(fp)                                ; = file start + 128 (first frame)
spd = w[110c+0x10]                               ; FLI speed in 1/70 s
t = trunc(spd * [dd] / 70.0)  (float const DS:e3 = 70.0)
if t < spd: t++                                  ; QUIRK: compares with spd, not with the exact quotient -> for spd>0
                                                 ;   always t = trunc(spd*50/70)+1  (spd 5 -> 4 ticks = 80 ms)
w[111c] = t
u32[1190] = secs * [dd]                          ; total-time timer (cdq quirk: >=656 s overflows)
[dc] = 0
loop_anim:
  for frame in 0 .. frames-1 (w[110c+6]):        ; ring frame not special; replay restarts at 'start'
     if mode in {3,6,10} and [dc]==0x39: goto done      ; SPACE
     if mode in {2,5,9}  and [dc]==1:    goto done      ; ESC
     if [dc]!=0 and mode in {1,4,8}:     goto done      ; any key
     [dc] = 0                                    ; any other key is swallowed each frame
     read(DS:10fc, 16, fp, 5)                    ; frame header: u32 size, u16 magic (NOT checked), u16 nchunks (DS:1102)
     while u32[118c] > 0 {}                      ; wait previous frame's delay (first frame immediate)
     for c in 0..nchunks-1:
        read(DS:10f6, 6, fp, 5)                  ; u32 csize, u16 ctype (DS:10fa)
        read(work buf 3792, csize-6, fp, 5)      ; no bound check (buffer 64000)
        ctype 4  -> COLOR256 (0x15b3)  values >>2 (/4) to DAC
        ctype 11 -> COLOR    (0x1546)  values as-is (6-bit) to DAC
        ctype 12 -> LC       (0x12e7)  -> VRAM
        ctype 13 -> BLACK    (0x151c)  -> VRAM   BUG: uses uninitialised locals as w/h
        ctype 15 -> BRUN     (0x143f)  -> VRAM
        ctype 16 -> COPY     (0x14f0)  -> VRAM   BUG: uses uninitialised locals as w/h
        other    -> ignored (data already consumed)
     u32[118c] = w[111c]
  fseek(fp, start)
  if mode == 11: while u32[1190] > 0 {}          ; "hold last frame" = wait until secs since START of PLAYFLI
  if mode in {7,8,9,10} and u32[1190] > 0: goto loop_anim   ; time only checked after a complete pass
  if mode in {4,5,6}: goto loop_anim
done:
  fclose; clear VRAM to 0; 0x65a0 (?) ; set_palette(DS:2580) (immediate, 0xb7f); compose full screen (0x4149);
  fd46 = 5 (show cursor); [dc] = 0 ; interpreter: pc += 4, continue (no yield)
```
Mode summary: 0 once; 1/2/3 once, abort on any/ESC/SPACE; 4/5/6 loop until any/ESC/SPACE; 7 loop until time;
8/9/10 loop until time or any/ESC/SPACE (key checked per frame, time per pass); 11 play once then wait until
`secs` have elapsed since start. Modes >11 behave like 0.

Chunk decoders (all write straight to VGA A000:0, stride 320; palette writes go only to the DAC – the engine
palette buffers 2280/2580 are untouched and 2580 is re-applied at the end):
* COLOR (11) / COLOR256 (4): `npackets = low byte of u16` ; idx = 0; per packet: `idx += skip` ;
  `n = count ? count : 256` ; out 3C8=idx; write 3n bytes. **BUG**: idx is not advanced by n, so with >1 packet
  later packets start at (sum of skips) instead of skip+previous counts. A full 256-colour chunk (1 packet) is fine.
* LC (12): u16 skiplines; u16 nlines; per line: u8 npackets; per packet: u8 xskip, s8 size: >0 copy size bytes,
  <0 replicate next byte -size times. Line stride = header width (DS:1114). After nlines, if bytes consumed <
  csize-7 it reads another (skip,nlines) header and continues from the current dest (non-standard tolerance);
  inner loop also aborts when consumed >= csize-7.
* BRUN (15): for header height (DS:1116) lines: u8 npackets (used as packet count); s8 n: >=0 replicate next byte n
  times, <0 copy -n bytes. Stride = header width.
* BLACK (13)/COPY (16): effectively broken (garbage size from uninitialised stack). Implement as standard
  (clear 64000 / copy 64000) – originals probably never contain them except COPY in rare files.
Timer 118c/1190 only decrement while [98]==0.

## 3. FIRSTFRAME(def, thing) op 63 @0x39ba / NEXTFRAME(def, thing) op 64 @0x39e6
DEFFRAMES at DS:1194 + 0x34*def: +1..+0x32 items (u8 COORANIM ids, 0xff end) [DS:1195], +0x33 cur [DS:11c7].
```
FIRSTFRAME: cur = 0;               NEWTHING(thing, items[cur])
NEXTFRAME:  cur++; if items[cur]==0xff: cur = 0;   NEWTHING(thing, items[cur])
```
(both jump into the NEWTHING handler tail at 0x31ef; no bounds check if no 0xff terminator).

## 4. Sound

### Detection / init (0xa5d5)
* `fm_detect(&DS:3788)` @0xc4ff: scan INT 0x80..0xBF, vector segment must contain "FMDRV" at offset 0x103
  (SBFMDRV signature). Found -> entry far ptr saved (cs:c492), status-byte ptr = DS:3788, returns 0.
  `b[3785] = found` (1 = sound enabled). Not found -> no sound at all (no VOC either).
* If found: `[3784]=0`; malloc(40000) -> CMF buffer DS:378e/3790, malloc(40000) -> VOC buffer DS:378a/378c;
  malloc(5000), paragraph-align, load file "CT-VOICE.DRV" (cwd, raw, 0xc1ef) – missing -> fatal(0x71);
  `ctv_init(driver, &DS:3786)` @0xc496: CT-VOICE fn 3 (init; result ignored), if ok fn 5 (status word = DS:3786).
  No base-port/IRQ calls (fn 1/2) – driver defaults.

Driver call wrappers:
* `fm_call(fn)` @0xc5c2 (SBFMDRV, called as pushf/cli/far call to its INT handler, BX=fn):
  fn 6 -> `fm_play()` @0xc562; fn 7 -> calls fn 7 (stop) then fn 8 (reset); others -> fn as is.
* `fm_play()`: only if status byte [3788]==0: fn 1 (status addr = DS:3788); fn 2 (instruments: DX:AX = cmf+w[cmf+6],
  CX = w[cmf+0x24]); fn 4 (driver clock: AX = table[(b[cmf+0xC] & 0xF0)>>4] where table @cs:c46a =
  {FFFF,FFFF,91A6,6119,48D3,3A42,308C,299D,2469,205D,1D21,1A76,1846,1668,14CE,136B} = 1193180/(16*k) Hz, i.e.
  CMF ticks/s rounded down to a multiple of 16, only low byte of the field used, <32 -> 18.2 Hz);
  fn 6 (play, DX:AX = cmf + w[cmf+8]).
* `ctv_call(fn)` @0xc4de (CT-VOICE, BX=fn, ES:DI = VOC buffer; for fn 6 DI += w[buf+0x14] i.e. skip VOC header).

### PLAYCMF(file) op 36 @0x353f -> 0x3ce8(off,size of FILE[file])
```
if (off,size) == (fd18,fd1c) and [fca6]==1: return             ; same tune already playing: keep playing
if sound: loadfile(off,size -> CMF buf, -5 decode) ; fail -> strcpy(219c, DS:0121 /*empty string! bug*/); fatal(0x68)
fd18,fd1c = off,size ; fc9e,fca2 = off,size (saved in savegame)
[3784] = 0
if sound: fm_call(8) /*reset*/ ; set music ptr (0xc54c) ; fm_call(6) /*play*/ ; set_timer_rate([dd])
[3784] = 1 /*loop*/ ; w[4338] = 1 ; [fca6] = 1
```
Music **loops forever**: the INT 8 handler restarts it (fm_call(6) + PIT reset) whenever status [3788]==0.
STOPCMF op 37 @0x3564: `if sound { fm_call(8); set_timer_rate(dd) } ; [3784]=0; [fca6]=0; w[4338]=-1`.
Savegame load (0x9354): stop as above, then if saved fca6==1 -> PLAYCMF(fc9e,fca2).
Tempo caveat (UNCERTAIN): the engine re-programs the PIT to 50 Hz immediately after SBFMDRV fn 6 (which
normally sets channel 0 to the music clock) and it calls the previous INT 8 vector (SBFMDRV's, if it hooked INT 8
before RUNDAT started) twice per 20 ms tick. The real playback rate on DOS therefore depends on SBFMDRV internals;
a reimplementation should play the CMF at its own ticks/s (field +0x0C) and keep the game at 50 Hz.

### GETVOC(file) op 40 @0x35ef
`if !sound return; ctv_call(8) /*stop*/; loadfile(FILE[file] -> VOC buf (40000, no size check), -5)`
fail -> strcpy(219c, FILE name); fatal(0x68); `ctv_setbuf(VOC buf)` (0xc4c8).
### PLAYVOC() op 38 @0x358c
`if sound: ctv_call(6)` (output voice from buf + w[buf+0x14]). One-shot, no loop, no wait, status word DS:3786
never read by the engine.

## 5. EXIT, fatal(), shareware

### EXIT() op 41 @0x365c
`fade_to_black()` (0xa4b) ; `fatal(0)`.

### fatal(code) @0xa836 (also the normal exit path)
```
set video mode 3 (int10 ah=0) ; gotoxy(1,1)
printf("ED PACK v2.0                                    Por Jose Luis Serrano Moreno\n")
printf("RUNDAT.EXE v1.0"); printf("\nVersion SHAREWARE"); printf("\n\n")
if code == 2000: print SETUP.CFG-not-found text; exit(0)             ; vectors not yet hooked
if code == -1000 (0xfc18): exit(0)            ; no caller found
if sound: [3784]=0; fm_call(8); fm_call(9); fm_call(7) (=fn7+fn8); ctv_call(8); ctv_call(9)
set_timer_rate(0) ; setvect(8, old8) ; restore INT 9 (0xc415) ; 0x631 (mouse reset/hide) ; free 5 gfx buffers
print message for code (0 -> none) ; 0xe893 (flush/close all) ; printf("\n\n") ; exit(0)
```
So the end of the game (EXIT, Ctrl+C, MENU "Salir" + 'S' at 0x9fd3) shows only the text-mode banner
"ED PACK v2.0 ... / RUNDAT.EXE v1.0 / Version SHAREWARE". There is **no graphic nag at exit**.
Messages relevant here: 0x34 ">50 commands without RET" (DS:503), 0x67 gfx(0) open error, 0x68 file open error,
0x33 not a FLI, 0x71 CT-VOICE.DRV missing, 0xc8 shareware limit (DS:a33):
"Esta es una version SHAREWARE, la cual esta limitada a los niveles 0,1,2,3,4 y 5\nPara conseguir la version
COMPLETA lea el archivo PEDIDO.TXT".

### Startup shareware screen (main 0xbb0e..0xbc33, after init, before level 0)
```
fd46 = 3 ; fd40 = 2                                    ; cursor hidden
clear_rect(0,0,320,200 on VRAM) to colour 0            ; palette is black at this point (init faded it out)
for (y, s) in [(60,"Esta aventura ha sido creada con"), (73,"la version SHAREWARE del EDPACK v2.0"),
               (95,"Para conseguir la version completa"), (108,"mira el archivo PEDIDO.TXT")]:
    draw_text(x = 160 - text_width(s)/2, y, s, colour 15, font DS:3780, -> VRAM)   ; 0x78a8 width, 0x70d0 draw
set_palette(DS:3480)                                   ; = default VGA palette captured after mode 13h (0xbb3)
fcdc = [dd]*6 ; [dc]=0 ; while [dc]==0 and fcdc>0 {}   ; 6 s or any key
black_palette() (0xb68) ; [dc]=0 ; fd40=0 ; fd46=0
```

### Shareware level limit
`[fc9c]` (current level, byte) is checked with **signed** byte compares at ~17 places
(load level 0x1b69 `>5`, GOLEVEL op 1 handler 0x2e86 `>=6` right after storing fc9c/fcd1(door)/fcd2(mode),
main loop several times `>=6`/`>5`, text and misc routines). Any level 6..127 -> fatal(0xc8) (text mode +
message above). Levels 128..255 are negative and pass the check (quirk; they'd index beyond the DAT table).
Only levels 0..5 are reachable.
