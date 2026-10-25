# EDPACK v2.0 RUNDAT.EXE — Text, fonts and UI specification

All addresses: code = RUNDAT.bin linear (one 64 KB near code segment; near call targets wrap mod 0x10000,
so "call 0x10688" in a raw disassembly is really 0x0688). "DS:xxxx" = RUNDAT.bin[0x12a80+xxxx].
Screen = 320x200 mode 13h. Work buffer = far [DS:3792] (320x200); VGA = far [DS:37a6] (A000:0000).
Timer: INT 8 hooked (ISR 0x3dc5); PIT is reprogrammed to 1193180/[DS:dd] Hz, with [DS:dd]=100, so
**1 tick = 10 ms**. [DS:dc] (= var25) = last raw keyboard make scancode (INT 9 handler at 0xc42d).

Sections:
1. Fonts (.FNT) and the glyph renderer
2. SPEAK / PTEXT (speech text)
3. STEXT (dialogue choices)
4. Information bar
5. Mouse cursor, verbs, click handling, MMOUSE
6. Inventory -> see **spec_inv.md** (with a short summary here)
7. MENU / savegames -> see **spec_menu.md** (with a short summary here)
8. Variable defaults and the word-overlap quirks that matter for the UI

---------------------------------------------------------------------------------------------------
## 1. Font file (.FNT) — LETRAS1.FNT == EDGAME.DAT entry 7 (byte-for-byte copy, offset 0x57460)

Loader (main init, 0xba74..0bb08): it seeks to the entry-7 offset and reads:
```
u8  space_w      -> DS:fd66   (EDLETRAS "ESPACIO": width in pixels of ' '; LETRAS1 = 5)
u8  separation   -> DS:fd67   (EDLETRAS "SEPARACION"; LETRAS1 = 0)
RLE block, decompressed size 0x2200 = 256 glyphs * 34 bytes -> far [DS:3780]
```
The RLE decoder is 0x1709 (the same one used by levels). It reads a byte c. If c > 0xC0 then count = c & 0x3F
and the next byte is repeated count times. Otherwise c is a literal; **0xC0 itself is a literal**.
LETRAS1.FNT is fully used (3518 bytes = 2 + RLE).
There is no 30-byte header. NIVEL.FNT is stored raw (not +5 encrypted).

Glyph record (34 bytes, glyph index = character byte value, **code page 850**, no remapping):
```
byte 0      : (width << 4) | height     (width = hi nibble, height = lo nibble, 0..15)
bytes 1..33 : 2 bits per pixel, MSB first (pixel0 = bits 7-6, pixel1 = 5-4, ...), row-major,
              width pixels per row, NO padding at row ends (the bit stream continues across rows)
              value 0 = transparent, 1 = text colour, 2 = border colour, 3 = transparent
```
The outline is **baked into the glyph bitmaps**: border pixels have value 2 and are designed by hand
(it looks like an 8-neighbour/box outline around the strokes). The engine computes no outline.
Unused trailing bytes of the 34 are garbage from the editor; ignore them.
LETRAS1: most glyphs are 7x9 (the outline is included), M/W/m/w are 9x9, descenders g/j/p/q/y and ',' ';' are
x10, 'i' and '.' are 4 wide. Glyphs are top-aligned: every glyph is drawn starting at the line's y.
Defined characters in LETRAS1: 33-59, 61, 63-93, 95, 97-123, 125, and cp850 0x81 ü, 0x82 é,
0x84 ä, 0x85 à, 0x89 ë, 0x8A è, 0x8B ï, 0x8D ì, 0x94 ö, 0x95 ò, 0x97 ù, 0xA0 á, 0xA1 í, 0xA2 ó, 0xA3 ú,
0xA4 ñ, 0xA5 Ñ, 0xA8 ¿, 0xAD ¡. Undefined glyphs (w=0 or h=0) draw nothing **and advance 0**,
so a character missing from the font simply disappears.
A verification render is at scratchpad/font1.png (fntpng.py, fnt.py).

### Glyph renderer 0x261 drawglyph(x, y, ch, colText, colBorder, font far*, dst far*)
```
p = dst + y*320 + x ; g = font + ch*34 ; w = g[0]>>4 ; h = g[0]&15
if w==0 || h==0: return
bit = 0 (the 2-bit counter is continuous over the whole glyph)
for row in 0..h-1: for col in 0..w-1:
    v = next 2 bits ; if v==1: p[row*320+col]=colText ; if v==2: p[...]=colBorder
```
There is **no clipping** (writes outside 320x200 wrap into memory). Text colour 0 or border colour 0 writes
colour 0. That is effectively "see-through" when the work buffer is copied to VGA by 0x4004,
which skips 0 pixels; see the STEXT mesh.

### Text metrics
* advance(ch) = space_w if ch==' ' ; else if glyph w>0: w + separation - 1 ; else 0
  (with separation 0, neighbouring glyphs overlap by 1 column: they share their outline column)
* TextWidth 0x78a8(s) = sum(advance) - separation   ('^' is not special here)
* DrawText 0x70d0(x, y, s, col, border, font, dst): draws each glyph at the running x, no clipping,
  and stops at NUL. '^' would be drawn as glyph 0x5E (not defined in LETRAS1, so nothing appears).
* DrawTextClip 0x7168(x, y, s, col, border, font, dst) (used by STEXT): ' ' **and '^'** act as spaces.
  It stops (returns) as soon as x+space_w > 318 for a space, or x + w + sep - 1 > 318 for a glyph.
* Line height used everywhere = **11** px.
* Shareware guards: 0x7168, 0x726d and 0x78a8 call fatal(0xc8) when the level number [fc9c] is > 5
  (6 for 0x726d). This is irrelevant for a port.

---------------------------------------------------------------------------------------------------
## 2. Speech text: SPEAK (op 3 -> 0x75af), PTEXT (op 4 -> 0x7667)

### State
```
DS:fca9  var7  text active (word) 1 = text pending/displayed
DS:fcab  word  text id to show (array indexed by fcc1; only [0] is ever set)
DS:fcc1  word  sequence index (0 = first text; reaching 1 ends the text)
DS:fcc3  word  index already laid out (-1 = none)
DS:fcbf  word  1 = SPEAK (player talks), 0 = PTEXT
DS:fcc5/fcc7   PTEXT x,y ; DS:fcd8 PTEXT colour (com4)
DS:fa2e  word  remaining display time in ticks (decremented by the 100 Hz ISR)
DS:8a24  byte  ticks per character (from SETUP.CFG)
Text line slots: 5 x 0x67 bytes at DS:f9cb (slot i at f9cb + i*0x67):
   +0x00 u8 active  +0x01 u8 active-last-frame (dirty tracking)  +0x02 i16 x  +0x04 i16 y
   +0x0a i16 width  +0x0c i16 height(=11)  +0x12 char text[57]  +0x65 u8 colour  +0x66 u8 border
```
Text-id resolution (SPEAK accepts all three ranges, PTEXT only the first):
* id < 1000   -> level text pool: DS:9823 + textoffs[id] (DS:962f, i16). An offset of -1 points at DS:9822, which
  holds an empty string.
* 1000..1999  -> description text of inventory **slot** (id-1000): far[DS:3796] + (id-1000)*0x4aa (+0). The records are per slot, not per object; see spec_inv.md
* >= 2000     -> engine message (id-2000) at DS:3bfa + (id-2000)*250. The 5 slots are filled from the level's
  message blocks (0 = ?, 1 = NOACINVINV, 2 = NOACINVOBJ, 3 = ?, 4 = YESACINVINV). The engine uses 2001, 2002, 2004.
  At start-up all 5 slots are set to "¡¡¡ ATENCION !!!^Este texto no esta definido." (DS:0c44). A level only
  overwrites a slot if its own string is non-empty.
* **Empty string**: Layout (0x726d) strcpy's message slot 0 into the empty string itself. That makes an
  undefined or empty text show "¡¡¡ ATENCION !!!^Este texto no esta definido." (unless a level defined msg 0).

### SPEAK(id) 0x75af
```
fcab[0]=id; walk state reset (fc1f=fc1d=fc1e=0, fc24=fc26=-100 => the player stops walking)
fcc1=0; fca9=1; fcbf=1
m = var1 (fbf5, current movement frame index)
for each direction d in (side: start=[d1],len=[d8]), (back: [cf],[d6]), (front: [d0],[d7]):
    if m > start && m <= start+len-1: m = start        # walking frame -> that direction's standing frame
fbf7 = m (saved), fbf5 = 0 ; fcc3 = -1
```
PTEXT(x,y,id,colour) 0x7667: it sets fcc5=x, fcc7=y, fcd8=colour, fcab[0]=id, fcc1=0, fca9=1, fcbf=0, fcc3=-1.
It does not touch the player.
Both opcodes first wait in the interpreter while fca9 != 0 (they re-yield). After setting the text they yield
(they act as RET). The interpreter yields again on any later SPEAK/PTEXT until the text is gone.

