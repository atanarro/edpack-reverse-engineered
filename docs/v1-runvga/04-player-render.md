# RUNVGA.EXE v1.03 (EDPACK v1): player movement and rendering

Addresses:
- Code addresses are offsets in the unpacked `RUNVGA.bin`, where CS = 0x1000.
- Data addresses are `DS:xxxx`, with DGROUP = 0x0fec relative to the load segment. Linear address = (0x1000 + 0xfec) × 16.

How this was validated:
- **Walker and GOXY.** A Python model (`scratchpad/re/ply_model.py`) was checked against the reference emulator (`ply_dosemu.py`, `ply_trace.py`). It used DS snapshots taken at entry and exit of every walker call (0x3e6a) and every GOXY call (0x7dab) in level 1 of *Manolo Kabezabolo*. Result: 2,448 walker calls (59 with movement) and 5 GOXY calls, including a LIMITMOV detour, with 0 mismatches.
- **Screen composition.** The rule "screen = overlay ≠ 0 ? overlay : background" matches VRAM everywhere except the 11×11 mouse cursor.

Emulator note: RUNVGA sets the int 33h ranges in pixel units (fn 7/8 with x range 0..319−w). Function 3, function 5/6 and the event handler must therefore report CX = x, not 2x. Function 4 stores x = CX.

Overall, v1 is v2 without scaling, shading, scrolling, PMOV, draw-order lists or THING AREA rectangles. The table sizes are smaller and the image pipeline is buffered: an overlay buffer drawn over a background buffer.

## 0. State variables

| DS | var# | type | meaning (v2 equivalent) |
|---|---|---|---|
| 91f5 | 0 | i16 | player visible (fc19). The player is drawn into the overlay when ≠0, but its new rectangle is only presented to VRAM when ==1. |
| 91f3 | – | i16 | previous 91f5 (redraw detection) |
| 91da | 1 | u8 (var writes are 16-bit and also clobber 91db) | frame index into the point list 96b0 (fbf5) |
| 91db | – | u8 | frame drawn last time; also "door saved frame" (fbf6) |
| 91dc | – | u8 | standing frame saved by SPEAK (fbf7) |
| 91d9 | – | u8 | "animate this tick" request (fbf4) |
| 91dd, 91df | 2, 3 | i16 | sprite top-left (fc01/fc03) |
| 91e1, 91e3 | – | i16 | previously drawn top-left |
| 91e5, 91e7 | – | i16 | source x, y of the current frame in the gfx-2 sheet |
| 91e9, 91eb | 4, 5 | i16 | sprite w, h. Overwritten with [980]/[982] at the end of every walker call. |
| 91ed, 91ef | – | i16 | previously drawn w, h |
| 91f7 | 6 | u8 | mirror flag (1 = facing left). The var write clobbers 91f8. |
| 91f8 | – | u8 | previous flip |
| 91fd, 91fe | – | u8 | dirX, dirY state (fc1d/fc1e): 0 unset, 1 +, 2 −, 3/4 = X "minor axis", 0xff = done |
| 91ff | – | u8 | path cursor: index in words, +2 per waypoint (fc1f) |
| 9204.. | – | i16 pairs | path array: point k is at 9204+4k (x), 9206+4k (y); terminator (−100,−100) |
| 92a0 | – | u8 | walking (fcce): set by GOXY, cleared at idle |
| 9297 | – | u8 | player speaking (fcbf) |
| 92a1 | – | u8 | walk-step countdown, decremented by the 50 Hz timer ISR (fccf) |
| 3536 | – | u8 | ticks per walk step, from SETUP.CFG byte[2]: 0→15, 1→8, 2→4, 3→3, 4→1 |
| 3537 | – | u8 | ticks between ANIMATION script runs, from SETUP.CFG byte[3]: 0→12, 1→6, 2→3, 3→2, 4→0 (set at 0xbcfe..0xbd3f) |
| 987, 986 | – | u8 | step size X / Y in pixels. Set to 10 / 4 once at start (0xbfc3); SPEEDXY changes them. Bytes, unlike v2's i16. |
| 980, 982 | – | i16 | player frame w, h (v2 DEFMOV w/h 4334/4336); from the level file |
| 9a..9f | – | u8 | sequence starts: 9a back, 9b front, 9c side, 9d talk-side, 9e talk-back, 9f talk-front |
| a1..a6 | – | u8 | sequence counts: a1 back, a2 front, a3 side, a4 talk-back, a5 talk-front, a6 talk-side |
| 96b0 | – | 100 × {i16 x, i16 y} | frame source points in the gfx-2 sheet. Sequences are separated by (−100,−100); unused points are (−1,−1). |
| 927c | – | i16 | current level; 92a3 (u8) = entry door |

