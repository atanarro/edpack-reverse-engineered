# RUNVGA.EXE v1.03 (EDPACK v1): main loop, input, UI — spec

Source: unpacked `RUNVGA.EXE` (Borland C++ 1991, one near code segment, CS base = image start,
DS = image + 0xfec0; in the reference emulator DS linear = (0x1000+0xfec)*16). All addresses
below are **DS offsets** (data) or **code offsets** (functions, written `fn 0x....`). Verified
against the unicorn reference emulator (`scratchpad/re/ui_play.py`) with *Las Aventuras de Manolo
Kabezabolo*; screenshots in `img/`.

Compared with v2 ([`../v2-rundat/01-main-loop.md`](../v2-rundat/01-main-loop.md), [`../v2-rundat/04-text-ui-inventory-menu.md`](../v2-rundat/04-text-ui-inventory-menu.md)): v1 has **no verbs and no
verb cursor**. Left and right mouse buttons have fixed meanings, the inventory is a permanent 8-slot
bar at the bottom of the screen, and the label line is fixed at y=155. Rendering uses a "work"
buffer composited over the background with dirty rectangles; the cursor is drawn by the timer ISR
straight into VGA memory.

## 1. Globals referenced here

| DS | size | meaning |
|---|---|---|
| 0x94 | b | 1 while the frame is being composed (ISR does not read the mouse) |
| 0x95 | b | 1 while the SPACE menu runs (ISR does not count down text/anim/walk timers) |
| 0x96 | b | last make-scancode (written by the keyboard ISR, cleared by consumers) |
| 0x97 | w | timer rate in Hz for the PIT (init 50) — `fn 0x80c(hz)` programs divisor 1193180/hz |
| 0x99 | b | data mode: 0 = `-n` loose files (debug overlay on), 1 = `-f` single archive |
| 0x98a4 | 128 b | keyboard state, byte per scancode (ISR writes a **word** 1/0 at 0x98a4+sc, so the next byte is also cleared on press) |
| 0x3536/0x3537 | b,b | from SETUP.CFG byte 1 (speed 0..4): (15,12) (8,6) (4,3) (3,2) (1,0). 0x3537 = animation script period (ticks); 0x3536 = player step period (used by player update) |
| 0x3538 | b | from SETUP.CFG byte 2 (text speed 0..4): 16, 12, 8, 4, 2 (ticks per character) |
| 0x903e | w | speech countdown (ticks) |
| 0x92a1 | b | player-walk countdown (dec by ISR) |
| 0x92a2 | b | animation-script countdown (dec by ISR, reload 0x3537) |
| 0x92a8 | w | script countdown (dec by ISR; used by interpreter ops) |
| 0x92b0 | w | **var 10**, cursor/input mode: 0 normal; 1 cursor shown, input only skips text; 2 = "hide requested" (ISR hides cursor, then sets 3); 3 hidden, no input; 4 = hide requested (same as 2) |
| 0x92b2 | w | last mode the ISR drew with |
| 0x92b4/0x92b6 | w,w | cursor top-left (= int33 position) |
| 0x92b8/0x92ba | w,w | cursor position drawn last |
| 0x92ac/0x92ae | w,w | cursor sprite source x,y in the gfx2 sheet (buffer [0xde0b]) |
| 0x92bc/0x92be | w,w | cursor sprite w,h |
| 0x92c4/0x92c6 | w,w | cursor hotspot; **click point = (0x92b4+0x92c4, 0x92b6+0x92c6)** |
| 0x927c / 0x927e | w,w | requested level / loaded level |
| 0x929c / 0x929d / 0x929e | b,b,b | current action number, its pc, its cmp flag (the "click action" script slot) |
| 0x929f | b | first inventory slot shown (scroll) |
| 0x9280 | w | hovered inventory slot + 1 (0 = none) |
| 0x9202 | b | hovered thing + 1 (0 = none) |
| 0x9282 | b | **var 7**, 1 = speech/text sequence active |
| 0x9297 | b | 1 = the active text is the player's (SPEAK), 0 = PTEXT |
| 0x35a0..0x35a7 | | inventory "held object" state, see §5 |
| 0x353d/0x353e | b,b | label state now/last drawn: 0 none, 1 shown, 2 rebuild requested; 0xff forces redraw |
| 0x92a4 | b | 1 = DAC must be reloaded from the current palette 0x9924 this pass |