### Per-frame text update 0x769c (main loop, only while fca9==1)
```
if fcc1 != fcc3:
    if fcab[fcc1] == -1 || fcc1 > 0:            # end of text
        fca9 = 0
        if fcbf==1: fcbf=0; fbf5=fbf7; fc17=-1; player src = movtable[fbf5] (DS:200c+4*m);
                    var4/var5 = default size (DS:4334/4336); old size copies updated
        return
    if fcbf==1 (SPEAK):  x = var2 + var4/2 ; y = var3 + var27     (fc01 + fc0d/2 , fc03 + fcf4)
         id<1000 -> Layout(x,y,pool text, var8 colour (fcd4), var9 border (fcd6))
         1000..1999 -> Layout(x,y,inventory text,  var8, var9)
         >=2000     -> Layout(x,y,message,         var8, var9)
    else (PTEXT): Layout(fcc5, fcc7, pool text, fcd8 colour, var10 border (fcda))
    fcc3 = fcc1
if fa2e <= 0 && fca9==1: clear all 5 slots (active=0), fa2e=0, fcc1++   # removed next frame
```
The position is computed **once**, from the player's position at that frame. The text does not follow the player.
The anchor (x,y) is the **horizontal centre / bottom** of the text block: var3+var27 is the top of the
scaled player sprite (head).

### Layout 0x726d(cx, cy, s, colour, border)
```
if s[0]==0: strcpy(s, msg[0])                    # see above
clear slots 0..4 (active=0, text[0]=0)
n = 0 ; si = 0                                   # n = number of characters counted for the duration
for slot = 4 down to 0:                          # first text line -> slot 4, next -> slot 3, ...
    if slot.active: continue                     # (never true, they were just cleared)
    slot.active=1; slot.w = -separation; slot.h = 11; slot.col=colour; slot.border=border
    di = 0
    while s[si] != 0 and s[si] != '^':
        if di < 56: slot.text[di]=s[si]; slot.w += advance(s[si]); slot.text[di+1]=0; n++
        si++; di++                               # characters past 56 on one line are dropped silently
    if s[si]=='^': si++
    if s[si]==0: break                           # more than 5 lines: the rest is ignored
# vertical stacking: the lowest active slot (= LAST text line) gets y = cy-11, each next slot
# up (= previous text line) gets prev.y - 11   ->  k lines occupy [cy-11*k, cy)
# horizontal: each line x = cx - w/2 (integer division truncating toward 0)
wide = the widest active line (first one found on ties, scanning slot 0..4); delta = 0
while wide.x < 5:          wide.x++, delta++
while wide.x + wide.w > 315: wide.x--, delta--
other active lines: x += delta                   # the block keeps centring relative to the widest line
# vertical clamp (dx is NOT reset between the two passes):
dx = 0
top = first active slot scanning 4..0 (= top line)
if top.y < 5:  while top.y < 2: top.y++, dx++     # QUIRK: tests <5 but only pushes down to y=2
               all slots below index(top): y += dx
bot = first active slot scanning 0..4 (= bottom line)
if bot.y > var23-12:  while bot.y > var23-12: bot.y--, dx--     # var23 = DS:fd22, default 187
               all slots with index > index(bot): y += dx       # QUIRK: dx still includes the top shift
fa2e = 8a24 * (n < 10 ? n+2 : n)                 # display time in 10 ms ticks
```
Ticks per character [8a24] come from SETUP.CFG byte 2 (text speed): 0->16, 1->12, 2->8, 3->4, 4->2 ticks.
The demo SETUP.CFG has 3, so 4 ticks = 40 ms per character. n counts every stored character
including spaces, but not '^'. The ISR decrements fa2e every tick while fca9==1 and fa2e>0. It does not
decrement while the menu is open ([DS:98]=1).
(SETUP.CFG byte 1 -> DS:8a22/8a23 = player / animation frame-delay ticks: 0:15/10 1:8/5 2:4/2 3:3/1 4:1/0.)

### Drawing (main loop 0x5687)
Every frame, each active slot with non-empty text is drawn into the work buffer with DrawText 0x70d0
(slot.x, slot.y, text, colour, border), **after** all things and the player. Only the information bar is drawn later.
Dirty rects: (x, y, w, 11) are copied to VGA. When a slot was active last frame and is not now, its rect is
restored from the background. A port can redraw the text over the scene every frame.

### Skipping with the mouse (input handler 0x500d)
While a text is active (fca9 != 0) **or** the cursor state fd46 != 0, the normal click handling is skipped.
Instead:
```
if MMOUSE mode (fd40) >= 2: nothing (modes 2,3,4 cannot skip)
elif left or right button pressed: 0x788d -> clear the 5 slots, fa2e = 0   (the text ends at the next update)
```
"Pressed" = 0x688(btn): INT 33h fn 5 press count > 0 **and** the current button status == exactly that button
(1=left, 2=right). The button must still be held when polled, and pressing both at once does not count.
No keyboard skip exists.

### Talking animation
While fcbf==1 the player updater (0x481c) switches the movement index to a talking sequence according to the
facing saved in fbf7: side ([d1]) -> [d2] (it keeps the mirror flag fc1b, so left-facing talk is mirrored),
back ([cf]) -> [d3] with mirror off, front ([d0]) -> [d4] with mirror off. While fbf5 >= [d3] it keeps animating (fbf4=1).
During speech the frame delay is doubled (fccf = 2*[8a22] instead of [8a22]). The bytes DS:cf..d5 are the
movement start indices from NIVEL.MOV (DEFMOVS order: back, front, side, talk-back, talk-front, talk-side).
The exact meaning of d2/d3/d4 belongs to the movement spec. NOTE: the comparisons above are copied literally
from the code. I did not verify how the side-talk case (d2 < d3) keeps animating.
When the text ends, the player gets back its saved standing frame (fbf7).

---------------------------------------------------------------------------------------------------
## 3. STEXT(sel, mode, mask) — op 42 -> 0xabf8 ; UI per frame 0xaefd ; draw 0xada8

SELECTEXT table: DS:40dc, 50 x 12 bytes: i16 text[5] (-1 = unused), +10 i16 unused.
State: 1bbc=mode (com2), 1bbd=drawn flag, 1bbe=highlighted row (0xff none), 1bbf=row under cursor,
fbd8..fbdc = hidden[5] (byte), fbce = rowY[5] (i16 offsets), fcf0 = active SELECTEXT (-1 = none),
fd2e = saved verb.

### Opcode 0xabf8
```
1bbf=1bbe=0xff; 1bbd=0; 1bbc=mode
8a8c=0 (drop the inventory object held as cursor); fc22=0; rebuild info bar text (0x52c0)
Y = var21 (fd05): if Y+57 >= 200: Y=144 ; if Y<0: Y=0     (stored back into var21)
fcf0 = sel ; fd2e = current verb (fd2c)
for i in 0..4:  hidden[i] = (decimal digit i of mask != 0) || text[i] == -1
                # digit 0 = units = text 1, digit 1 = tens = text 2, ... (mask is DECIMAL, e.g. 00101 = 101)
k=0; for i in 0..4: if !hidden[i]: rowY[i] = 11*k; k++      (hidden rows keep stale rowY)
MOUSEACTION(4)            # Hablar cursor
set mouse position (x unchanged = fd4a, y = var21)   -> the cursor's TOP-LEFT moves to y = var21
```
While fcf0 != -1 the main loop calls only 0xaefd(fcf0) each frame. It skips scripts, animations, the walk
and the normal input; the timer still runs. When fcf0 becomes -1 it does a full 320x200 work->VGA copy
(this removes the panel; the background is rebuilt by the normal loop).

### Panel draw 0xada8 (first frame only)
```
clamp var21 as above; Y = var21
mesh: for r in 0..55 (screen rows Y-1 .. Y+54), x in 0..319:
      colour = ((x + r) even) ? var17 (fce8) : var18 (fcea)        # checkerboard, row Y-1 starts with var17
      written into the work buffer (colour 0 is written too; 0x4004 then does not copy 0 pixels, so the old
      screen shows through => "0 = no se dibuja" as the manual says)
for i in 0..4: if text[i] != -1 && textoffs[text[i]] != -1 && !hidden[i]:
      DrawTextClip(1, Y+rowY[i], pooltext(text[i]), var13 (fce0) colour, var14 (fce2) border)
copy rect (0, Y-1, 320, 57) to VGA (0x4004: non-zero pixels only)
if no text was drawn: fatal 0x73 "No hay textos definidos para STEXT"
```
Note: var14 "colour de fondo" is in fact the **glyph border colour** (value-2 pixels). The same goes for var16.
The text starts at x=1 and is clipped at x=318 (DrawTextClip; '^' = space). Only pool texts (id<1000) can be used.
The defaults var17=var18=0 (the manual claims 7/0) mean no visible mesh by default.

### Per frame 0xaefd(sel)
```
if !1bbd: draw panel; 1bbd=1
hx = cursorX + hotX (fd4a+fd5e) ; hy = cursorY + hotY (fd4c+fd60) ; Y = var21
row = 0xff
for i in 0..4:  if 0<=hx<320 && Y+rowY[i] <= hy < Y+rowY[i]+11 && text[i] != -1 && !hidden[i]: row = i  (last wins)
if row != 1bbe:
    if row <= 4: DrawTextClip(1, Y+rowY[row], text, var15 (fce4), var16 (fce6))      # highlight
    if 1bbe <= 4: DrawTextClip(1, Y+rowY[1bbe], text, var13, var14)                    # un-highlight
    copy rect (0, Y-1, 320, 57) to VGA ; 1bbe = row
    (the mesh is NOT redrawn; the new glyphs paint over the old ones exactly)
if (left pressed || right pressed) && row <= 4:          # either button selects
    var43 (fbdd) = sum over i of (hidden[i] || i==row) ? 10^i : 0      # 1,10,100,1000,10000
    var19 (fcec) = row+1 ; var20 (fcee) = text[row] ; fcf0 = -1
    MOUSEACTION(fd2e)                                    # restore verb (cursor hotspot kept)
    if mode == 0: SPEAK(text[row])
```
So var43 is a decimal "digit" mask, directly reusable as the next STEXT mask. Undefined (-1) entries also
count as hidden, so when everything is used var43 = 11111.

