# RUNVGA.EXE v1.03 (EDPACK v1): data formats, loading, globals, savegames

Source: RUNVGA.EXE (Sep 1996, "RUNVGA.EXE v1.03 Por Jose Luis Serrano Moreno"), unpacked with the
emulator stub-runner; Borland C++ 1991, one near code segment, far data. DGROUP = image paragraph
0x0fec (linear 0xfec0 in the unpacked image; in the reference emulator DS linear = (0x1000+0xfec)*16).
All addresses below are DS offsets unless written as code addresses (0x....: in code).
Checked against "Las Aventuras de Manolo Kabezabolo" (RESOURCE.DAT, 3,825,124 bytes) and against
DS dumps from the unicorn reference emulator after the start level (35) loaded.

Library routines used by everything below: fopen 0xeeee, fclose 0xeb7e, fread 0xf013,
fwrite 0xf1f5, fseek 0xf123, ftell 0xf19f, fgetc 0xf346, fputc 0xf670, strcpy 0xfc0d,
strcat 0xfbce, strcmp 0xfc36, strncpy 0xfc96, fcloseall 0xeab3, farmalloc 0xe0ac,
fatal(code) 0xb9c9 (table at the end).

## 1. Command line, file modes

`RUNVGA SETUP` -> reads SETUP.CFG byte 0 and exits with it as errorlevel (0/1/2) (used by the batch file).
`RUNVGA -f<file>` (single archive, byte [0x99]=1) or `RUNVGA -n<base>` (separate files, [0x99]=0);
argv[1] is strncpy'd (40 chars) to 0x9460 and the 2-char switch removed, so 0x9460 = archive name
(-f) or base name (-n). Anything else -> fatal 4 (usage). Manolo uses `RUNVGA -fRESOURCE.DAT`.