"feet" = (91dd + 91e9/2, 91df + 91eb), where `/2` is C integer division (truncates toward 0). GOXY targets, LIMIT/LIMITMOV boxes, doors and thing GOTO points are all feet coordinates.

Other variables (from the pointer table at DS:0ea4, which holds far pointers):
- 7 → 9282, 8 → 92a5, 9 → 92a6, 10 → 92b0, 11 → 3539, 12 → 353b.
- Thing vars: 30 + 20t + v (t = 0..8 only, 9 things).

## 1. Player animation data ("MOVIMIENTOS") — per level, not a global file

**Optional level block.** After the ACINVTH-like table, the level loader (0x2133..0x22ed) reads:
1. i16 → [980] if ≥ 0.
2. i16 → [982] if ≥ 0.
3. i16: if < 0, the MOV block is absent. The loader skips 0x19b bytes and keeps the previous data. Otherwise the low byte → [9a], then single bytes → 9b, 9c, 9e, 9f, 9d, a1, a2, a3, a4, a5, a6, then 400 bytes → 96b0.

In file order this is: back, front, side, talk-back, talk-front, talk-side starts, then the counts in the same order. That is exactly the order of v2's DEFMOV header.

Count convention (same as v2): the back count = number of frames; the other counts = frames + 1, including the separator.

Level 1 of Manolo:
- starts = 0, 6, 12, 22 (talk-back), 26 (talk-front), 30 (talk-side)
- counts = 5, 6, 10, 4, 4, 4
- w × h = 32 × 50

**Pixels.** The pixels come from gfx slot 2 (name at DS:a62, e.g. `manoanim.PCX`), loaded once, in level 0 only (0x17de):
- An empty name is fatal 0x6a, "Fichero de los MOVIMIENTOS no especificado en el nivel 0".
- The image goes to far buffer [de0b:de0d], its palette to DS:a824.

The same sheet also holds the mouse cursor images (see §5).

## 2. Walking

### 2.1 GOXY(x, y) — 0x7dab (op 2, click-to-walk, thing GOTO)
```
for i in 0..29:                              # LIMIT DS:3392, 30 x 14 bytes {used,x1,y1,x2,y2,tx,ty}
    L = LIMIT[i]; if L.used == 0: continue   # (v2: 40 entries)
    if L.x1 <= x <= L.x2 and L.y1 <= y <= L.y2:          # inclusive; uses the already redirected x,y
        if L.tx not in (-100,-101): x = L.tx
        if L.tx == -101: x = feetx               # current position
        if L.ty not in (-100,-101): y = L.ty
        if L.ty == -101: y = 91df + 91eb         # current feet y
        # -100 = keep the clicked coordinate.  (v2 used -100 = current, -200 = keep!)
cx, cy = feetx, 91df + 91eb
if cx == x and 91df + 91df + 91eb == y: return   # BUG: top counted twice -> practically never "already there"
k = 0; c = 0                                      # LIMITMOV DS:30fe, 30 x 22 bytes (v2: 320), single pass
while c < 30:
    R = RULE[c]
    if R.used != 0 and R.tx1<=x<=R.tx2 and R.ty1<=y<=R.ty2 and R.cx1<=cx<=R.cx2 and R.cy1<=cy<=R.cy2:
        path[k] = (R.wx, R.wy); cx, cy = R.wx, R.wy; k++
        c = 0            # then c++ -> rescans from rule 1 (rule 0 only on the first sweep), like v2
    c++
path[k] = (x, y); 92a0 = 1; path[k+1] = (-100,-100); 91ff = 0
```
Rule layout is the same as v2:
- `+0 used`
- `+2 tx1, +4 ty1, +6 tx2, +8 ty2`: target box
- `+a cx1, +c cy1, +e cx2, +10 cy2`: current-position box
- `+12 wx, +14 wy`: waypoint

Differences from v2:
- One ascending pass only; no descending or third pass.
- No `-100` "keep" handling for waypoints.
- No check that the waypoint differs from the current position, so a rule whose waypoint stays inside its own boxes loops forever.
- No "+10000 ignores LIMIT".
- `used` is tested `!= 0` rather than `== 1`.