---------------------------------------------------------------------------------------------------
## 4. Information bar

State: 8a29 (0 = none, 1 = valid/drawn, 2 = needs rebuild), 8a2a = last drawn state (0xff forces a redraw),
8a33 = last hovered thing+1, text buffer DS:8a3c,
x DS:8a2b, y DS:8a2d (= var24, default 189 = 0xbd), w DS:8a34, h DS:8a36 = 11, previous rect at 8a2f/8a31/8a38/8a3a.
Colours: var11 (8a25, default 2) text, var12 (8a27, default 15) border. Enable var29 (fd01, default 1).

### Text builder 0x52c0 (called when 8a29 == 2, and from STEXT/MOUSEACTION)
```
s = ""
if 8a8c (an inventory object is held as the cursor):
    s += invobj[8a8d].name ; s += " & "                    (DS:0122 = " & ")
if fca7 != 0 (inventory slot under the cursor, see spec_inv) && slot fca7 used:
    if 8a8c==0 || (8a8c==1 && slot.obj != 8a8e):
        s += slotrecord[fca7-1].name   (far[3796]+(fca7-1)*0x4aa+0xfa; fca7 = hovered slot+1, slot table DS:8a95, 6 bytes/slot)
        8a8f = slot byte +2
        if verb (fd2c)==8 && 8a8c==0: s += " con "          (DS:0126) — verb 8 never seems to be set; dead code
if fc22 != 0 (THING under the cursor = fc22-1): s += thing[fc22-1].name ; 8a33 = fc22
w = TextWidth(s)
if w > 0: 8a34=w; 8a36=11; 8a2b = 160 - w/2; 8a29=1; 8a2a=0xff     (centred on x=160; no clamping)
(if w == 0 nothing changes here; the hover code sets 8a29=0 when nothing is under the cursor)
```
**Only names are shown, never verb words** ("Puerta", or "Vaso & Puerta" while using an inventory object).
The THING name is stored at thing+0x31 (chars +0x0D in the level file); inventory names are at
record+0xfa.

### Hover detection 0x54ed (each frame from the input handler, only if var30 (fcff)==1 and the cursor state is 0;
skipped when MMOUSE mode is 1 or 2)
```
p = hotspot point (fd4a+fd5e, fd4c+fd60)
for t in drawOrder (DS:8b49, 30 bytes, 0xff = end ... the loop runs all 30 and skips 0xff):
    if AREA.x <= p.x <= AREA.x+AREA.w && AREA.y <= p.y <= AREA.y+AREA.h (inclusive) && mode in {1,3}:
        fc22 = t+1 ; if 8a33 != t+1: 8a29 = 2 ; return           # FIRST match in draw order wins
                                                                # (= the lowest INDEX, i.e. the one drawn first/below!)
none: if 8a8c==0 && fca7==0: 8a29 = 0
      if 8a8c!=0 && 8a33!=0: 8a29 = 2
      8a33 = 0
```
The input handler also sets 8a29=2 whenever fc22 changed since the previous frame.
The main loop sets 8a29=0, 8a2a=0xff, 8a33=0 while a text is active or an action script runs (fcc9 in
1..49), so the bar is hidden during speech and cut-scenes.

### Drawing
When (8a29 != 8a2a or fd01 != fd02): the old rect is restored from the background. Then, if var29==1 and
8a29 != 0, DrawText(8a2b, var24, 8a3c, var11, var12) is drawn into the work buffer, as the last thing in the
frame (on top of the speech text), and copied to VGA. With dirty-rect rendering it is only redrawn when it
changes. A port should draw it every frame while 8a29 != 0 and var29 == 1.

---------------------------------------------------------------------------------------------------
## 5. Mouse cursor, verbs, clicks

### NIVEL.MOU (EDGAME.DAT entry 9; source FUENTES/MOUSE.ED, compiled type 3)
```
30-byte header (02 00 00 03 + 26 zeros) ; u16 count ;
count x DEFMOUSE records, 6 rows x 6 i16 = 72 bytes each:
    row r (0 Ir a, 1 Mirar, 2 Coger, 3 Usar, 4 Hablar, 5 menu cursor): srcX, srcY, w, h, hotX, hotY
    (source graphic = MOVSPCX, colour 0 transparent)
```
Demo: count=1, record 0 = (299,1,6,10,2,9) (298,13,13,7,0,0) (298,22,10,11,1,10) (298,34,9,11,2,0)
(298,47,11,10,0,9) (298,59,9,12,0,0). Not stored +5; it is plain little-endian.

NEWMOUSE(n) 0x2aae (op 61): it opens EDGAME.DAT, seeks to entry 9 and checks type==3 (fatal 0x0b otherwise).
It reads count; n >= count -> fatal 0x74 "El NEWMOUSE() '%s' no existe". Then it seeks to entry + **0x20 + n*0x3c** and
reads 0x48 bytes into DS:1fc4 (6 x 12-byte rows). **BUG**: the stride is 60 but records are 72 bytes,
so only NEWMOUSE(0) works correctly. A port should use 0x20 + n*60 for exactness, or n*72 to do what was
meant. Finally it calls MOUSEACTION(0) (verb resets to Ir a). The cursor table is uninitialised until the first
NEWMOUSE (ROOM0 action 0 calls NEWMOUSE(0)).

### Cursor position model
The mouse driver coordinate (DS:fd4a, fd4c) is the cursor's **top-left** pixel. The INT 33h user handler (0x6d2)
copies cx,dx on every movement event. The "hot" point used for everything is (fd4a+hotX, fd4c+hotY).
Current cursor gfx: fd42/fd44 = srcX/srcY, fd52/fd54 = w/h, fd5e/fd60 = hotX/hotY.
Ranges set by MOUSEACTION: x in [0, 320-w+1], y in [0, 200-h] (INT 33h fn 7/8). So the hot point can
never reach the screen edges past the cursor size, and the inventory trigger (y<=0) means the cursor top
is touching the top edge.
The cursor is drawn by the 100 Hz timer ISR directly to VGA (0xcae, clipped; the previous rect is restored
from the work buffer by 0x4d5). It is not drawn when MMOUSE mode == 2. fd46 is a handshake state with the ISR:
0 normal, 2 = request erase -> ISR sets 3 (hidden), 4 = erase+hide, 5 = request redraw -> ISR redraws, sets 0.

### MOUSEACTION(n) 0x53ec (op 44)
```
if n == 100: verb++ else verb = n               (verb = DS:fd2c, byte)
if verb > 4 && verb != 101: verb = 0
if verb == 101: verb = 5                         (5 = menu cursor row; MOUSEACTION(101) shows it)
keep the hot point fixed: setmouse(fd4a + oldHotX - newHotX, fd4c + oldHotY - newHotY)
load row[verb] -> fd42,fd44,fd52,fd54,fd5e,fd60 ; fd48=-1 (force redraw) ; 8a8c = 0 (drop held object)
rebuild info bar text (0x52c0)
set ranges x 0..321-w, y 0..200-h
```
(The cursor-move uses the old hotspot values before they are overwritten. With an uninitialised table the
first call may jump the mouse.) Verb 100 from 4 -> 0 and from 5 -> 0.

### Input handler 0x500d (each frame, main loop)
```
if fd46 in {2,3}: return
if fca9 != 0 || fd46 != 0:  [text skip, see §2] ; return
if 0 < fcc9 < 50: return                           # an ACTION script triggered by a verb is running
# inventory trigger
if fd2b==0 && fd4c <= 0 && var31 (fcfe)==1 && (fd40 < 2 || fd40 == 4) && 8a90 == 0:
    fd2b = 1 (open the inventory, see spec_inv) ; fd2e = verb ; if verb not in {1 Mirar, 3 Usar}: MOUSEACTION(1)
    return
fd46 = 0 ; fc23 = fc22 ; fc22 = 0
if var30 (fcff)==1: hover (0x54ed)  ; if fc22 != fc23: 8a29 = 2
if (fd40 < 2 || fd40 == 4) && RIGHT pressed && var32 (fd03)==1: MOUSEACTION(100)   # right button = next verb
(inventory-object-in-hand bookkeeping with 8a8c/8a90/8a91 -> 0x7e0b use object on thing,
 0x7d29 combine two inventory objects; see spec_inv)
if !LEFT pressed or !(fd40 < 2 || fd40 == 4): return
fd2f = verb ; click point fd5a/fd5c = hot point
if fca9==0 && fcc9 != 0 && fd46 != 2: fcc9 = 0         # cancel a pending walk-to-thing
stop walking (fc1f=fc1d=fc1e=0, fc24=fc26=-100)
if 8a8c != 0 && fca9==0 && fca7==0 && fc22==0: 8a8c = 0        # click on nothing drops the held object
if 8a8c != 1 && fcc9==0 && fc22==0 && clickY <= var23 && verb == 0: GOXY(clickX, clickY)   # walk only with "Ir a"
if 8a8c != 0 && fca9==0 && fc22 != 0: 8a91=100; 8a92=fc22-1; 8a90=1; 8a29=2   # use held object on THING (next frame)
if fc22 != 0 && 8a8c != 1 && fca9==0 && verb < 5: VerbOnThing (0x8362)
```
Walking by clicking empty floor happens **only with verb 0 (Ir a)**. Clicking empty floor with another verb
does nothing. Clicks are edge-ish: 0x688 uses INT 33h fn 5 press counts (since the last call) plus "the button is
still down".