Variable pointer table (var n → far pointer at 0x0ea4+4n), set by main (`0xc00c`): var0 0x91f5
(player visible), 1 0x91da (player frame), 2 0x91dd (player x), 3 0x91df (player y), 4 0x91e9
(player w), 5 0x91eb (player h), 6 0x91f7 (player mirror), 7 0x9282 (speaking), 8 0x92a5 (player
text colour, init 1), 9 0x92a6 (text border colour, init 15), 10 0x92b0 (cursor mode), 11 0x3539
(label colour, init 2), 12 0x353b (label border, init 15). Vars 30+20t..: per-thing fields for 25
things (0x39c7 state, 0x39b1 layer, 0x39e4 type, 0x39e6 action, 0x39e2 take-action, 0x39b5 x,
0x39b7 y, 0x39c9 mirror, 0x39da walk x, 0x39dc walk y, 0x39de dir, 0x39e0, 0x39e8 look text).
Vars 1000..1999 → a separate 2000-byte far buffer [0xea0].

## 2. Timing

* PIT reprogrammed to [0x97] = **50 Hz**; the ISR at `fn 0x36ee` runs every tick (it also
  chains the old int 8 handler twice: at entry and at exit).
* Main loop pass: everything, then **two vertical-retrace waits** (`fn 0xc99d` ×2; ≈ 28.5 ms at
  70 Hz), **or**, when [0x92a4]==1, `fn 0xcddf(0x9924)` instead (DAC load in two halves of 384
  bytes, each after one retrace wait) and [0x92a4]=0.