GOXY does not reset 91fd/91fe; the click handler does (§2.3).

### 2.2 Walker — 0x3e6a, once per main-loop pass

Verified bit-exact. It is v2's 0x45d2 algorithm (see [`../v2-rundat/03-player-movement.md`](../v2-rundat/03-player-movement.md) §2.3), with these changes:
- **Direction setup:** the X "already there" test compares `P.x == 91dd` (the sprite's **left edge**), not feet x. If P.x happens to equal the left edge, dirX becomes 0xff and that leg does not move in X. Y uses feet y, as in v2.
- **Step countdown:** reloaded with `[3536]` whether or not the player is speaking. v2 used double the value while speaking.
- **Step sizes:** X steps are byte [987] (half for dirX 3/4 while Y is moving). Y steps are byte [986] (half while dirX is 1/2).
- **Snapping:** the X snap is done twice when the next point is the terminator (as in v2). Y snaps to `P.y − 91eb`.
- **Talk sequence selection (speaking ≠ 0):** if `frame < [9e]`:
  - saved == [9c] → frame = [9d] (talk side, flip kept)
  - saved == [9a] → frame = [9e], flip 0
  - saved == [9b] → frame = [9f], flip 0

  Otherwise anim = 1.
- **Frame wrap rules** after `frame++`, in this order:
  - == 9c+a3−1 → 9c+1
  - == 9a+9a−1 → 9a+1 (BUG, as v2)
  - == 9b+9b−1 → 9b+1 (BUG, as v2)
  - == 9d+a6−1 → 9d
  - == 9e+a4−1 → 9e
  - == 9f+a5−1 → 9f
- **Idle normalization:** back to the standing pose of the side, back or front sequence (exclusive bounds). Then path[0] = terminator, 92a0 = 0; 91ff is not reset.
- **Every call ends with:** `91e5,91e7 = point[frame]` and `91e9,91eb = [980],[982]`. There is no PMOV source.

The reference Python model is `ply_model.py: walker()`; port it literally.

### 2.3 Click-to-walk (0x4842, the main-loop input routine)

Input is ignored while cursor state [92b0] is 2 or 3. [92b0]: 0 normal, 2 busy (set during a level change), 3 hidden.

On a press of the left (0) or right (1) button, detected via int 33h fn 5, "pressed since last poll AND still down":
1. If text is not active ([9282]==0), [929c] ≠ 0 and [92b0] ≠ 2 → [929c] = 0. This cancels a pending verb action.
2. Stop the walk: 91ff = 0, 91fd = 91fe = 0, path[0] = (−100,−100).
3. If no inventory object is held ([35a0] ≠ 1), no action is running ([929c]==0), the cursor is over no thing ([9202]==0) and `mouseY + hotY < 0x94` (148), then `GOXY(mouseX + hotX, mouseY + hotY)`. The hotspot is [92c4]/[92c6].

The verb/object logic in the same routine (inventory scroll arrows at x ≤ 13 / x ≥ 307 with y > 173, look/use/pick) belongs to the UI spec. Both buttons walk.

### 2.4 SPEEDXY / PPOS / doors
- **SPEEDXY(a,b) (op 35):** `if a >= 0: [987] = a (byte)`; `if b >= 0: [986] = b`.
- **PPOS(d) (op 39):**
  - 0: frame = [9c], flip = 1
  - 1: frame = [9c], flip = 0
  - 2: frame = [9b], flip 0
  - 3: frame = [9a], flip 0

  The source point is not updated until the next walker call.
- **Doors:** DS:2de4, 20 × 10 bytes {i16 used, x, y, dir, action}. They are only used at level entry (0x11c9); walking into one does nothing. Level changes come only from GOLEVEL (op 1: [927c] = level, [92a3] = door).

Level entry, after ACTION 0 has run to completion (each yield clears [92a0] and [9282]):
```
D = DOOR[92a3]
if D.used:
    91e9,91eb = [980],[982]
    if D.x != -100: 91dd = D.x - 91e9/2
    if D.y != -100: 91df = D.y - 91eb
    if D.dir != -100: 91db = 91da; apply PPOS(D.dir)
    91e5,91e7 = point[91da]; [929c] = D.action (low byte)   # runs on the first main-loop pass
else:
    91e9,91eb = [980],[982]; 91da = [9c]
    91e5 = point.x; 91e5 = point.y    # BUG: 91e7 not set (same bug as v2)
    [929c] = 0
[929d] = 0; full redraw (0x61df); inventory arrows (0x6c87); present whole screen; upload palette (0xcd25); [92b0] = 0
```
Position, flip and visibility carry over from the previous level unless the door sets them.

## 3. THING struct — DS:39a5 + 0x45·t, t = 0..29 (69 bytes, byte-packed)

| off | type | meaning | thing var v (var 30+20t+v) |
|---|---|---|---|
| +00, +02 | i16 | source x, y in the gfx-1 sheet ("DIBUJOS") | – |
| +04, +06 | i16 | previous source x, y | – |
| +08, +0a | – | unused by the renderer | – |
| +0c | i16 | layer: 0/2 below the player, 1/3 above. 2/3 are fixed; 0/1 are re-evaluated every pass (§4.4). | 1 |
| +10, +12 | i16 | destination x, y (top-left on screen) | 5, 6 |
| +14, +16 | i16 | previous destination x, y | – |
| +18, +1a | i16 | w, h (drawn size **and** hover rectangle) | – (set by NEWTHING) |
| +1c, +1e | i16 | previous w, h | – |
| +20 | i16 | previous mode | – |
| +22 | i16 | mode: 0 off, 1 drawn + clickable, 2 drawn only, 3 clickable only (invisible hotspot) | 0 |
| +24 | u8 | mirror; only the value 1 mirrors | 7 |
| +26..+34 | char[] | name, stored +0x0d; decrypted at load (0xc3a8) for things 0..24 only | – |
| +35, +37, +39 | i16 | GOTO x, y, dir (feet; dir as PPOS) | 8, 9, 10 |
| +3b | i16 | ? | 11 |
| +3d, +3f, +41, +43 | i16 | action/verb fields (UI spec: +3f look mode 0..3, +41 look action) | 4, 2, 3, 12 |

NEWTHING(t, c) (op 31): COORANIM table DS:0b80, 100 × {i16 x, y, w, h}. `thing.src = (x,y)`, `thing.w,h = (w,h)`. Unlike v2, the size comes from COORANIM.

There is no AREA rectangle and no draw-order list (v2's 8b49). Things are drawn and hit-tested in index order 0..29.

## 4. Rendering

### 4.1 Buffers (far, 64000 bytes each, 320 wide)

| ptr | contents |
|---|---|
| [ddff:de01] | **overlay** (work) buffer: sprites, texts and icons only; 0 = transparent |
| [de0f:de11] | background = gfx slot 0 (name DS:a48), palette DS:9f24; loaded every level |
| [de07:de09] | gfx slot 1 = thing sheet (a55), palette DS:a224; loaded every level |
| [de0b:de0d] | gfx slot 2 = player frames and mouse cursors (a62), palette DS:a824; level 0 only |
| [de03:de05] | gfx slot 3 = inventory icons (a6f), palette DS:a524; level 0 only |
| [de13:de15] | VRAM A000:0000 |

At a level change (0x11c9), the gfx-2/3 names and offset-table entries (DS:998..9a7) are saved before the level loads and restored after. A level's own slot-2/3 entries are therefore ignored after level 0. The PCX loader is 0xa3e(name, buffer, palette); the image must be 320×200.

**Palette merge (0x162d, every level change):**
1. P (DS:9924) = all 0xff; P = palette 0.
2. Then, in order, palette 3, palette 1, palette 2: for each entry where P is (ff,ff,ff) and the source entry is not (ff,ff,ff), copy it.
3. Divide all 768 bytes by 4 (8-bit → 6-bit).

Same rule as v2: white means an empty slot. Merge order is 0 → 3 → 1 → 2.

### 4.2 Primitives

| addr | signature | behaviour |
|---|---|---|
| 0x5f4 | blit(sx, sy, dx, dy, w, h, src, dst, flip) | Transparent (0) copy. If flip == 1, source columns run sx+w−1 down to sx; the full width is drawn, with no lost column (unlike v2). A pixel is written only if 0 ≤ dx ≤ 319, **0 ≤ dy ≤ 148 (0x94)**, 0 ≤ src x ≤ 319 and 0 ≤ src y ≤ 199. w or h == 0 underflows to 65535 iterations (quirk; the clip checks keep it harmless but slow). |
| 0x705 | blit(...) | same, but dy is clipped to 0..199 (inventory icons) |
| 0x39d0 | clear(x, y, w, h, buf) | sets buf to 0 for columns x..x+**w** (w+1 columns) and rows y..y+h−1, clipped to the screen. Skipped entirely if x+w<0, y+h<0, x>319 or y>199. |
| 0x3a7c | clear_player(x, y, w, h, buf) | same, rows clipped to y < 149 |
| 0x3cb5 | present(x, y, w, h, W, B, VRAM) | for columns x..x+w and rows y..y+h−1: VRAM = W ≠ 0 ? W : B, except pixels covered by a visible cursor pixel (§5) |
| 0x3b28 | present_overlay(x, y, w, h, W, …, VRAM) | same region and cursor rule, but writes only where W ≠ 0 |
| 0x38df | restore(x, y, w, h, W, B, VRAM) | like present but rows y..y+h+2 (h+3 rows) and no cursor test; the ISR uses it to erase the cursor |

### 4.3 Main-loop draw (inline in 0x5037, after the logic calls)

**Phase A: erase in the overlay** ([94] = 1 suspends the cursor ISR's drawing):
1. Each thing 0..29 whose mode, source, destination or w/h differs from its "previous" copy: clear(prev x, prev y, prev w, prev h).
2. PTEXT slots 0..4 (DS:8fdb + 0x66·i, UI spec) that went from shown to hidden: clear their old rectangle.
3. If the player's position, w/h, flip, frame or visibility changed: clear_player of the old rectangle (91e1, 91e3, 91ed, 91ef). While speaking, the **current** position and [980]/[982] are used instead.
4. If the info text state [353d] ≠ [353e]: clear its old rectangle (3543, 3545, 354c, 354e).
5. Inventory bar slots 0..7 (record DS:35c7 + 0x22·(scroll [929f] + i)) whose bytes +0/+2 differ from +1/+3: clear(13+37i, 173, 35, 26).

**Phase B: draw into the overlay**, full and not incremental (everything visible is redrawn every pass):
6. Things 0..29 with mode ∉ {0,3} and layer ∈ {0,2}: blit(src = thing sheet, dst = overlay).
7. If 91f5 ≠ 0, the player: blit(91e5, 91e7, 91dd, 91df, 91e9, 91eb, gfx-2 sheet, overlay, 91f7).
8. Things 0..29 with mode ∉ {0,3} and layer ∈ {1,3}.
9. PTEXT slots, then the info text if it changed and is non-empty (font DS:ab24, UI spec).
10. Inventory icons 0..7 if changed and non-empty. Icon id = byte +2 of the slot record; source = ((id % 9)·35, (id / 9)·26), 35×26, from gfx 3, drawn to (13+37i, 173) with 0x705.

[94] = 0. Then, if palette-dirty [92a4] == 1, upload DS:9924 and clear the flag; otherwise wait for 2 vertical retraces (the frame pacing).

**Phase C: present to VRAM:**
11. If the player changed: present the old rectangle (the current one while speaking).
12. Things that changed: present their old rectangle.
13. PTEXT slots that disappeared; the old info-text rectangle; changed inventory slots: present.
14. Things that changed with mode ∉ {0,3}: present_overlay of the new rectangle. Then copy all current values to "previous".
15. Player, if changed: if 91f5 == 1, present_overlay of the new rectangle (current position with [980]/[982] while speaking). Then copy to previous.
16. New PTEXT slots, the new info text and changed inventory slots: present_overlay. Then copy the state.

Implementation shortcut that gives identical pixels: keep the overlay and background, and show `overlay ≠ 0 ? overlay : background` with the cursor on top. The incremental rules only matter for stale pixels. Because changed regions are cleared before redraw and the overlay is fully redrawn, a straight per-pass recomposition equals VRAM: verified in level 1.

One case where the incremental scheme deviates from a straight recomposition: an object that does not change but overlaps one that did is re-blitted in phase B anyway, so the result is the same.

**Full redraw (0x61df):** used for DRAW (op 34), level entry and after menus.
- Clears the whole overlay.
- Draws **things 0..24 only** (QUIRK: 25..29 are skipped until they next change) in layer order, then the player (if 91f5 ≠ 0), PTEXT slots, the info text and the inventory icons.
- It does not present; DRAW and level entry then present(0, 0, 320, 200).
- It does not update the "previous" copies.

### 4.4 Layer auto-swap (0x4f83, every pass after the walker)

For each thing with mode ≠ 0 and layer ∉ {2,3}: `layer = (pos.y + h >= 91df + [982]) ? 1 : 0`. This is the same as v2, using the DEFMOV height [982] rather than 91eb.

### 4.5 Hover (0x4c44): which thing is under the cursor

Called from the input routine when [92b0] == 0. With p = mouse + hotspot:
- For t = 0..29, in index order: if `pos.x ≤ px ≤ pos.x + w` and `pos.y ≤ py ≤ pos.y + h` (inclusive, using the **drawn rectangle** +10/+12/+18/+1a) and mode ∈ {1,3}, then [9202] = t+1 and the first match wins. If [3547] ≠ t+1, the info text is marked dirty ([353d] = 2).
- No match: [9202] stays 0 (it was cleared earlier in the pass), with info-text bookkeeping (UI spec).

Unlike v2, this uses no AREA rectangle, no order list and no MMOUSE gating. Mode 2 things are drawn but never hovered.

### 4.6 Palette ops (all set [92a4] = 1; uploaded at the end of the next pass)

The palette is DS:9924 (6-bit). For all four: if first + count > 255, then count = 256 − first.
- **op 27 (0xf80)** (first, count, amount): each component c with c > 0 → c = (c + amount) & 0xff. There is no clamp: values over 63 wrap in the DAC. Zero components stay zero.
- **op 28 (0xefe)** (first, count, amount): each component with c − amount > 0 → c −= amount, otherwise unchanged. Darkening never reaches 0.
- **op 29 (0xff9)** (first, count): rotate the range by one entry; the last entry moves to the first slot.
- **op 30 (0x10e1)** (first, count): rotate the other way; the first entry moves to the end.

There is no SHADOW/SHADOWMOV, no SCALEXY/SCALEMOV and no scrolling in v1.

The inventory-arrow colours (0x6c87, UI spec): palette entries 248/249 are set from the gfx-0 palette when scrolling is possible, otherwise to entry 0's colour.

## 5. Mouse cursor (software, drawn by the 50 Hz timer ISR 0x36ee)

Each tick, unless [94] == 1 (the main loop is composing), the ISR:
- Reads the position from the int 33h event handler (0x8b0 stores CX, DX) into 92b4/92b6.
- If cursor state [92b0] is 2 or 4: restore the old rectangle (0x38df), then [92b0] = 3 (hidden).
- If the state ≠ 3 and the position or state changed: restore the old rectangle, then draw the cursor with 0xc7e1, a transparent blit from the gfx-2 sheet at (92ac, 92ae), size 92bc × 92be, to VRAM at (92b4, 92b6).

Level 1 uses source (300, 37) and a size of 11 × 11. The click hotspot is 92c4/92c6.

The ISR also decrements:
- 903e: only while text is active, [9282] == 1
- 92a1: walk step
- 92a2: ANIMATION interval
- 92a8

These are skipped while [95] ≠ 0 (menu), except 92a8.

To reproduce: draw the cursor last, over the composed frame, at mouse position (not mouse + hotspot). Pixels under the cursor rectangle keep the cursor where its source pixel ≠ 0.

## 6. Differences from v2, summary
- Tables are smaller: LIMIT has 30 entries (v2 40), LIMITMOV 30 (v2 320). LIMITMOV is single-pass ascending. The LIMIT magic values are −100 = keep clicked, −101 = current position.
- GOXY's "already there" test is bugged (2·top + h), and the walker's X "arrived" test at leg setup uses the left edge.
- Player data (frames, w/h, sequence table) is per level in the level file. The sheet is the global gfx slot 2.
- There is no scaling, shading, PMOV, AREA or order list. Hit-testing uses the drawn rectangle. Things draw in index order.
- Step sizes are bytes (987/986). The step interval does not double while speaking.
- The mirror blit draws all columns, and sprites are clipped to y ≤ 148: the play area is 149 rows, with the inventory bar at y 173.
- The full redraw only covers things 0..24.
- The cursor is a software sprite drawn by the timer ISR from the player sheet.

Uncertain:
- The meaning of thing +3b and the exact verb semantics of +3d..+43; deferred to the UI/main-loop spec.
- Whether any script can make 91f5 a value other than 0/1. With 2, the player is drawn into the overlay but its new rectangle is never presented, so it shows up only where other updates repaint that area.