### VerbOnThing 0x8362 -> state machine 0x7937 (fcc9/fcca), thing t = fc22-1 (8a8e = t)
```
v = thing[t].ACTIONS[verb]   (thing + 0x48 + 2*verb)
v >= 5000: fd36 = v-5000; fcc9=100; fcca=1         # don't walk, face GOTO dir, say text
v >= 4000: fcc9=106; fcca=0                        # face GOTO dir, then run ACTION (v-4000)
v >= 3000: fd36 = v-3000; fcc9=100; fcca=2         # (undocumented) say text, no walk, no facing
v >= 2000: fd36 = v-2000; fcc9=100; fcca=0         # walk to GOTO, face, say text
v >= 1000: fcc9 = (v-1000) & 0xff; fcca=0          # run ACTION immediately where you stand
v > 0:     fcc9=101; fcca=0                        # walk to GOTO, face, run ACTION v
v == 0:    if verb==0: fd36=0; fcc9=100; fcca=0    # Ir a: just walk (text 0 is NOT said: fd36==0 => nothing)
```
State 100/101/106 per frame: fcca 0 -> GOXY(thing GOTO x,y), fcca=1. fcca 1 and not walking (fcce==0) -> set
the facing from GOTO dir (0 left = side frame [d1] + mirror, 1 right = [d1], 2 front [d0], 3 back [cf]),
fcca=2. fcca 2 -> 100: SPEAK(fd36) if fd36 != 0 ; 101: fcc9 = action (low byte) ; 106: fcc9 = (v-4000) low byte
(if v == 4000 it never leaves state 106). Any 1..99 fcc9 runs ACTION[fcc9] each frame until it finishes
(only while no text and not walking). States 102/104 = inventory-object-on-thing (ACINVTH), see spec_inv.
Note: the action number is truncated to a byte.

### MMOUSE(mode) (op 46 -> fd40=mode, fd46=5)
| mode | cursor drawn | hover/info bar | clicks/right-click/inventory | click skips text |
|------|--------------|----------------|------------------------------|------------------|
| 0 | yes | yes | yes | yes |
| 1 | yes | **no** (0x54ed skipped) | yes (so only walking works) | yes |
| 2 | no  | no  | no  | no |
| 3 | yes | yes | no  | no |
| 4 | yes | yes | yes | **no** |

### "Usar" with an inventory object
Picking an object in the inventory makes it the cursor/held object (8a8c=1, 8a8d/8a8e = object). The info bar
then shows "<obj> & <target>". A left click on a THING queues 8a91=100, 8a92=thing, 8a90=1. The next frame runs
0x7e0b (ACINVTH lookup). Clicking an inventory slot combines objects (0x7d29: IAC table, YESACINVINV 2004 /
NOACINVINV 2001). Details: **spec_inv.md**.

---------------------------------------------------------------------------------------------------
## 6. Inventory — summary (full spec in spec_inv.md)
* NIVEL.INV: 30-byte header, u16 count, 768-byte palette, then 0x4aa-byte records at 0x320: text[250] (+0x1E),
  name[30] (+0x20), u16 w, u16 h, w*h pixels (pre-cut picture, colour 0 transparent).
* Slots: 30 x 6 bytes at DS:8a95 {u8 used, u8 flag, u16 obj, u16 cache}. ADDINV copies the record into the first
  free slot's buffer (far[3796]+slot*0x4aa). SUBINV removes the first match and shifts the rest down.
* The panel opens when the cursor top reaches y=0 and closes when the cursor y > 30. It is a 320x30 checkerboard
  var44/var45 with 8 slots at x=13+37*i, y=2 and arrows in var46. The game is frozen while it is open.
  Only Mirar/Usar work inside it (right-click toggles them). Mirar says the slot text (id 1000+slot).
  Usar picks the object up (the info bar shows "Name & ..."; the cursor graphic does NOT change).
* Combining uses NIVEL.IAC (50 x {used,obj1,obj2,new,mode}): SPEAK 2004 on success, 2001 on failure.
  Using an object on a THING goes through ACINVTH, with the fallback SPEAK 2002. See spec_inv.md.

## 7. MENU — summary (full spec in spec_menu.md)
It opens with **SPACE** (scancode 0x39 in [dc]) when var28==1, or with op 52 MENU. ESC only closes it.
The panel is (48,10)-(270,132) with the title from NIVEL.TIT and buttons CARGAR/GUARDAR/SALIR/JUGAR at x 207..267,
y 46/61/75/89. There are 8 savegame slots SAVEGAME._1_.._8_. The file format is in spec_menu.md.

---------------------------------------------------------------------------------------------------
## 8. Defaults (varinit 0xb78c) and overlap quirks

| var | addr | default | use |
|---|---|---|---|
| 8  | fcd4 | 1   | SPEAK text colour |
| 9  | fcd6 | 15  | SPEAK border |
| 10 | fcda | 15  | PTEXT border |
| 11 | 8a25 | 2   | info bar text |
| 12 | 8a27 | 15  | info bar border |
| 13 | fce0 | 2   | STEXT text |
| 14 | fce2 | 15  | STEXT border ("fondo") |
| 15 | fce4 | 6   | STEXT selected text |
| 16 | fce6 | 15  | STEXT selected border |
| 17 | fce8 | **0** | mesh colour 1 (the manual says 7) |
| 18 | fcea | 0   | mesh colour 2 |
| 21 | fd05 | 5   | STEXT Y |
| 23 | fd22 | 187 | play-area height (walk/text clamp) |
| 24 | 8a2d | 189 | info bar Y |
| 28 | fd04 | 1 (byte) | menu enable |
| 29 | fd01 | 1 (byte) | info bar enable |
| 30 | fcff | 1 (byte) | things enable |
| 31 | fcfe | 1 (byte) | inventory enable |
| 32 | fd03 | 1 (byte) | cursor/verb (right-click) enable |
| 33 | fbdf | 250 | colours darkened by the menu |
| 34,35 | fbeb,fbed | 254,253 | menu title text/border |
| 36..40 | fbe1..fbe9 | 250..254 | menu colours |
| 44,45,46 | fd30,fd32,fd34 | 2,0,4 | inventory bg 1/2, arrows |