* ISR per tick, in order:
  1. if [0x94]!=1: read mouse (`fn 0x8f0`, values recorded by the int33 handler) → [0x92b4],[0x92b6].
  2. if mode ∈ {2,4}: restore the screen under the old cursor (`fn 0x38df`, see §7), remember
     position, mode = 3, [0x92b2] = 3.
  3. if mode != 3 and (cursor moved or [0x92b2] != mode): (`-n` mode only: restore rect
     (0,0xa0..) w=0x46 h=0xb) restore under the old cursor, draw the cursor sprite at the new
     position (`fn 0xc7e1`, colour 0 transparent) **directly into VGA memory**; [0x92b2] = mode.
  4. if [0x95]==0: if speaking and [0x903e]>0: [0x903e]--; [0x92a1]-- if >0; [0x92a2]-- if >0.
  5. [0x92a8]-- if >0 (always).
  6. Music loop: if sound present [0xddf2]==1, music status word [0xddf5]==0 (ended) and
     [0xddf1]==1 (music wanted) → restart the CMF (`fn 0xc644(6)`) and re-program the PIT to
     [0x97] (the FM driver's clock call changes it). So CMF music loops forever.

## 3. Main loop `fn 0x5037`

Called once by main after init; returns only when **Esc and Ctrl are both held**
([0x98a5] and [0x98c1] non-zero), then main calls `fn 0xb9c9(0)` (normal exit: stop sound,
restore int 8/9, PIT, mouse, text mode, print the version banner).

Per pass:

1. `if [0x927c] != [0x927e]`: **change level** `fn 0x11c9` (§8); [0x927e] = [0x927c].
2. `fn 0x7d26` — animation scripts: only when [0x92a2]==0: for each of 50 entries (10 bytes at
   0x94b0: +0 active word, +2 script start, +4 pc, +6 cmp flag) with active==1 run the
   interpreter (`fn 0x2464(start, &pc, &flag)`); when it returns 0 (END) pc=0, active=0. Then
   [0x92a2] = [0x3537].
3. `fn 0x764e` — **click-action dispatcher** (§5.4).
4. if [0x9282]==1: `fn 0x7513` — text sequencer (§6).
5. `fn 0x4842` — **input** (§5).
6. `fn 0x3e6a` — player update (walking/animation; not covered here).
7. if [0x353d]==2: `fn 0x4d56` — rebuild the label (§5.3).
8. `fn 0x4f83` — auto layering: for each thing with state != 0 and layer not 2/3:
   layer = (thing.y + thing.h >= player.y + [0x982]) ? 1 (in front of the player) : 0.
9. if [0x96]==0x39 (SPACE): [0x95]=1, menu `fn 0xabda` (§9), [0x96]=0, [0x95]=0.
10. if 0 < [0x929c] < 50 or speaking: label off ([0x353d]=0, [0x353e]=0xff, [0x3547]=0).
11. **Compose** (§7) with [0x94]=1 … 0, then the retrace wait / palette load, then copy the
    changed rectangles to VGA and update the "old" copies.
12. if [0x99]==0 (`-n`): debug overlay `fn 0x6537` (numbers drawn at y 150/160).

## 4. Input primitives

* **Mouse**: int33 fn 0 reset, fn 7/8 ranges (x 0..319−cursor w, y 0..200−cursor h, i.e. the
  driver returns screen pixels, **no ×2**), fn 4 set position (160,100 at start), fn 0Ch installs
  `fn 0x8b0` with mask 0x15; the handler only stores CX,DX on movement events.
* **Button test** `fn 0xced7(b)`: int33 fn 5 (press count for button b); returns 1 only if the
  count is non-zero **and the current button status (AX) == b+1**, i.e. that button alone is
  still held at the poll. A click released before the next pass is lost; holding both buttons
  registers neither. (Implementation: report a click on the pass after the press as long as the
  button is still down; the poll happens once per pass for each button, left first.)
* **Keyboard**: own int 9 handler `fn 0xcff5`: make code → word 1 at 0x98a4+sc and byte [0x96]=sc;
  break code → word 0 at 0x98a4+(sc&0x7f). EOI, no BIOS chaining. The save dialog temporarily
  restores the BIOS handler (`fn 0xcfdd`) to read text with getch.

## 5. Input handling `fn 0x4842` (once per pass)

```
if mode == 2 || mode == 3: return
if speaking || mode != 0:
    if button(0) || button(1): end_text()          // fn 0x762b: all lines off, [0x903e]=0 -> next text
    return
if 0 < [0x929c] < 50: return                         // a click action script is running
[0x9200]=0; [0x9202]=0; [0x9280]=0
inventory_hover()  /* fn 0x67a1 */ ; thing_hover() /* fn 0x4c44 */
if held==0xff && pend==0xff: held=0; pend=0; label=2
if held==1 && pend==1:                               // a combination was clicked last pass
    if [0x35a5]==100: use_on_thing() /* 0x7c02 */; pend=0
    else:             combine()      /* 0x7a9a */; held=0xff; pend=0xff
if button(0):  LEFT
    p = click point
    if 0<=p.x<=13 && p.y>173:   if first>0: first--; redraw_inventory(); return
    if 307<=p.x<=320 && (cursor.y + hotspot.X) > 173:          // bug: uses 0x92c4 for y
                                if slot[first+8].used: first++; redraw_inventory(); return
    common_click()
    if hover_thing && held!=1 && !speaking: thing_left_click()   // fn 0x7c99
    if hover_inv && held!=1 && !speaking: SPEAK(text 0x31 + hover_inv)   // = 50+slot, the object description
if button(1):  RIGHT
    common_click()
    if hover_inv && held!=1 && !speaking:            // pick the object up
        [0x35a1]=slot; [0x35a2]=object id; held=1; label=2
    if held==0 && hover_thing && !speaking: [0x35a2]=thing; [0x929c]=101; [0x929d]=0

common_click():
    if !speaking && [0x929c]!=0 && mode!=2: [0x929c]=0          // cancel a running click action
    walk state reset: [0x91ff]=[0x91fd]=[0x91fe]=0; [0x9204]=[0x9206]=-100
    if held && !speaking && !hover_inv && !hover_thing: held=0  // click on nothing drops it
    if held!=1 && [0x929c]==0 && !hover_thing && p.y < 148: GOXY(p.x, p.y)   // fn 0x7dab
    if held && !speaking:
        if hover_inv:   [0x35a5]=slot; [0x35a6]=object id; pend=1; label=2
        if hover_thing: [0x35a5]=100;  [0x35a6]=thing;     pend=1; label=2
```
held = [0x35a0] (0 none, 1 holding, 0xff "just combined"), pend = [0x35a4]. Note the right
button **also** walks (GOXY) when nothing is hovered, exactly like the left.

### 5.1 Hit tests
* Inventory slot i (0..7) is hovered when `13+37i <= p.x <= 48+37i` and `p.y >= 173` and slot
  [0x929f]+i is used; then [0x9280] = [0x929f]+i+1. If holding and that slot holds the held
  object, it does not count (0x9280=0). Label rebuild requested when the hovered object changes.
* Things (0..29, in index order, first match wins): `x <= p.x <= x+w`, `y <= p.y <= y+h`
  (inclusive) and state ∈ {1, 3}. Sets [0x9202]=t+1 and requests a label rebuild if it changed
  ([0x3547] = last labelled thing). If nothing is hovered and not holding: label off.

### 5.2 Thing record (0x45 bytes at 0x39a5, 30 things) — fields used by the UI
+0 src x, +2 src y (in the gfx1 sheet), +0x0c layer (0/1 auto behind/front, 2 fixed behind,
3 fixed front), +0x10 x, +0x12 y, +0x18 w, +0x1a h, +0x22 **state** (0 off, 1 visible+hotspot,
2 visible only, 3 hotspot only/invisible), +0x24 mirror, +0x26 name (15 bytes),
+0x35 walk-to x, +0x37 walk-to y (−100 = don't walk), +0x39 facing dir (0..3, −100 none),
+0x3d take action, +0x3f **type**, +0x41 action, +0x43 look text. (+0x04/+0x06, +0x14/+0x16,
+0x1c/+0x1e, +0x20 are the "last drawn" copies of src, pos, size and state.)

### 5.3 Label line `fn 0x4d56` (string at 0x3550)
Text = (held ? held name + " & " : "") + (hovered inventory object name, if not holding or it
differs from the held one) + (hovered thing name). Width = Σ advances (" " and "&" use the full
glyph width `w`, not `w−1` — quirk). Drawn at **x = 160 − w/2, y = 155 (0x9b)**, height 10, colour
[0x3539] (var 11, init 2) with border [0x353b] (var 12, init 15). If the text is empty nothing is
built. [0x353d]=1, [0x353e]=0xff. See `img/ui-label.png`, `img/ui-combine.png`.

### 5.4 Click actions `fn 0x764e` (states 100..104 in [0x929c], sub-state [0x929d])

Left click on thing t (`fn 0x7c99`), by **type** (+0x3f):
* 0: action 100 from sub-state 0 — walk to (+0x35,+0x37), face, say look text (+0x43);
* 1: action 100 from sub-state 2 — say the look text without walking;
* 2: action 104 — walk, face, then [0x929c] = thing action (+0x41) (runs that script);
* 3: [0x929c] = thing action immediately (no walk).

States:
* **100** (thing [0x35a2]): sub 0: GOXY(walk x, walk y), sub=1. Sub 1 (when [0x92a0]==0, i.e.
  arrived): face by dir: 0 → frame [0x9c] mirrored ([0x91f7]=1), 1 → [0x9c] unmirrored,
  2 → [0x9b], 3 → [0x9a]; sub=2. Sub 2: SPEAK(look text); [0x929c]=0.
* **104**: like 100 but at sub 2: [0x929c] = thing action, sub 0.
* **101** (right click on thing): walk+face, then if take action (+0x3d) != 0 → run it, else
  SPEAK(fixed message 1).
* **102** (held object used on thing [0x92ab], with walking): walk+face to the thing, sub 3:
  held=0; if [0x35a7] (action from the table) != 0 → run it, else SPEAK(message 2).
* **103** (held object used on thing, no walking): held=0; action or SPEAK(message 2).
* **1..49**: if !speaking and !walking: run script at action_offsets[[0x929c]] (0x2eac) with
  pc 0x929d / flag 0x929e; when it returns 0 → [0x929c]=0,[0x929d]=0. Note a running action
  is cancelled by any click (common_click) unless mode==2.

Use on thing (`fn 0x7c02`): search the 30-entry table at 0x300e (8 bytes: mode, object,
thing, action): first entry with mode != 0, object == held object [0x35a2], thing == [0x35a6];
mode 1 → state 102, mode 2 → state 103, [0x35a7] = action. No match → state 102 with action 0
(walk there, then message 2). [0x92ab] = thing, pend=0.

Combine (`fn 0x7a9a`): 50-entry table at 0x2f14 (5 bytes: used, obj1, obj2, new, mode),
matched with `fn 0x7bc9(obj1,obj2)` against the held object and the target object. Mode 0
removes both, 1 removes obj1, 2 removes obj2; then ADDINV(new) and SPEAK(message 4). No match →
SPEAK(message 3).

Fixed messages 1–4 are text slots 1..4 (loaded from the level's 6 fixed messages).

## 6. Text and speech

* Font: 256 glyphs × 51 bytes at 0xab24 (from the LETRAS resource). Byte 0 = `w<<4 | h`, then
  4 bits per pixel, high nibble first, row-major, continuous; 1 = text colour, 2 = border colour,
  other = transparent. Draw char: `fn 0x910`, no clipping. Advance = **w − 1** (`fn 0x6d15`
  draws a string with that advance). Line height 10.
* 5 line slots, 0x66 bytes at 0x8fdb: +0 active, +1 drawn last pass, +2 x, +4 y, +0xa width,
  +0xc height (10), +0x12 text (≤ 56 chars), +0x65 colour.
* `SPEAK(t0..t9)` (`fn 0x73f5`): queue up to 10 text ids at 0x9283 (−1 terminates), index
  [0x9298]=0, [0x9282]=1, [0x9297]=1, [0x9299]=0xff; save the player frame to [0x91dc] (clamped
  to the first frame of its direction group: groups start at [0x9c],[0x9a],[0x9b] with lengths
  [0xa3],[0xa1],[0xa2]) and set [0x91da]=0 (talking).
* `PTEXT(t, x, y, colour)` (`fn 0x74c4`): one text, [0x929a]=x, [0x929b]=y, [0x92a7]=colour,
  [0x9297]=0.
* Sequencer `fn 0x7513` (each pass while speaking): when the index changed: if id == −1 or
  index > 9 → end: [0x9282]=0; if it was the player: restore frame [0x91dc], sprite source from
  the frame table 0x96b0 (4 bytes per frame: src x, src y), size [0x980]x[0x982], [0x91f3]=−1.
  Otherwise lay out text slot id (0x41bb + 250·id): player → `fn 0x6d74(text, [0x92a5])`,
  other → `fn 0x70c3(x, y, text, [0x92a7])`. Then when [0x903e] <= 0 → `fn 0x762b` (lines off)
  and index++.
* Layout (`fn 0x6d74` / `fn 0x70c3`): split at `^` (and NUL) into lines of ≤ 56 chars; the first
  line goes to slot 4, the next to slot 3, … Each line is centred: player: x = player.x +
  [0x980]/2 − w/2; PTEXT: x = X − w/2. Then the widest line is clamped to 5 ≤ x and
  x + w ≤ 315 and the same shift applied to the other lines. y: the lowest active slot (= last
  line) is at player.y − 11 (PTEXT: Y − 11), each earlier line 10 px higher; then if a line's
  y < 5 every line is pushed down to make it 5.
* Duration: n = total characters; [0x903e] = textspeed·(n+2) if n < 10 else textspeed·n
  (50 Hz ticks; textspeed = [0x3538], 8 for SETUP text speed 2 → 20 chars = 3.2 s).
* Colours: player text [0x92a5] (var 8, init 1) with border [0x92a6] (var 9, init 15); PTEXT
  colour argument with border [0x92a6]. See `img/ui-speech.png`.
* Any mouse click while speaking (or while mode==1) ends the current text immediately.

## 7. Composition (what the screen shows)

Buffers (64000 bytes each): bg [0xde0f] = gfx0 (room picture), things [0xde07] = gfx1,
player+cursor [0xde0b] = gfx2 ("MOVIMIENTOS" sheet), inventory icons [0xde03] = gfx3, work
[0xddff], screen [0xde13] = A000:0. Visible pixel = work != 0 ? work : bg. Each pass the
original clears the work buffer only at old rectangles of things/player/lines/label/inventory
slots that changed, redraws everything into work, and copies only changed rectangles to VGA
(avoiding the cursor). A full redraw per pass gives the same picture; the order is:

1. background (gfx0, full 320×200 including the bottom bar art);
2. things with state ∉ {0,3} and layer 0 or 2, in index order: blit from gfx1 (src x,y, w,h)
   to (x,y), mirrored if +0x24, colour 0 transparent, **rows clipped to y ≤ 148** (`fn 0x5f4`);
3. player if var0 [0x91f5] != 0: from gfx2 at ([0x91e5],[0x91e7]) size [0x91e9]x[0x91eb] to
   ([0x91dd],[0x91df]), mirror [0x91f7], clipped to y ≤ 148;
4. things with layer 1 or 3;
5. active speech lines (font, colour per line, border [0x92a6]);
6. the label (if [0x353d] != 0);
7. inventory icons for visible slots i=0..7 that are used: object o = slot.obj, source
   ((o mod 9)·35, (o div 9)·26) in gfx3, size 35×26, at (13+37i, 173), not clipped to 148
   (`fn 0x705`);
8. the mouse cursor (ISR, §2) on top, unless mode 3.

Inventory arrows: the bar art uses colours 248 (left) and 249 (right). `fn 0x6c87` (called after
scrolling, level load, ADDINV/SUBINV…) sets current-palette entry 248 to the background
palette's entry 248 if [0x929f] > 0 else to black, and entry 249 to entry 249 if slot
[0x929f]+8 is used else black, then [0x92a4]=1 (DAC reload on the next pass). (It copies the raw
8-bit bg palette bytes into the 6-bit palette — the DAC keeps the low 6 bits; uncertain whether
games rely on that. Black is taken as (cur[0], cur[1], bg[2]) — a typo, harmless when colour 0
is black.)

The cursor restore `fn 0x38df` writes work-or-bg for a rect of the cursor size **plus 3 extra
rows**.

## 8. Level change `fn 0x11c9` and startup

```
mode = 2                                   // ISR hides the cursor
-n: name = "<base>.<nnn>"; load_level_files() else load_level_archive() (fn 0x1e23); fatal 0x65 on failure
keep gfx2/gfx3 names and offsets across the load (they are global, set by level 0)
if level == 0:                              // only at startup
    fn 0x17de: run ACTION 0 of level 0 to completion (loop: while run()!=0 {[0x92a0]=0; [0x9282]=0})
              the intro lives here; it must set [0x927c] (else fatal 0x69)
              load gfx2 (fatal 0x6a/0x6c) into 0xde0b, palette 0xa824; gfx3 (0x6b/0x6d) into 0xde03, palette 0xa524
    goto top (load the start level)
copy every thing's current pos/size to its "old" fields
load gfx1 into 0xde07 (palette 0xa224, fatal 0x66), gfx0 into 0xde0f (palette 0x9f24, fatal 0x67)
merge palettes fn 0x162d (below)
FADE OUT fn 0xccab                         // the previous screen is still on VGA
[0x929c]=[0x929d]=0
run ACTION 0 to completion (same loop as above: yields are re-entered immediately, walking and speech cancelled)
place the player: door d = [0x92a3] (10-byte records at 0x2de4: +0 used, +2 x, +4 y, +6 dir, +8 action)
    used: w,h = [0x980],[0x982]; x = door.x − w/2 (if != −100); y = door.y − h (if != −100);
          dir != −100: dir 0 → frame [0x9c] mirrored, 1 → [0x9c], 2 → [0x9b], 3 → [0x9a]
          sprite src from frame table 0x96b0; [0x929c] = door action (runs after arrival)
    unused: x/y unchanged, frame [0x9c]  (bug: the src y is written to 0x91e5 instead of 0x91e7)
full redraw (fn 0x61df), redraw_inventory (fn 0x6c87), copy the whole screen to VGA
FADE IN to palette 0x9924 (fn 0xcd25)
mode = 0; [0x96] = 0
```

Palette merge `fn 0x162d` into the current palette 0x9924 (8-bit PCX palettes, (255,255,255)
marks an unused entry): start from all 0xff, copy bg (0x9f24), then fill each still-white entry
from inventory icons (0xa524), then things (0xa224), then player sheet (0xa824); finally divide
every byte by 4 (to the 6-bit DAC range).

Fades (CPU-speed dependent busy loops in the original): fade-out = 64 steps, each step every
non-zero DAC component −1, then `loop 0x7d00` (no retrace wait). Fade-in = 64 steps, each
component moves 1 toward the target. Suggested: 64 steps over ≈ 0.4 s (DOSBox at 20000 cycles
≈ 1.6 ms per step plus the DAC writes; real 486-class machines similar). `fn 0x560` is the same
fade-out for a range of entries (used by ops).

Startup (main `fn 0xbc17`): [0x97]=50; `RUNVGA SETUP` just exits with errorlevel = SETUP.CFG
byte 0. Otherwise read SETUP.CFG bytes 1 (speed) and 2 (text speed) (§1), init (mode 13h,
fade-out, sound detection: SBFMDRV via the "FMDRV" signature in int 0x80..0xBF; if present load
CT-VOICE.DRV (fatal 0x71 if missing) and allocate CMF buffers), allocate the 5 screen buffers,
vars buffer and the 10000-byte code buffer, install int 8 (50 Hz) and int 9, mouse (fatal 5 if
absent; range 0..315 × 0..190, position 160,100, install handler). Parse `-f<file>` / `-n<base>`.
Defaults: gfx names cleared, [0x980]=32, [0x982]=50 (player size), [0x986]=4, [0x987]=10,
[0x984]=−1, cursor src/size from 0x96a4.., mode = 3, [0x92b4]=[0x92b6]=100, label colours
2/15, texts 0..79 empty. DAC all black, then the main loop (level 0 first).

## 9. SPACE menu `fn 0xabda`

Saves mode/[0x92b2]; mode=2 and **busy-waits until the ISR has hidden the cursor**. Draws into
the work buffer (so the scene stays visible around it), then copies the whole screen:

* filled panel (48,20)–(270,130) colour 251 (fill `fn 0xb38d(x1,y1,x2,y2,c)` covers
  x1..x2−1, y1..y2−1); bevel with `fn 0xb2b7` rectangle outlines (x1..x2, y1..y2):
  (48,20,270,130,252), (49,21,269,129,252), shadows (50,129)-(269,129) 250, (49,130)-(269,130)
  250, (269,21)-(269,129) 250, (270,20)-(270,130) 250;
* list box: (51,24,205,127) 250, then (51,127)-(205,127) 252 and (205,24)-(205,127) 252;
* 4 buttons x 214..260: rows (46..60) (61..74) (75..88) (89..102), each outline colour 252 with
  the bottom and right edges 250;
* labels (colour 0xfe, border 0xfb): "Cargar" (219,49), "Guardar" (216,63), "Salir" (225,77),
  "Jugar" (221,91).

Colours are palette indices of the current scene (the panel looks grey or dark depending on the
room; see `img/ui-menu.png`). Then mode=0 and loop on clicks (left or right; **cursor top-left,
no hotspot**, x 215..259):

* y 89..101 **Jugar**: redraw the label in colour 0xfd, full redraw, resume.
* y 75..259 **Salir** (bug: upper bound 0x103): hide cursor, label 0xfd, black band
  (0,93)-(320,108) with a colour-255 outline, "¿Salir (S/N)?" at (125,96) colour 255 border 0;
  wait for a key: 0x1f (S) → exit; anything else → full redraw and restart the menu.
* y 61..73 **Guardar** → save dialog `fn 0x8046`; returns 1 → resume, 0 → menu again.
* y 46..60 **Cargar** → load dialog `fn 0x915b`; same.
* Esc (scancode 1) → resume.

Resume = full redraw (`fn 0x61df`) and screen copy, restore mode and [0x92b2]. While the menu
runs [0x95]=1 freezes the text/walk/animation countdowns; the main loop does not run.

### 9.1 Save / load dialogs
* Same panel; the list shows 8 slots (names: 50 bytes at 0x92d0+50i, used flags 0x92c8+i, read
  from `SAVEGAME._<i+1>_` (first byte 0x0A, then the 50-byte name); missing/invalid →
  "- VACIO -"). Row i: fill (52, 25+13i)-(205, 35+13i) colour 251, text at (52, 26+13i) colour
  0xfe border 0xfb. Slot hit box: x 51..205, y 24+13i .. 35+13i (24..35, 37..48, 50..61, 63..74,
  76..87, 89..100, 102..113, 115..126).
* Save: buttons "Ok" (230,63) at y 61..74 and "Anular" (220,77) at y 75..88. Choosing a slot
  edits its name (starts from the existing name, "_" cursor). Accepted characters: a–z A–Z 0–9
  space . , / - ; : and CP850 á é í ó ú à è ì ò ù ä ë ï ö ü ñ Ñ; appended only while the pixel
  width < 138; Backspace deletes; Enter with a non-empty name saves (`fn 0xa4f9(slot)`) and
  resumes; Esc stops editing (stays in the dialog). Anular / Esc → back to the menu.
* Load: only an "Anular" button (y 75..88, `img/ui-load.png`); clicking a used slot loads it
  (`fn 0x9df3`): reload all four graphics and sounds, restart music, fade-out, full redraw,
  inventory redraw, screen copy, fade-in.
* Savegame content (in this order; the implementation may use its own format): 0x0A, name[50],
  0x91d9[0xd3] (player block), ACINVTH 0x300e[0xf0], IAC 0x2f14[0xf0], 0xb80[0x320],
  0x30fe[0x294], 0x3392[0x1a4], gfx names 0xa48[0x34], texts 0x41bb[20000], code[10000],
  action offsets 0x2eac[0x64], cursor block 0x92ac[0x1c], things 0x39a5[0x816], inventory
  0x35a9[0x3fc], anims 0x94b0[0x1f4], file names 0xa7c[0x104], text lines 0x8fdb[0x1fe] (chars
  +0xca while saving), file table 0x9a8[0xa0], gfx table 0x988[0x20], palettes 0x9924, 0xa824,
  0xa524, 0xa224, 0x9f24, 0x9c24 (0x300 each), vars 0..1999 (2 bytes each, through the pointer
  table), 0x984[2], frame table 0x96b0[0x190], 0x980[4], bytes 0x9a..0xa6 (13), 0x96a4[0xc],
  0x3539[0x70].

## 10. Inventory slots
30 slots, 0x22 bytes at 0x35a9: +0 name (30 bytes, stored −0x20 in the file), +0x1e used,
+0x1f used-last-drawn, +0x20 object id, +0x21 id-last-drawn. ADDINV (`fn 0x689a`) takes the first
free slot, reads object o's 280-byte record from the inventory resource (offset o·0x118:
description 250 bytes −0x1e, name 30 bytes −0x20), stores the description as **text 50+slot**.
Left-clicking the slot speaks that text.

## 11. Differences from v2 that matter for the port
* No verbs/cursor sets; the cursor is one sprite from the gfx2 sheet set per level (0x96a4).
* Click semantics as in §5; thing behaviour is chosen by the thing's type field.
* Text speed/duration, fixed label line, 8-slot bottom inventory with scroll arrows.
* Main loop pass = 2 retraces like v2; ISR 50 Hz like v2.
* The SPACE menu is a modal panel with Cargar/Guardar/Salir/Jugar and 8 save slots;
  Ctrl+Esc quits immediately.

## 12. Emulator recipe (reference runs)
`ui_play.py` fixes int33 coordinates (no ×2) and uses 8 M instructions/s; clicks must be held
≥ 12 ticks because a pass takes several ticks in the emulator. From launch:
`INTRO` (= `w 2500; c 160 100` ×10, then `w 3000`) reaches level 35 (title menu); `w 2000; m 240
150; w 300; c 240 150` clicks "Comenzar" (thing 2, type 3 → action 2 → GOLEVEL 1); after ≈ 6 s
the player is in level 1 (bedroom). "Instrucciones" is thing 1 at (5,107) 144×92.