-n mode file names (base = first 14 chars of 0x9460):
* level n: base + ".nnn" (3 decimal digits, template ".000" at 0xad) e.g. NIVEL.035
* font: base + ".998"  (whole file read: 0x32cd bytes into 0xab24)
* inventory: base + ".999"
* resources (PCX/CMF/VOC): opened by their own name (as stored in the level's name tables).
Level loader for -n is 0x1895, for -f is 0x1e23; both are identical except that -f seeks to the
table entry first and reads the two trailing offset tables (FILE_TAB/GFX_TAB).

## 2. RESOURCE.DAT (-f archive)

* Offset 0: u32 table[1000] (4000 bytes). table[n] = absolute file offset of entry n, 0 = unused.
  The loader seeks to 4*n, reads the u32, fatal 0x65 ("No encuentro el nivel '%d'") if 0.
* Entry n < 998: level n. Entry 998: font (0x32cd bytes). Entry 999: inventory records.
* **No obfuscation of resources** (unlike v2's +5): PCX/CMF/VOC bytes are stored verbatim.
  All 136 FILE entries of Manolo have valid magic (0x0A / "CTMF" / "Creative Voice File").
* A level entry = the fixed level tables (0x7000 = 28672 bytes, §3) followed by resource files.
  FILE_TAB/GFX_TAB hold **absolute** archive offsets and sizes; identical files are stored once and
  shared by several levels (e.g. level 35's `manoanim.pcx` points into level 0's area).

Manolo's 27 used entries:
levels 0-10, 12-21, 30, 32, 35, 40 (25 levels), 998 = font at 3,805,959 (13,005 bytes),
999 = inventory at 3,818,964 (6,160 bytes = 22 records). Level 0 = setup level (gfx PORTADA/dibujos/
manoanim/OBJETOS, intro pictures soft*.pcx, VOCs, dwuh.cmf; its ACTION 0 plays the intro and
GOLEVELs to 35, the title/menu screen "select.pcx"). Text dump: `fmt_level.py <n>`.

## 3. Level file layout (offsets relative to the level start) and DS destinations

All raw fread's (no RLE, unlike v2). Total fixed part = 0x7000 bytes when both optional blocks present
(block sizes do not change: absent blocks are skipped with fseek, so the layout is fixed).

| off | size | dest | v1 name | v2 equivalent |
|---|---|---|---|---|
| 0x0000 | 0x294 (660) | 0x30fe | LIMITMOV: 30 x 22 | LIMITMOV (v2: 320 x 22) |
| 0x0294 | 0x1a4 (420) | 0x3392 | LIMIT: 30 x 14 | LIMIT (v2: 40 x 14) |
| 0x0438 | 0x816 (2070) | 0x39a5 | THINGS: 30 x 0x45 | THINGS (v2 30 x 92) |
| 0x0c4e | 0xc8 (200) | 0x2de4 | DOORS: 20 x 10 | DOORS |
| 0x0d16 | 6 x 250 | temp -> 0x41bb+250i | messages 0..5, decoded at once (c-0x0f); copied with strcpy **only if non-empty** (empty keeps the previous level's message) | MESSAGES (v2: 5) |
| 0x12f2 | 44 x 250 | 0x41bb+250*6.. | texts 6..49 (raw; decoded after load; right after the 6 messages: 0xd16 + 6*250) | TEXT pool (v2: 25000-byte pool + offsets) |
| 0x3dea | 10000 | far [0x2f10] | bytecode, 5000 words | code |
| 0x64fa | 100 | 0x2eac | ACTION offsets: 50 x i16 (word offsets into code; action 0 = level script) | ACTION_OFFS |
| 0x655e | 500 | 0x94b0 | ANIMS: 50 x 10 | ANIMS |
| 0x6752 | 52 | 0xa48 | GFX names 4 x char[13] (key 0x2d) | GFX_NAMES |
| 0x6786 | 240 | 0x300e | ACINVTH: 30 x 8 | ACINVTH |
| 0x6876 | 800 | 0xb80 | COORANIM: 100 x {i16 x,y,w,h} rects in gfx 1 | COORANIM (v2 10-byte) |
| 0x6b96 | 260 | 0xa7c | FILE names 20 x char[13] (key 0x19) | FILE_NAMES |
| 0x6c9a | 50 x 5 | 0x2f14 | ACINVINV (per level!): per record: u8 flag; if flag != 0: flag stored at +0 and 4 more bytes read to +1..+4; if 0: fseek +4 (record left as is) | global NIVEL.IAC in v2 |
| 0x6d94 | 2 | 0x980 | i16 player sprite width (stored only if >= 0) | — |
| 0x6d96 | 2 | 0x982 | i16 player sprite height (stored only if >= 0) | — |
| 0x6d98 | 1+11+400 | 0x9a.. / 0x96b0 | optional PLAYER-ANIM block (below) | MOV file |
| 0x6f34 | 12 | 0x96a4.. | optional MOUSE-CURSOR block (below) | MOU file |
| 0x6f40 | 160 | 0x9a8 | FILE_TAB 20 x {u32 off, u32 size} (-f only) | FILE_TAB |
| 0x6fe0 | 32 | 0x988 | GFX_TAB 4 x {u32 off, u32 size} (-f only) | GFX_TAB |

(0x7000 = end.) Then fclose, 0xc3a8 (string decryption), and if sound present ([0xddf2]==1):
[0xddf1]=0, SBFMDRV fn 8 (0xc644(8)), reprogram PIT with [0x97] (0x80c); [0x984] = -1 (no music
file playing). Returns 1.

### Optional PLAYER-ANIM block (0x6d98)
1 byte is fread into the low byte of a local word whose **high byte is still the high byte of the
previous 2-byte read (the 0x982 value)**; the block is present iff that word >= 0, i.e. effectively
iff the stored-or-not value read for 0x982 is >= 0 (high byte < 0x80). (Quirk; Manolo always pairs
"-1,-1 / 0xff" or "w,h / data".) If present: that byte -> [0x9a]; then 11 single bytes to, in this
order, [0x9b],[0x9c],[0x9e],[0x9f],[0x9d],[0xa1],[0xa2],[0xa3],[0xa4],[0xa5],[0xa6]
(note 0x9d after 0x9f; [0xa0] is never loaded from a level); then 400 bytes -> 0x96b0.
If absent: fseek +0x19b (411 = 11+400). Previous values stay (they persist from level 0).
* 0x96b0: 100 x {i16 srcx, i16 srcy}: frame source positions in gfx 2 ("MOVIMIENTOS" sprite sheet);
  all frames are [0x980] x [0x982] pixels. (-100,-100) terminates a sequence; (-1,-1) unused.
* [0x9a],[0x9b],[0x9c]: first frame index of the walk sequences used by door directions
  3, 2, and 0/1 (0 = mirrored, [0x91f7]=1); [0x9d],[0x9e],[0x9f]: start frames of further sequences
  (talking, used by the player updater 0x40xx/0x44xx), [0xa1..0xa6]: per-sequence parameters
  (lengths / delays). Exact semantics: see the movement spec. Manolo level 0:
  `00 06 0c 16 1a 1e 05 06 0a 04 04 04` with frames 0-4,6-10,12-20,22-24,26-28,30-32, size 32x50.
  Level 17 overrides size to 8x12 with its own frames.

### Optional MOUSE-CURSOR block (0x6f34)
i16 flag (= first field); if >= 0: 0x96a4 = flag, then 5 more i16 -> 0x96a6, 0x96a8, 0x96aa,
0x96ac, 0x96ae; copied to the live cursor: 0x92ac=srcx, 0x92ae=srcy (rect in gfx 1 "dibujos"),
0x92bc=w, 0x92be=h, 0x92c4/0x92c6=hotspot x/y; [0x92b8] = [0x92b4]-1; mouse range set to
x 0..319-w (int33 fn 7) and y 0..200-h (fn 8). If < 0: fseek +10. Manolo: (300,37,11,11,2,2).

### Level change (0x11c9) — the parts that touch formats
* [0x92b0]=2; -n: build file name (above).
* Saves gfx names 2,3 (0xa62,0xa6f) and GFX_TAB entries 2,3 (0x998..0x9a6), loads the level, restores
  them: **gfx 2 (player sheet) and gfx 3 (inventory sheet) are global, set only by level 0**.
* If the loaded level is 0: 0x17de runs ACTION 0 (script at [0x2eac]) to completion, then requires
  gfx2 name (fatal 0x6a "MOVIMIENTOS no especificado") and gfx3 name (fatal 0x6b "INVENTARIO no
  especificado"), loads gfx2 -> far [0xde0b] (pal 0xa824) and gfx3 -> [0xde03] (pal 0xa524);
  then if [0x927c] (level) is still 0 -> fatal 0x69 "Nivel de comienzo no especificado", else restarts
  the level change (so level 0's script must GOLEVEL to the start level).
* Things: for all 30, copies POS x,y -> prev pos (+0x10->+0x14, +0x12->+0x16) and w,h -> prev (+0x18->+0x1c,
  +0x1a->+0x1e).
* Loads gfx 1 -> far [0xde07] (pal 0xa224, fatal 0x66) and gfx 0 -> far [0xde0f] (pal 0x9f24,
  fatal 0x67) through 0xa3e.
* 0x162d builds the scene palette 0x9924 (below), 0xccab (?), runs ACTION 0 until it returns 0,
  then places the player from door [0x92a3] (DOORS record) — see main-loop spec.

## 4. Records

### THING (0x45 = 69 bytes, base 0x39a5 + 0x45*t, t = 0..29)
| off | type | meaning (v1) | script var (30+20t+k) |
|---|---|---|---|
| +0x00 | i16 | COOR src x in gfx1 (320,200 = no graphic) | — |
| +0x02 | i16 | COOR src y | — |
| +0x04/+0x06 | i16 | previous src x/y (redraw tracking, main loop 0x50ce) | — |
| +0x0c | i16 | layer / draw class (values 1..3) | k=1 |
| +0x10 | i16 | POS x | k=5 |
| +0x12 | i16 | POS y | k=6 |
| +0x14/+0x16 | i16 | previous POS x/y (set at level load) | — |
| +0x18 | i16 | width | — |
| +0x1a | i16 | height | — |
| +0x1c/+0x1e | i16 | previous w/h | — |
| +0x20 | i16 | previous mode | — |
| +0x22 | i16 | mode / visibility (v2 "+2b") | k=0 |
| +0x24 | u8 (var writes a word) | mirror | k=7 |
| +0x26 | char[15] | name, stored c+0x0d, NUL-terminated; decrypted at load **only for things 0..24** | — |
| +0x35 | i16 | GOTO x (-100 = none) | k=8 |
| +0x37 | i16 | GOTO y | k=9 |
| +0x39 | i16 | GOTO dir | k=10 |
| +0x3b | i16 | ? (0 in Manolo) | k=11 |
| +0x3d | i16 | "look" text number (used by 0x78e1) | k=4 |
| +0x3f | i16 | use type: 0 -> action 100 pc 0, 1 -> action 100 pc 2, 2 -> action 0x68, 3 -> action = +0x41 (0x7c99) | k=2 |
| +0x41 | i16 | action number for type 3 | k=3 |
| +0x43 | i16 | ? (0x0a/0x0f/0x12 in Manolo; pushed at 0x7752) | k=12 |

### DOOR (10 bytes at 0x2de4 + 10*d, d = 0..19)
{i16 used, i16 x (-100 = keep), i16 y (-100 = keep), i16 dir (-100 = keep; 0 side mirrored,
1 side, 2 [0x9b], 3 [0x9a]), u8 action to run on arrival (-> [0x929c]), u8 pad}.
Player placed at x - w/2, y - h (w,h = [0x91e9],[0x91eb]).

### LIMIT (14 bytes at 0x3392 + 14*i, i = 0..29) — used by GOXY 0x7dab
{i16 used, x1, y1, x2, y2, newx, newy}: if x1 <= tx <= x2 and y1 <= ty <= y2 the target becomes
(newx,newy); -100 = keep that coordinate, -101 = player's current foot coordinate.

### LIMITMOV (22 bytes at 0x30fe + 22*i, i = 0..29)
{i16 used, tx1, ty1, tx2, ty2 (target rect), px1, py1, px2, py2 (current-position rect),
wx, wy (waypoint)}; GOXY repeatedly takes the first matching rule, appends (wx,wy) to the path at
0x9204 and restarts the scan from that waypoint.

### ACINVTH (8 bytes at 0x300e + 8*i, i = 0..29) — 0x7c02
{i16 mode (0 unused, 1 -> action 0x66, 2 -> action 0x67), i16 thing, i16 inv object, u8 value,
u8 pad}; matched against [0x35a2] (thing) and [0x35a6] (object); the byte goes to [0x35a7].
No match: action 0x66 with [0x35a7] = 0.

### ACINVINV (5 bytes at 0x2f14 + 5*i, i = 0..49) — 0x7a9a
{u8 used, u8 obj1, u8 obj2, u8 result, u8 mode}: matches (obj1,obj2) in either order against
[0x35a2]/[0x35a6]; mode 0 removes both, 1 removes obj1, 2 removes obj2, other keeps both; then
ADDINV(result) and SPEAK(message 4); no match -> SPEAK(message 3). Per level (cleared only at
startup), not a global file as in v2.

### ANIM (10 bytes at 0x94b0 + 10*a, a = 0..49)
{i16 active, i16 script base (word offset into the level code), u8 pc (+4), u8 pad, u8 cmp flag
(+6), ...}; 0x7d26 runs every active one: run(base, &tmp, &+4, &+6); when the script ends (returns 0)
+4 (word) and +0 are cleared.

### COORANIM (8 bytes at 0xb80 + 8*i, i = 0..99)
{i16 x, y, w, h}: source rectangles in gfx 1 (dibujos.pcx), used by NEWTHING-like op 0x1f.

## 5. Strings and their keys (all: subtract key from each byte until NUL)
| data | key | when |
|---|---|---|
| level messages 0..5 | 0x0f | during load (temp buffer) |
| level texts 6..49 (0x41bb) | 0x0f | 0xc3a8 after load |
| thing names (+0x26, things 0..24 only) | 0x0d | 0xc3a8 |
| FILE names 0xa7c | 0x19 | 0xc3a8 |
| GFX names 0xa48 | 0x2d | 0xc3a8 |
| inventory text (999 record +0) | 0x1e | ADDINV |
| inventory name (999 record +0xfa) | 0x20 | ADDINV |
Same keys as v2. `^` is the line break in texts. Resource names are compared with strcmp (case
sensitive), FILE names first, then GFX names.

## 6. Embedded resources

### Lookup (-f, 0xc2e) / open (-n, 0xa3e)
A name is searched in FILE names (0xa7c, 20) then GFX names (0xa48, 4); not found -> fatal 0x63.
Offset/size from FILE_TAB 0x9a8+8i or GFX_TAB 0x988+8i. CMF/VOC: whole file read into a 30000-byte
far buffer ([0xddfb] CMF, [0xddf7] VOC) with 0xc31d (-f: opens the archive, seeks, fgetc*size) or
0xc2b1 (-n: reads the loose file to EOF); fatal 0x68 on failure. No size check (> 30000 overflows).
LOADPCX (op 0x21, gfx slot g, FILE n) copies FILE name n and FILE_TAB n into gfx slot g, then reloads.

### PCX (0xa3e / 0xc2e) — 320x200x8 only
byte0 must be 0x0a (else fatal 0x62 "no es un PCX"), byte 3 (bpp) must be 8 (else fatal 6), xmax+1
must be 320 and ymax+1 200 (fatal 6); header skipped to 128. Decoding: 200 rows x 320 counter;
byte > 0xC0 = run of (b & 0x3f) copies of next byte, the run is written completely even past the
row end (row counter goes negative, i.e. runs may cross rows, no clipping); then 1 byte (0x0C marker,
not checked) and 768 palette bytes (8-bit, stored as is). Destination = far 64000-byte buffer.

### Scene palette 0x9924 (0x162d)
Fill 768 bytes with 0xff, copy gfx0 palette (0x9f24) entirely, then for every colour still
(ff,ff,ff) take the first non-white of gfx3 pal (0xa524), else gfx1 pal (0xa224), else gfx2 pal
(0xa824); finally divide all 768 by 4 (8->6 bit). Palette 0x9c24 = full-screen picture (op 0x20),
divided by 4 by 0x17bc.

### Font (entry 998 / base.998), 0x32cd bytes at 0xab24
No header. 255 glyphs (chars 0..254) x 51 bytes: byte 0 = (w << 4) | h (w <= 9, h <= 10 in Manolo),
then 4 bits per pixel, high nibble first, continuous across rows (rows are not byte aligned); value
1 = text colour, 2 = border colour, others transparent. Glyph blit 0x910 (no clipping).
Advance = w - 1 (no separation byte; the 1-px gap is in the artwork); space = 0x51 (w 5, h 1).
DrawText 0x6d15. (v2 font: 2-byte header + RLE + 2 bits/pixel - different.)

### Inventory (entry 999 / base.999) — ADDINV 0x689a
Record obj (0x118 = 280 bytes) at entry offset + obj*0x118 (32-bit multiply here):
+0 text[250] (key 0x1e), +0xfa name[30] (key 0x20). Fatal 1 if the table entry is 0.
ADDINV(obj) takes the first free of 30 inventory slots:
slot s: 0x35a9 + 0x22*s = name[30], +0x1e (0x35c7) u8 used, +0x1f (0x35c8) u8 = 0,
+0x20 (0x35c9) u8 obj; text -> 0x728f + 250*s (= text slots 50..79 of the 0x41bb array).
The icons are not in the file: they come from gfx 3 (inventory PCX, OBJETOS.PCX) by object number
(see UI spec).

### Mouse cursor
Not a file: rectangle of gfx 1 given by the level's MOUSE-CURSOR block (0x92ac..), default from level 0.

## 7. Startup (main 0xbc17) and global initialisation
* [0x97] = 50 (timer Hz; PIT reprogrammed by 0x80c(hz): divisor 0x1234dc/hz). Never changed.
* SETUP.CFG (only 3 bytes read; byte 0 ignored outside `RUNVGA SETUP`; missing file = message + exit):
  byte1 speed 0..4 -> ([0x3536],[0x3537]) = (15,12),(8,6),(4,3),(3,2),(1,0);
  byte2 text speed 0..4 -> [0x3538] = 16,12,8,4,2. Other values leave the bytes 0.
* 0xb74d: mode 13h, mouse, sound detect (SBFMDRV int 0x80 -> [0xddf2]), timer ISR 0x36ee on int 8 at
  50 Hz, allocations: CMF buf [0xddfb] 30000, VOC buf [0xddf7] 30000, [0x976] 5000, CT-VOICE.DRV
  (fatal 0x71), 5 x 64000 screens: gfx0 [0xde0f], gfx2 [0xde0b], gfx1 [0xde07], gfx3 [0xde03],
  back buffer [0xddff]; screen [0xde13] = A000:0000; user vars [0xea0] 2000 bytes; code [0x2f10]
  10000 bytes.
* Font: -n "<base>.998", -f entry 998, 0x32cd bytes -> 0xab24 (fatal 2).
* Defaults: gfx names 0..3 empty, [0x984]=-1, [0x987]=10, [0x986]=4, [0x980]=32, [0x982]=50,
  [0x92a5]=1, [0x92a6]=15, level [0x927c]=0, previous level [0x927e]=-1, [0x9204]=[0x9206]=-100,
  [0x929f]=0, [0x3539]=2, [0x353b]=15, texts 0..79 emptied, ACINVINV cleared, user vars 0,
  [0x92b0]=3, mouse [0x92b4]=[0x92b6]=100, cursor from 0x96a4.. (still 0), [0x92b8]=[0x92ba]=0.
  Then 0xcdc8 and the main loop 0x5037.

### Script variable pointer table: 2000 far pointers at 0xea4 (var n at 0xea4+4n; ends at 0x2de4)
Unset entries are 0000:0000 (BSS) - reading/writing such a var touches interrupt vector 0
(divide-error vector) in real DOS; all unset vars alias the same word.
| var | DS addr | meaning (best guess) |
|---|---|---|
| 0 | 0x91f5 | player state word (tested ==0 / ==1 by the main loop) |
| 1 | 0x91da | player current frame (byte field; var write also sets 0x91db) |
| 2 | 0x91dd | player x (left) |
| 3 | 0x91df | player y (top) |
| 4 | 0x91e9 | player width |
| 5 | 0x91eb | player height |
| 6 | 0x91f7 | player mirror (byte) |
| 7 | 0x9282 | byte flag (text/speech in progress; SPEAK/PTEXT wait while != 0) |
| 8 | 0x92a5 | text colour (init 1) |
| 9 | 0x92a6 | text border colour (init 15) |
| 10 | 0x92b0 | cursor/mode word (3 at start, 2 during level change, 0 after) |
| 11 | 0x3539 | info-bar text colour (init 2) |
| 12 | 0x353b | info-bar border colour (init 15) |
| 13-29 | unset | |
| 30+20t+k, t=0..24, k=0..12 | thing t field, see THING table (k: 0 +0x22, 1 +0x0c, 2 +0x3f, 3 +0x41, 4 +0x3d, 5 +0x10, 6 +0x12, 7 +0x24, 8 +0x35, 9 +0x37, 10 +0x39, 11 +0x3b, 12 +0x43) |
| 30+20t+13..19, and things 25..29 (vars 530..999) | unset | |
| 1000-1999 | far [0xea0] + 2*(n-1000) | user vars, zeroed at startup, persist across levels |
Value arguments >= 25000 read var (arg-25000) through the same table; dest args are raw var numbers.

## 8. Savegames: "SAVEGAME._N_", N = slot+1 ('1'+slot), slots 0..? (name table 0x92d0 + 50*slot)
Write 0xa4f9 (fwrite) / read 0x9df3 (fread), same order:
1. u8 0x0a (fputc; ignored on load)
2. 50 bytes save name (0x92d0 + 50*slot)
3. 0xd3 bytes 0x91d9..0x92ab (player and core globals, includes level 0x927c)
4. 0xf0 0x300e ACINVTH; 0xf0 0x2f14 ACINVINV (only 0xf0 of its 250 bytes - records 48,49 not saved)
5. 0x320 0xb80 COORANIM; 0x294 0x30fe LIMITMOV; 0x1a4 0x3392 LIMIT; 0x34 0xa48 GFX names
6. 0x4e20 0x41bb texts (80 x 250: level 50 + inventory 30)
7. 10000 code (far [0x2f10]); 0x64 0x2eac ACTION offsets
8. 0x1c 0x92ac..0x92c7 (cursor + mouse state)
9. 0x816 THINGS; 0x3fc 0x35a9 inventory slots (30 x 0x22); 0x1f4 ANIMS; 0x104 FILE names
10. 0x1fe 0x8fdb..0x91d8 (speech lines etc.)
11. 0xa0 FILE_TAB; 0x20 GFX_TAB
12. palettes 0x300 each: 0x9924, 0xa824, 0xa524, 0xa224, 0x9f24, 0x9c24
13. 2000 x i16: the value of every script var through the pointer table (on load written back
    through the pointers; unset vars hit vector 0)
14. 2 bytes 0x984 (music file index), 0x190 0x96b0 frames, 4 bytes 0x980/0x982
15. 13 single bytes [0x9a]..[0xa6] (in address order, includes 0xa0)
16. 0xc 0x96a4..0x96af; 0x70 0x3539..0x35a8
Strings are stored shifted: texts -0x14, inventory names -0x16, gfx names -0x04, FILE names -0x0e,
speech lines (5 x 0x66 at 0x8fed) -0x36 (encrypted in place before writing, restored after; load
adds the key back). After loading: reload gfx 0,1,2,3 via 0xa3e (palettes 0x9f24, 0xa224, 0xa824,
0xa524), reload/restart CMF [0x984] if != -1 and sound present, 0xccab, 0x61df, 0x6c87, full redraw,
set palette 0x9924.

## 9. Fatal error codes (0xb9c9)
1 inventory file, 2 font file, 3 memory, 4 usage, 5 no mouse, 6 not 320x200x256, 0x62 not a PCX,
0x63 can't read '%s' inside '%s', 0x64 can't read '%s', 0x65 level not found, 0x66/0x67 gfx 1/0 not
found, 0x68 file not found, 0x69 start level not set in level 0, 0x6a MOVIMIENTOS not set,
0x6b INVENTARIO not set, 0x6c/0x6d gfx 2/3 not found, 0x6e gfx not found, 0x6f invalid command,
0x71 CT-VOICE.DRV not found. Message printed after the "RUNVGA.EXE v1.03" banner, then exit.

## Notes for the reference emulator
* The game sets the int 33h ranges itself (fn 7: x 0..319-w) and uses CX directly as a pixel x,
  so fn 3/5/6 must return x in 0..319 (dosemu.py currently returns mouse[0]*2; level 35 DS showed
  [0x92b4] = 320 for mouse x 160).