**Word-access overlap quirks** (script VAR/ADD always read and write 16 bits):
* VAR 31 (fcfe) also writes fcff = var30 -> setting var31 to 0 or 1 **clears var30 (THINGs off)**.
* VAR 32 (fd03) also writes fd04 = var28 -> setting var32 **disables the menu** (var28=0).
* VAR 28 (fd04) also writes the low byte of var21 (fd05) -> it **zeroes the low byte of the STEXT Y**.
* VAR 29 (fd01) also writes fd02 (the info bar's "previous enable" copy) -> it only forces a redraw.
* VAR 30 (fcff) also writes fd00 (the things' "previous enable" copy) -> it forces a redraw.
* Reading var28..32 returns the byte plus 256 * the next byte.
A port aiming at exact behaviour should model these variables as overlapping bytes.

## Open points / uncertain
* What fca7 / 8a8d / 8a8e hold exactly, and the " con " path (verb 8), are described in spec_inv.md. I found no setter for verb 8.
* The talking-animation sequence mapping (d2/d3/d4) is copied literally; I did not confirm it against NIVEL.MOV.
* Message slots 0 and 3 of the level message blocks: I could not tie them to a compiler keyword
  (slot 0 is used as the "undefined text" replacement).


---------------------------------------------------------------------------------------------------
# APPENDIX A — Inventory (full, from spec_inv.md)

# EDPACK v2.0 RUNDAT — Inventory spec (item 6)

All addresses: code = RUNDAT.bin linear (near-call targets wrap mod 0x10000), data = DS offsets.
Pseudo-code follows the original; quirks are marked QUIRK. "hot" = cursor hotspot point
(hx = [fd4a]+[fd5e], hy = [fd4c]+[fd60]); [fd4a]/[fd4c] = cursor top-left (mouse position).

## 1. NIVEL.INV (EDGAME.DAT entry 6, file type 1)

```
+0x000  30-byte compiled header (02 00 00 01 + 26 zero)   (checked: type must be 1, else fatal 0xb)
+0x01e  u16  count                (number of OBJINV; demo = 11)
+0x020  u8   palette[768]         = the PCX palette of INVENTAR.PCX, 8-bit values (verified identical to
                                    the last 768 bytes of FUENTES/INVENTAR.PCX)
+0x320  record[count], 0x4aa (1194) bytes each, record n at 0x320 + n*0x4aa:
        +0x000 char text[250]   each byte +0x1E (decode: c-0x1E, until NUL; NUL is stored as 0x1E)
        +0x0fa char name[30]    each byte +0x20 (decode: c-0x20 until NUL)
        +0x118 u16 w, u16 h     (demo: always 35 x 26)
        +0x11c u8  pix[w*h]     row-major, palette index, 0 = transparent; 910 bytes used of 910 (0x392-4)
```
The compiler cuts each object's graphic out of the PCX named in `PCX("INVENTAR.PCX",x,y)`
(35x26 block at x,y); the runtime never reads INVENTAR.PCX / INVPCX for object pictures.
Unused tail bytes of text/name contain stale data from the previous record (compiler buffer not cleared).

Palette loader (0x2403..0x24eb, at level/game start): opens EDGAME.DAT, seek entry 6, checks header,
fseek(+2 relative → skips count) and reads 768 bytes into DS:3180 (INV palette). It is merged by
0x2210 into the working palette DS:2580: an entry whose RGB is FF,FF,FF in the scene palette is
"free" and gets the INV palette entry if that one is not FF,FF,FF (palette merge is another agent's area).

Object records are NOT preloaded: ADDINV reads the record from the file each time (below).

## 2. In-memory inventory

```
DS:8a95 + 6*s, s = 0..29   (30 slots)            [NOTES said 8a97: that is the obj field]
   +0 u8  used        (1/0)
   +1 u8  drawnFlag   (cleared for the visible 8 slots by 0x6e9b; never set -> effectively unused)
   +2 u16 obj         (object number = OBJINV index)
   +4 u16 cacheObj    (set to -1 on compaction; compared but never written otherwise)
far[DS:3796] + s*0x4aa : the full 0x4aa record (text,name,gfx) of the object in slot s (decoded)
DS:fccc  u8  first visible slot (scroll offset)
DS:fd2b  u8  panel open flag ;  DS:009b u8 panel-drawn flag ; DS:0099 u8 last hovered (fca7 copy)
DS:fca7  u16 hovered slot + 1 (0 = none)
DS:fd2e  u8  verb saved when the panel opened (also by STEXT)
"object in hand" state:
DS:8a8c  u8  0 = none, 1 = holding, 0xff = combine just executed (reset next frame)
DS:8a8d  u8  slot of held object ; DS:8a8e u8 obj number of held object
DS:8a90  u8  pending-use request (1) / 0xff after combine ; DS:8a91 u8 second slot, or 100 = "on a THING"
DS:8a92  u8  second operand: obj number (combine) or THING index (use on thing)
DS:8a93  u8  ACINVTH action/text selected ; DS:8a8f u8 obj number last shown in info bar (0xff none)
```
Text id 1000+s (SPEAK) = text of the object in SLOT s (not object number s).

### ADDINV(obj)  (0x6ae7)
```
for s in 0..29:
  if slot[s].used == 0:
     f = fopen(EDGAME.DAT,"rb"); if !f return 0
     seek(4*6); off = u32; if off==0 -> fatal(1)
     seek(off); read 30-byte header; if type != 1 -> fatal(0xb, "NIVEL.INV")
     count = u16
     if obj >= count -> fatal(0x76, itoa(obj))      // "El Objeto del Inventario '%s' no existe..."
     seek(off + (u16)(obj*0x4aa) + 0x300 + 0x20)     // QUIRK: obj*0x4aa is 16-bit (overflow for obj>=55)
     slot[s].obj = obj; slot[s].used = 1; slot[s].drawnFlag = 0
     rec = far3796 + s*0x4aa
     read 250 -> rec+0;     decode bytes until first 0: c += 0xE2 (i.e. -0x1E)
     read 30  -> rec+0xfa;  decode until 0: c += 0xE0 (-0x20)
     read 0x392 raw -> rec+0x118
     fclose; refresh(0x6e9b); return
fatal(0x32)   // "ERROR : - No se pueden añadir mas objetos al inventario -"
```
New objects go to the FIRST free slot (always the end, list is kept compact). No duplicate check.

### SUBINV(obj)  (0x6d3c)
```
for s in 0..29: if slot[s].obj == obj:          // QUIRK: does not test .used (free slots have obj 0)
     slot[s].used = 0; slot[s].obj = 0; compact(s); refresh(); return
```
compact(s) (0x6d78): for i = s..29: if i==29 {slot29 = {0,_,0,-1}; break}
 slot[i].used = slot[i+1].used; slot[i].obj = slot[i+1].obj; slot[i].cache = -1;
 if !slot[i+1].used break; copy record i+1 -> i (strcpy text, strcpy name, 0x392 gfx bytes).
 Then `while fccc>0 && !slot[fccc+7].used: fccc--` ; refresh().

### SEEINV(obj) (handler 0x377c) — QUIRK, always var22 (DS:fd20) = 0:
```
var22 = 0; si = 0; while (si < 0) {...}     // loop condition inverted -> body never runs
```
Intended: var22 = any slot.obj == obj.

### Savegame: the 30*6 = 0xb4 bytes at DS:8a95 are written (0x986e). On load (0x9327):
for s in 0..29: if slot[s].used==1 { slot[s].used = 0; ADDINV(slot[s].obj) } — records rebuilt
from NIVEL.INV (each lands back in slot s). fccc is not touched there.

## 3. Panel open / close (input handler 0x500d, panel loop 0xb4e3, main loop 0x56b5)

Opening (in 0x500d, per frame, only when no text active ([fca9]==0), cursor state [fd46]==0,
[fcc9] not in 1..49 (no pending walk-action)):
```
if fd2b==0 && mouseTopLeftY [fd4c] <= 0 && var31 [fcfe]==1 && (MMOUSE<2 || MMOUSE==4) && 8a90==0:
    fd2b = 1; fd2e = verb [fd2c]
    if verb != 1 (Mirar) && verb != 3 (Usar): MOUSEACTION(1)      // panel only knows Mirar/Usar
    return
```
i.e. the panel opens when the mouse touches the top screen row (y = 0).
QUIRK: var31 is DS:fcfe accessed as a 16-bit word: VAR(31,x) also writes the high byte into
DS:fcff = var30 (things enable) -> VAR(31,1) sets var30 low byte to 0. (Default fcfe=1, fd03=1.)

Main loop (0x56b5): if fd2b==1 { call 0xb4e3; if still open -> skip the WHOLE rest of the frame
(no scripts, animations, text timers, movement: the game is frozen while the panel is open);
if it was closed in this call -> 0x65a0 (rebuild back buffer), blit full 320x200, rebuild info text
(0x52c0), show cursor ([fd46]=5, wait) and continue next frame }.

0xb4e3 (one call per frame while open):
```
if [9b]==0: drawPanel(); [9b]=1
if [fd4c] > 30:                         // cursor top-left below row 30 -> close
    hide cursor; fd2b=0; [9b]=0; fca7=0; [99]=0
    if 8a8c != 1: MOUSEACTION(fd2e)      // restore verb unless an object is in hand
    goto infobar
fc22 = 0; fca7 = 0; hoverSlots()        // 0x6a0a
if [99] != fca7: [99] = fca7; goto infobar        // hover changed: only update bar this frame
if leftButton():
    if 0 <= hx <= 13 && fd2b:            { if fccc>0: fccc--; refresh(); return }        // left arrow zone
    if 307 <= hx <= 320 && fd2b && slot[fccc+8].used && fccc+8 < 30: { fccc++; refresh(); return }
    if fca7 && verb==1 (Mirar):
        hide cursor; fd2b=0; [9b]=0; MOUSEACTION(fd2e)    // (MOUSEACTION also clears 8a8c)
        SPEAK(fca7 + 999)                                // = text 1000+slot: object description
        8a8c=0; fca7=0; [99]=0
    if fca7 && verb==3 (Usar):
        if 8a8c != 0:                    // second object chosen -> combine request
            hide cursor; 8a91 = fca7-1; 8a92 = slot[fca7-1].obj; 8a90 = 1; 8a29 = 2
            fd2b=0; [9b]=0; fca7=0; [99]=0          // panel closes, verb stays Usar
        if 8a8c != 1:                    // (also true after a combine request? no: 8a8c is 1 there)
            8a8d = fca7-1; 8a8e = slot[fca7-1].obj; 8a8c = 1; 8a29 = 2   // pick up, panel stays open
infobar:
    buildInfoText()   (0x52c0)
    if var29 [fd01]==1: restore rect (0,var24,320,11) from background, compose, draw info text at
       (8a2b, var24) colours var11/var12, flush rect to screen
if rightButton() && var32 [fd03]==1: MOUSEACTION(verb==1 ? 3 : 1)    // right click toggles Mirar<->Usar
```
Note: no other verb works in the panel (Coger/Hablar/Ir a do nothing on slots). There is no
keyboard shortcut for the inventory.

hoverSlots (0x6a0a):
```
for i in 0..7:
  if i*37+13 <= hx <= i*37+48 && hy >= 0 && slot[fccc+i].used:
     fca7 = fccc+i+1
     if 8a8c && 8a8e == slot[fca7-1].obj: fca7 = 0; 8a8f = 0xff     // the held object cannot be hovered
     if !(8a8f == obj_at(fca7) && 8a29 != 0): 8a29 = 2               // request info-bar refresh
     return
if 8a33==0 && 8a8c==0: 8a29 = 0
if 8a8c && 8a8f != 0xff: buildInfoText()
8a8f = 0xff
```
(obj_at(0) reads the word at DS:8a91 — harmless quirk.)

## 4. Panel drawing

drawPanel (0xb1a6) — into back buffer far[3792] (320 wide), then blit rows 0..44 to screen:
```
0x65a0()   // re-compose back buffer first
for y in 0..29: for x in 0..319:  pixel = ((x+y)&1)==0 ? var44 [fd30] : var45 [fd32]
            // checkerboard ("malla"); defaults var44=2, var45=0
for i in 0..7: s = fccc+i; if slot[s].used:
     blitTransparent(x = i*37+13, y = 2, rec(s)+0x118)   // w,h header, colour 0 transparent
if fccc > 0:  left arrow, colour var46 [fd34] (default 4), horizontal spans (x1..x2 inclusive, y):
     (4..5,11) (3..5,12) (2..10,13) (1..10,14) (0..10,15) (1..10,16) (2..10,17) (3..5,18) (4..5,19)
if slot[fccc+8].used && fccc+8 < 30:  right arrow:
     (314..315,11) (314..316,12) (309..317,13) (309..318,14) (309..319,15) (309..318,16)
     (309..317,17) (314..316,18) (314..315,19)
blit back buffer rect (0,0,320,45) to screen
```
Slot pictures: 35x26 at x = 13,50,87,124,161,198,235,272 (spacing 37), y = 2..27.
QUIRK: the call from 0xb2bb targets cs:0x0bfb, 10 bytes before the real transparent-blit routine
0x0c05 (those bytes hold the routine's own CS variables and get executed as code first). Implement
simply as transparent blit (pixel 0 skipped, no clipping).

refresh (0x6e9b) — called by ADDINV/SUBINV/compact/scroll:
```
clear drawnFlag of slots fccc..fccc+7
drew = false
if fccc>0 && fd2b: draw left-arrow spans as above but at y = 35..43 (instead of 11..19); drew=true
if slot[fccc+8].used && fccc+8<30 && fd2b: right-arrow spans at y = 35..43; drew=true
if drew: drawPanel()
```
QUIRK: these extra arrows land in the back buffer below the panel (rows 35-43, inside the 45-row
blit), so after scrolling a second pair of arrows is visible just under the panel over the scene
until that area is redrawn. Also: scrolling only redraws the panel if at least one arrow exists
(always true after a real scroll). Nothing is drawn while the panel is closed.

Panel geometry summary: 320x30 strip at the top; 8 visible slots; scroll by one slot per click;
left-arrow click zone hx 0..13, right zone hx 307..320; close when cursor top-left y > 30.

## 5. Info bar interaction (0x52c0 builds DS:8a3c)
```
text = ""
if 8a8c (holding): text = name(slot 8a8d) + " & "            // DS:0122 " & "
if fca7 && slot[fca7-1].used && (8a8c==0 || (8a8c==1 && 8a8e != slot[fca7-1].obj)):
     text += name(slot fca7-1); 8a8f = slot[fca7-1].obj
     if verb==8 && 8a8c==0: text += " con "                 // DS:0126; verb 8 never occurs -> dead code
if fc22 (hovered THING+1): text += thing[fc22-1].name; 8a33 = fc22
```
e.g. "Queso" (hover), "Saco & " (holding, nothing hovered), "Saco & Queso", "Gancho & Puerta".
No verb words are ever shown. (Layout/colours of the bar: see main UI spec.)
The cursor does NOT change to the object's picture while holding it; the cursor stays the "Usar"
cursor (no code reads the record gfx except the panel).

## 6. Held object outside the panel (0x500d, per frame)
```
if (MMOUSE<2 || MMOUSE==4) && rightButton() && var32: MOUSEACTION(100)
        // QUIRK: MOUSEACTION always sets 8a8c=0 -> cycling verbs drops the held object
if 8a8c==0xff && 8a90==0xff: 8a8c=0; 8a90=0; 8a29=2       // finish combine
if 8a8c==1 && 8a90==1:
    if 8a91==100: useOnThing() (0x7e0b); 8a90=0
    else:         combine()    (0x7d29); 8a8c=0xff; 8a90=0xff
if leftButton() && (MMOUSE<2 || MMOUSE==4):
    clickpoint = hot; ...
    if 8a8c && !textActive && fca7==0 && fc22==0: 8a8c = 0          // click on nothing: drop object
    if 8a8c!=1 && fcc9==0 && fc22==0 && hy <= var23 && verb==0: GOXY(hx,hy)
    if 8a8c && !textActive && fc22: 8a91=100; 8a92=fc22-1; 8a90=1; 8a29=2   // use on THING (next frame)
    if fc22 && 8a8c!=1 && !textActive && verb<5: verbAction(0x8362)
```
(Clicking nothing while holding only drops the object — no walk, since verb is Usar.)
STEXT (0xabf8) also clears 8a8c. Level change (0x1b8e) sets fd2b=0, fd2e=0.

### combine (0x7d29) — NIVEL.IAC
NIVEL.IAC (entry 10, type 4): 30-byte header, then 0x1f4 raw bytes read straight (no RLE, no
encryption) into DS:698e at game start (0xb9c1): 50 records x 10 bytes
`{i16 used(1), i16 obj1, i16 obj2, i16 newObj, i16 mode}` (demo: (1,3,4,0) (6,0,5,0) (2,8,9,0)).
```
for i in 0..49: r = iac[i]
  if r.used==1 && ((8a8e==r.obj1 && 8a92==r.obj2) || (8a8e==r.obj2 && 8a92==r.obj1)):  // either order
     if r.mode==0: SUBINV(r.obj1); SUBINV(r.obj2)
     if r.mode==1: SUBINV(r.obj1)
     if r.mode==2: SUBINV(r.obj2)
     // mode 3: remove nothing
     ADDINV(r.newObj)
     SPEAK(2004)          // YESACINVINV  (message slot 4)
     return
SPEAK(2001)               // NOACINVINV   (message slot 1)
```
Messages: id>=2000 -> DS:3bfa + (id-2000)*250; slot0 default "¡¡¡ ATENCION !!!^Este texto no esta
definido.", 1 NOACINVINV, 2 NOACINVOBJ, 3 unused, 4 YESACINVINV (level overwrites non-empty ones).
Matching uses object numbers, first matching record wins, only the first 50.

### use on THING (0x7e0b) — ACINVTH table DS:6b82, 30 x {i16 mode, i16 inv, i16 thing, i16 val}
```
for i in 0..29: e = acinvth[i]
  if e.mode != 0 && ((e.inv==8a8e && e.thing==8a92) || (e.inv==-100 && e.thing==8a92) ||
                     (e.inv==8a8e && e.thing==-100) || (e.inv==-100 && e.thing==-100)):
     8a93 = (u8)e.val            // QUIRK: only low byte kept
     fcca = 0
     mode 1: fcc9=0x66            (walk to THING GOTO, face GOTO dir, run ACTION val)
     mode 2: fcc9=0x66, fcca=3    (stay, run ACTION)
     mode 3: fcc9=0x66, fcca=1    (face THING GOTO dir without walking, run ACTION)
     mode 4: fcc9=0x68            (walk, face, SPEAK text val)
     mode 5: fcc9=0x68, fcca=3    (stay, SPEAK)
     mode 6: fcc9=0x68, fcca=1    (face, SPEAK)
     goto done                    (first match in table order wins)
no match: 8a93=0; fcc9=0x66; fcca=0          // walks to the thing, faces, then NOACINVOBJ
done: fcdf = 8a92 (target thing); 8a90 = 0
```
Execution, state machine 0x7937 (each frame, for fcc9 0x66/0x68, thing t = fcdf):
```
fcca==0: GOXY(thing[t].gotoX, thing[t].gotoY); fcca=1
fcca==1 && not walking ([fcce]==0): face thing[t].gotoDir (0 left:mov=[d1],mirror=1; 1 right:[d1],0;
          2 front:[d0]; 3 back:[cf]); fcca=3
fcca==3: 8a8c = 0  (object leaves the hand)
     0x66: if 8a93 != 0: fcc9 = 8a93 (that ACTION then runs via the generic fcc9 path, fcca=0)
           else: SPEAK(2002); fcc9=0
     0x68: SPEAK(8a93 != 0 ? 8a93 : 2002); fcc9=0
```
QUIRK: ACTION 0 / TEXT 0 in an ACINVTH entry is indistinguishable from "no match" -> NOACINVOBJ.
The inventory object is never removed automatically; scripts use SUBINV.

## Uncertain
- 8a8f/drawnFlag/cacheObj fields look like an unfinished redraw cache; behaviour above is what runs.
- Exact palette-merge semantics of DS:3180 belong to the palette code (0x2210), only sketched here.
- [fd46]=2 + busy-wait = "hide mouse cursor synchronously" (ISR state machine at ~0x3dd8); a port can
  ignore it.


---------------------------------------------------------------------------------------------------
# APPENDIX B — MENU and savegames (full, from spec_menu.md)

# MENU (load / save / quit) and SAVEGAME._N_ — RUNDAT.EXE spec

All addresses: code = linear in RUNDAT.bin (near call targets wrap mod 0x10000), data = DS offsets.
Screen = 320x200 8bpp. "backbuf" = far [DS:3792] (work screen), "VGA" = far [DS:37a6] (A000:0000),
"blit(x,y,w,h)" = 0x4149 copy backbuf rect -> VGA (also composites cursor). Font text = 0x70d0(x,y,str,color,border,...)
(font/renderer: see spec_ui.md), textwidth = 0x78a8.

## 1. Keyboard state ([DS:dc] = var25)

Custom INT 9 handler at 0xc42d (installed by 0xc3d0(keytab=DS:2200, lastkey=DS:dc), removed by 0xc415):
```
sc = in(0x60)
if sc < 0x80: *(u16*)(DS:2200+sc) = 1 ; DS:dc = sc      // word write -> also clears key sc+1's byte (quirk)
else:         *(u16*)(DS:2200+(sc&0x7f)) = 0              // dc NOT cleared on release
EOI, iret (BIOS handler NOT chained -> no BIOS key buffer while installed)
```
So `[dc]` = raw PC/XT make scancode of the last pressed key. Relevant codes: 0x39 SPACE (opens menu),
0x01 ESC, 0x1f 'S'.

## 2. How the menu opens

* Main loop (0x5690, every frame): `if [dc]==0x39 && byte[fd04](var28)==1: [98]=1; MENU(); [dc]=0; [98]=0; [1bbe]=0xff`.
  i.e. **SPACE** opens it (not ESC), only if var28 = 1 (default 1, set in init at 0xbd6b).
* Opcode 52 MENU() (handler at 0x3711): `[98]=1; MENU(); [dc]=0; [98]=0;` then continues the script
  (does NOT check var28).
* `[98]=1` = "paused": the 100 Hz timer ISR (0x3f64) skips decrementing the DELAY counters (DS:118c/1190),
  the text timer (fa2e) and fccf/fcd0 while it is set, so speech/delays freeze during the menu.

## 3. Cursor/ISR handshake used everywhere in the menu

`fd40` = MMOUSE mode (2 = cursor hidden, buttons off; 0 = normal). `fd46` = command to the timer-ISR cursor code:
`fd46=5; while(fd46==5);` = "let the ISR erase+redraw the cursor once" (ISR resets it to 0);
`fd46=2; while(fd46==2);` = "ISR removes the cursor and parks (state 3)" — done before file I/O.
(Exact ISR cursor logic belongs to the mouse spec; for a port: hide cursor while drawing/doing I/O, show after.)

## 4. MENU() 0x9ae1 — pseudo-code

```
MENU():
  [fd2b]=0; [9b]=0
  save10f2=fd46; save10f0=fd48; save10ee=fd40; fd2d = fd2c (current verb)
  MOUSEACTION(0x65)              // -> verb 5 = menu cursor (6th DEFMOUSE row), keeps hotspot position
REDRAW:                           // 0x9b0e, also re-entered after Quit-cancel / Load-cancel / Save-cancel
  palCur = copy(DS:2580,768); palOrig = copy(DS:2280,768); DS:2280 = DS:2580
  if 0 < var33(fbdf) < 255:       // default 250
     MODPAL(first=0, count=var33, amount=-20, src=DS:2280, dst=DS:2580)   // 0x587: each 6-bit RGB comp +(-20), clamp 0..63
  fd40=2; fd46=5; wait
  draw_frame()                    // 0xa46e, see 4.1
  draw 4 buttons (4.2) + labels CARGAR / GUARDAR / SALIR / JUGAR in var40 (fbe9), border var37 (fbe3)
  set_vga_palette(DS:2580)        // 0xb7f -> colours 0..var33-1 darkened, menu colours 250..254 untouched
  DS:2580 = palCur; DS:2280 = palOrig   // arrays restored (darkening only on the DAC)
  blit(0,0,320,200)
  fd40=0; fd46=5; wait            // cursor visible, menu active
LOOP:                             // 0x9e04
  if mouse_button(0) || mouse_button(1):     // 0x688, left or right (level, not edge)
     hx = fd4a+fd5e ; hy = fd4c+fd60          // cursor hotspot
     if 207<=hx<=267:
       if  89<=hy<=101: fd40=2; fd46=5; draw "JUGAR"(var39 fbe7) ; blit full; goto EXIT     // resume
       if  75<=hy<= 88: QUIT_DIALOG
       if  61<=hy<= 73: fd40=2; fd46=5; wait; draw "GUARDAR" in var39; blit;
                        if SAVE_DIALOG()==1 goto EXIT else goto REDRAW
       if  46<=hy<= 60: fd40=2; fd46=5;       draw "CARGAR" in var39;  blit;
                        if LOAD_DIALOG()==1 goto EXIT else goto REDRAW
  if [dc]==1 (ESC): [dc]=0 (busy-loop until 0); goto EXIT
  goto LOOP
  (y=74 is a dead line between GUARDAR and SALIR hit boxes; no hover highlight exists — the var39 colour is
   only shown on the clicked label, which is never un-highlighted because the screen is redrawn/left.)

QUIT_DIALOG (0x9efc):
  fd40=2; fd46=5; draw "SALIR" in var39; blit full
  // drawn DIRECTLY on VGA (not backbuf), fixed colours:
  fillrect(0,93 .. 320,108) colour 0          // a2d5(x1=0,y1=0x5d,x2=0x140,y2=0x6c): x<320, y 93..107
  rect_outline(0,93,320,108) colour 255       // a213 (x=320 column clipped away -> no right edge)
  text(125, 96, "¿Salir (S/N)?", colour 255, border 0)
  [dc]=0; while [dc]==0;                       // any key
  if [dc]==0x1f ('S'): fatal(0) -> program exit (0xa836 code 0: normal shutdown)
  blit full (erase prompt); goto REDRAW        // any other key = cancel

EXIT (0xa18d):
  fd40=2; fd46=5; wait
  MOUSEACTION(fd2d)                             // restore verb/cursor
  render scene (0x65a0); if fcf0 != -1: STEXT redraw ada8(fcf0)
  blit full
  fd40=save10ee; fd46=5; wait; fd46=save10f2; fd48=save10f0
  [dc]=0
  set_vga_palette(DS:2580)                      // undarkened (or the loaded game's palette)
```

### 4.1 draw_frame 0xa46e (into backbuf)
Colours: var36 fbe1 "Bordes", var37 fbe3 "Fondo", var38 fbe5 "Bordes2", var34 fbeb title, var35 fbed title border.
Primitives: `fill(x1,y1,x2,y2)` = 0xa2d5, x in [x1,x2), y in [y1,y2). `rect(x1,y1,x2,y2)` = 0xa213:
top row y1 x∈[x1,x2), sides x1 and x2 for y∈[y1,y2), bottom row y2 x∈[x1,x2] (i.e. inclusive outline);
with x1==x2 or y1==y2 it degenerates to a line. Both clip to 320x200.
```
fill(48,10, 270,132, var37)                          // panel 0x30,0x0a - 0x10e,0x84 (exclusive)
text(160 - w/2 - 2, 13, title DS:9d, var34, var35)   // w = textwidth(title)
rect(48,10, 270,132, var38) ; rect(49,11, 269,131, var38)        // outer 2px, light (top/left)
line (50..269,131) var36 ; line (49..269,132) var36               // bottom 2px dark
line (269, 11..131) var36 ; line (270, 10..132) var36             // right 2px dark
rect(51,24, 205,129, var36)                          // slot list box, sunken: dark top/left
line (51..205,129) var38 ; line (205,24..129) var38  // light bottom/right
```
Title: NIVEL.TIT (EDGAME.DAT entry 11): 30-byte hdr, then **raw** 0x32 bytes -> DS:9d (each byte +0xa5 until NUL,
i.e. stored = plain-0xa5, padding 0xa5->0x00), then raw u16 -> DS:fd62 = title check value.
For the demo: "Peter's Adventure I", fd62=0x06d1 = byte sum of the plaintext title (compiler-computed;
the runtime only compares it).

### 4.2 buttons (main menu; save/load reuse buttons 2-3)
All x 207..267 (0xcf..0x10b), label centred at x = 237 - textwidth/2 (0xed), colour var40, border var37.
| # | rect y (outline) | label y | main menu | save dlg | load dlg | hit-test y (hotspot) |
|---|---|---|---|---|---|---|
| 1 | 46..60 | 49 | CARGAR | – | – | 46..60 |
| 2 | 61..74 | 63 | GUARDAR | OK | – | 61..73 (main) / 61..74 (dlg) |
| 3 | 75..88 | 77 | SALIR | ANULAR | ANULAR | 75..88 |
| 4 | 89..102 | 91 | JUGAR | – | – | 89..101 |
Button look: `rect(207,y1,267,y2,var38)` then `line(207..267,y2,var36)` and `line(267,y1..y2,var36)` (raised).
Selected/clicked label colour = var39 (fbe7). Defaults (init 0xbcac): var36=250, var37=251, var38=252,
var39=253, var40=254, var34=254, var35=253, var33=250.

### 4.3 save slots
8 slots, files `SAVEGAME._1_` .. `SAVEGAME._8_` (char 10 of "SAVEGAME._1_" = '1'+slot) in the current dir.
Slot name table DS:1bc8 + i*0x37 (55 bytes), used flag DS:1bc0[i].
`scan_slots` 0xa318: for each i: fopen rb; if fails -> name "- VACIO -", flag 0. Else read b0,b1;
if (b0==0 || b1==0xff) [sic, OR not AND]: skip 4 bytes, RLE-read 0x37 bytes name, flag 1, fclose;
else name "- VACIO -", flag 0 (file left open — leak).
`draw_slot(i,str,col,border)` 0x9603: `fill(52, 25+13i, 205, 37+13i, var37)`, `text(52, 26+13i, str, col, border)`,
blit(52, 26+13i, 153, 16). Normal colour var40/border var37, highlighted var39/var37.
`draw_all_slots` 0xa420: draw_slot(i, name, var40, var37) for i=0..7, then blit full screen.
`slot_under_cursor(prev)` 0x8cd8: first i with 51<=hx<=205 and 25+13i<=hy<=37+13i, else returns prev
(rows overlap by 1px; lower index wins). Only called when a mouse button is down.

### 4.4 LOAD_DIALOG 0x8d2e (returns 1 = loaded)
```
fd40=2; fd46=5; wait; scan_slots(); draw_frame(); button 3 + "ANULAR"; draw_all_slots()
fd40=0; fd46=5; wait; sel=100; [dc]=0
loop:
  while sel==100 and [dc]!=1:
     if button down: if hit button3 (207..267, 75..88): fd40=2; fd46=5; wait; "ANULAR" var39; blit; return 0
                     sel = slot_under_cursor(sel)
  if [dc]==1: [dc]=0; (same ANULAR flash) return 0
  draw_slot(sel, name, var39, var37)
  if flag[sel]==1: fd46=2; wait; LOAD(sel); return 1
  draw_slot(sel, name, var40, var37) x3 ; restart dialog loop with sel=100 (cursor re-shown)
```
### 4.5 SAVE_DIALOG 0x840f (returns 1 = saved)
```
fd40=2; fd46=5; wait; scan_slots(); draw_frame(); buttons 2 "OK", 3 "ANULAR"
PHASE1: draw_all_slots(); fd40=0; fd46=5; wait; sel=100; [dc]=0
  while sel==100 && [dc]!=1:
     if button down:
        if hit OK (61..74): flash OK (var39 then var40, two full blits) — no effect
        if hit ANULAR (75..88): fd40=2..; ANULAR in var39; blit; return 0
        sel = slot_under_cursor(sel)
  if [dc]==1: [dc]=0; fd46=2; wait; flash ANULAR (var39 then var40); [dc]=0; return 0
PHASE2 (text entry): uninstall INT9 (0xc415) -> BIOS keyboard
  buf = flag[sel] ? name[sel] : ""     (max 0x3e bytes on stack)
  len = strlen(buf); buf += "_"       // '_' = text cursor
  draw_slot(sel, buf, var39, var37)
  loop:
    if button down:
       hit OK (61..74):   OK var39, blit; buf[len]=0 (drop '_');
                          if buf!="": name[sel]=buf; reinstall INT9; fd46=2; wait; SAVE(sel); return 1
                          else: OK var40, blit
       hit ANULAR (75..88): ANULAR var39, blit; reinstall INT9; return 0
       s = slot_under_cursor(sel); if s!=sel: sel=s; draw_all_slots(); restart PHASE2 for new slot
    if kbhit():                 // BIOS 0xe2c4 / getch 0xe167
       c = getch()
       Enter(0x0d): buf[len]=0; OK var39 blit; if buf!="": name[sel]=buf; fd46=2; SAVE(sel); reinstall INT9; return 1
                    else OK var40 blit (continue)
       Esc(0x1b):   reinstall INT9; goto PHASE1 (slot names redrawn from files' names, edit discarded)
       0:           getch() (discard extended key)
       Backspace(8): if len>0: len--; buf[len]=0; buf+="_"
       printable: if (glyph(c).width>0 || c==' ') && textwidth(buf w/o '_') < 138 && len < 54:
                    buf[len++]=c; buf+="_"
       on change: draw_slot(sel, buf, var39, var37)
```
(Characters accepted = those with a non-empty glyph in the font, cp850; e.g. ñ á ¿ if the BIOS delivers them.)

## 5. SAVEGAME._N_ file format

Header written raw (fputc/fwrite):
```
u8 0x00, u8 0xFF, u8 [fd65]=2, u8 [fd64]=3, u16 [fd62] (title check)
```
Then 38 blocks, each independently RLE-encoded (writer 0x1667: runs of 2..63 equal bytes, or any single byte
>=0xC0, are written as (0xC0+n, byte); other bytes literal; reader 0x1709: c>0xC0 -> run). Generic routine
0x9693(fp, slot, mode 1=write / 0=read) handles them in this order:

| # | DS addr | size | contents |
|---|---|---|---|
| 1 | 1bc8+slot*0x37 | 0x37 | slot name (NUL-terminated) |
| 2 | fbce | 0x172 | engine globals fbce..fd3f (most system vars 0-46, text/speak state, verb fd2c, ...) |
| 3 | 6b82 | 0xf0 | ACINVTH |
| 4 | 4532 | 0x3e8 | COORANIM |
| 5 | 6c72 | 0x1b80 | LIMITMOV |
| 6 | 87f2 | 0x230 | LIMIT |
| 7 | 43fa | 0x34 | gfx PCX names |
| 8 | 9823 | 0x61a8 | text pool |
| 9 | 962f | 0x1f4 | text offsets |
| 10 | far [698a] | 0x2710 | bytecode |
| 11 | 6926 | 0x64 | ACTION entry table |
| 12 | fd40 | 0x22 | mouse/cursor state fd40..fd61 (see note) |
| 13 | 8b67 | 0xac8 | THINGS |
| 14 | 8b49 | 0x1e | draw order |
| 15 | 8a95 | 0xb4 | inventory: 30 x 6 bytes {u8 used, u8 ?, u16 object, ...} |
| 16 | 1dd0 | 0x1f4 | ANIMATION table |
| 17 | 442e | 0x104 | FILE names |
| 18 | f9cb | 0x203 | 5 speech text line slots (5 x 0x67) |
| 19 | 435a | 0xa0 | FILE offset table |
| 20 | 433a | 0x20 | gfx offset table |
| 21-27 | 2280, 2580, 3480, 3180, 2e80, 2b80, 2880 | 7 x 0x300 | palettes (orig, current, MOVS, ?, DIBUJOS, ESCENARIO, ?) |
| 28 | 4338 | 2 | ? |
| 29 | 200c | 0x190 | player frame table (100 x {x,y}) used by SPEAK restore |
| 30 | 4334 | 4 | player w,h |
| 31 | 1fc4 | 0x48 | current cursor set (6 x {x,y,w,h,hotx,hoty}) |
| 32 | 8a25 | 0x70 | 8a25..8a94: var11/12/24, info-bar state, selected inventory object (8a8c-8a8f) |
| 33 | 40dc | 0x258 | SELECTEXT |
| 34 | 3bfa | 0x4e2 | 5 x 250 messages |
| 35 | 3a02 | 0x1f4 | DRAWANIM table (25 x 20 bytes) |
| 36 | 389a | 0x168 | SCALEMOV |
| 37 | 37aa | 0xf0 | SHADOWMOV |
| 38 | 1194 | 0xa28 | DEFFRAMES |

Then raw: 2000 x u16 = `*varptr[i]` for i = 0..1999 (varptr table DS:491e; covers system vars, thing vars and
the persistent 1000..1999 buffer — redundant with the blocks above for 0..999), then 13 raw bytes DS:cf..db
(player movement-index table used by SPEAK; one fputc each). No trailer.

Notes:
* Before writing, SAVE (0x9435) calls MOUSEACTION(fd2d) (so the saved verb/cursor is the in-game one) and
  `0xc2b4(0)` which re-encrypts strings in memory (thing names +0x0D, text pool +0x0F (all 25000 bytes),
  FILE names +0x19, gfx names +0x2D); after writing `0xc2b4(1)` decrypts again. So the strings in the file are
  in the encrypted (level-file) form. Open failure ("wb") is silently ignored.
* Block 12 on write: fd40/fd46/fd48 are first restored to the pre-menu values (10ee/10f2/10f0), written, then
  set to fd40=0, fd46=5. On read: the loaded values are stashed into 10ee/10f2/10f0 (so MENU's EXIT restores the
  loaded ones) and fd40=2, fd46=5.

### LOAD 0x8f90(slot)
```
0xa4b (?, sound/screen prep); fopen "SAVEGAME._N_" rb (failure: return silently)
if b0!=0 || b1!=0xff || b2!=fd65 || b3!=fd64: fatal 0x61 "El archivo '%s' no es un SAVEGAME o es de una versión anterior"
if u16 != fd62: fatal 0x60 "Se ha intentado cargar un SAVEGAME no perteneciente a '%s'"
read blocks (0x9693 mode 0); read 2000 vars through varptr; read 13 bytes -> DS:cf..db; fclose
0xc2b4(1)                                         // decrypt strings
reload ESCENARIOPCX (name DS:43fa) -> [37a2] background, palette -> DS:2b80 (fatal 0x67 on failure)
0x685f()                                          // (scene prep; other spec)
for i in 0..24: if DRAWANIM[i].active (byte DS:3a0f+20i)==1:
     load its PCX (name at DS:3a02+20i) into DIBUJOS buffer [379a] (pal 2e80) and re-stamp it into the
     background with 0x429e at (3a12,3a14) using COORANIM[3a10] — i.e. DRAWANIMs are replayed
reload DIBUJOSPCX (DS:4407) -> [379a] pal 2e80 ; MOVSPCX (DS:4414) -> [379e] pal 3480
for i in 0..29: if inv[i].used==1: inv[i].used=0; ADDINV(inv[i].object)   // rebuild inventory graphics/order
if sound present ([3785]==1): [3784]=0; 0xc5c2(8); reset timer rate (0x10de7([dd]))
   if [fca6]==1: [fca6]=0; PLAYCMF(fc9e..fca4)    // restart the music that was playing
if fbf3==2: player gfx source = MOVS buffer [379e]; if fbf3==3: = DIBUJOS buffer [379a]  (PMOV state)
render scene (0x65a0); [1bbe]=0xff; if fcf0!=-1: ada8(fcf0) (re-open pending STEXT); 0x6e9b()
blit full; 0xac5(DS:2580) (palette, probably fade-in); mouse x range 0..321-curW, y range 0..200-curH
```
Level number is not stored explicitly in a block of its own: it is inside block 2 (fc9c, level number,
is in fbce..fd3f) — the level is NOT reloaded from EDGAME.DAT; everything level-related comes from the save.

### Port guidance
Binary compatibility not required: an equivalent save must capture all of the above state (complete level
data incl. bytecode/texts as possibly modified, things, inventory, all 2000 vars, palettes, cursor set,
DRAWANIM list, speech lines, STEXT pending id fcf0, music playing state, PMOV state), then on load rebuild the
background by reloading ESCENARIO and replaying active DRAWANIMs.

## Uncertain
* Exact meaning of the 0x4338, 0x3180, 0x2880 blocks and of 0xa4b / 0x685f / 0x6e9b / 0xc5c2 (outside scope).
* fd46 ISR states described from a quick read only (5 = redraw cursor once -> 0; 2/4 = remove cursor -> 3).
* 0xb7f is "set VGA DAC from 768-byte buffer" (two 0x180-byte halves with retrace waits); 0xac5 assumed fade-in.
