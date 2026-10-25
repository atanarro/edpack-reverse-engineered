# EDPACK v2.0 RUNDAT.EXE – player character spec (movement, frames, scaling, shading)

All addresses: code = RUNDAT.bin linear; data = DS:xxxx (DS linear 0x12a80).
FP code: Borland emulator ints were patched back to x87 (`CD 34..3B` -> `9B D8..DF`, `CD 3D` -> `9B 90`)
into `SCRATCH/RUNDAT_fp.bin`; full linear disassembly in `SCRATCH/rundat_fp.asm` (compiler: `SCRATCH/ed.asm`).
Verified LIMITMOV generator re-implementation: `SCRATCH/re/limitmov.py` (bit-exact vs levels 1,4,5).

"feet" = bottom-centre of the sprite box = (fc01 + fc0d/2, fc03 + fc0f). `/2` is C `int/2` (idiv, trunc toward 0).
All coordinates in scripts/doors/GOTO/LIMIT/LIMITMOV/SCALEMOV/SHADOWMOV refer to the FEET point.

## 0. Player state variables

| DS addr | var# | type | meaning |
|---|---|---|---|
| fc19 | 0 | i16 | player visible (drawn only if != 0). Not set by init (0) – game must VAR(0,1). |
| fc17 | – | i16 | "previous fc19" for redraw detection; PMOV sets fc17=fc19+1 and speech end sets 0xffff to force a redraw. |
| fbf5 | 1 | u8 (var writes are 16-bit: also clobber fbf6) | current frame index into DEFMOV point list (DS:200c) |
| fbf6 | – | u8 | frame drawn last time (redraw detection) |
| fbf7 | – | u8 | standing frame saved by SPEAK (restored at end of speech) |
| fbf4 | – | u8 | "animate this tick" request flag |
| fc01,fc03 | 2,3 | i16 | sprite TOP-LEFT (unscaled box) |
| fc05,fc07 | – | i16 | previous drawn top-left |
| fc09,fc0b | – | i16 | source x,y of current frame in the source picture |
| fc0d,fc0f | 4,5 | i16 | current sprite w,h (DEFMOV w,h or PMOV cooranim w,h) – recomputed every walker tick |
| fc11,fc13 | – | i16 | previous drawn w,h |
| fc1b | 6 | u8 (var write clobbers fc1c) | horizontal mirror flag (1 = mirrored = facing LEFT) |
| fc1c | – | u8 | previous flip |
| fbef/fbf1 | – | far ptr | source picture: MOVSPCX buffer [379e:37a0] (fbf3=2) or DIBUJOSPCX [379a:379c] (fbf3=3, PMOV) |
| fbf8 | – | u8 | PMOV active (1) |
| fbf9,fbfb,fbfd,fbff | – | i16 | PMOV cooranim x,y,w,h (copied from COORANIM DS:4532+10n) |
| fd3c,fd3e | 41,42 | i16 | feet position, recomputed at the END of each walker tick and at level entry (read-only in practice; writing them only affects a GOXY issued before the next walker tick) |
| fcf2,fcf4 | 26,27 | i16 | pixels removed from width / height (scale) |
| fcf6,fcf8 | – | i16 | previous scale (redraw detection) |
| fcfa,fcfc | – | i16 | step size X / Y in pixels (init 10 / 4) |
| fd22 | 23 | i16 | play-area bottom (init 187) |
| fd27,fd29 | – | i16 | SHADOW(first,count) palette range (init 0,0 = disabled) |
| fd25 | – | i16 | shade value currently applied (DS initial 0; never reset) |
| fcbf | – | i16 | 1 = player is speaking (SPEAK), 0 otherwise (PTEXT sets 0) |
| fcce | – | u8 | 1 = walking (set by GOXY, cleared when the path is exhausted) |
| fc1f | – | u8 | path cursor: word index into path array (advances by 2 per waypoint) |
| fc1d,fc1e | – | u8 | current X / Y direction state (see 2.3) |
| fc24.. | – | i16 pairs | path array: point k at DS:fc24+4k (x), fc26+4k (y). Terminator (-100,-100). Room until fc9c (30 points) – overflow overwrites fc9c (level number) etc. |
| fccf | – | u8 | step countdown, decremented by timer ISR (50 Hz) |
| 8a22 | – | u8 | ticks per walk step (from SETUP.CFG byte[2]: 0→15,1→8,2→4,3→3,4→1; demo cfg = 3) |
| 8a23 | – | u8 | ticks between ANIMATION script runs (0→10,1→5,2→2,3→1,4→0) |

DEFMOV header bytes (loaded by NEWMOVS): DS:cf,d0,d1,d2,d3,d4 = start indexes, DS:d6..db = counts:

| byte | meaning | demo value |
|---|---|---|
| cf | start of seq1 "walk away/back" (index cf = standing frame) | 0 |
| d0 | start of seq2 "walk toward/front" | 6 |
| d1 | start of seq3 "walk right/side" | 12 |
| d3 | start of seq4 "talk back" | 21 |
| d4 | start of seq5 "talk front" | 30 |
| d2 | start of seq6 "talk side" | 51 |
| d6 | count seq1 (= frames, compiler omits the +1) | 5 |
| d7 | count seq2 (= frames+1, i.e. includes separator) | 6 |
| d8 | count seq3 | 9 |
| d9 | count seq4 | 9 |
| da | count seq5 | 21 |
| db | count seq6 | 21 |

Timer: int 8 handler at 0x3dc5 (chains to BIOS), PIT programmed with 1193180/[DS:dd]; init (0xb78c) sets [dd]=50 → 50 Hz.

## 1. NIVEL.MOV format (EDGAME.DAT entry 8, loader 0x2877 = NEWMOVS(n))

```
+0   30-byte header (02 00 00 02 + 26 zeros; type byte must be 2, else fatal 0xb)
+30  u16 count            (n >= count -> fatal 0x75)
+32  count x 0x1a0 (416) byte DEFMOV entries:
     +0  u16 w      -> DS:4334
     +2  u16 h      -> DS:4336
     +4  u8 start1(back)  -> cf        +10 u8 cnt1 -> d6
     +5  u8 start2(front) -> d0        +11 u8 cnt2 -> d7
     +6  u8 start3(side)  -> d1        +12 u8 cnt3 -> d8
     +7  u8 start4(talk back)  -> d3   +13 u8 cnt4 -> d9
     +8  u8 start5(talk front) -> d4   +14 u8 cnt5 -> da
     +9  u8 start6(talk side)  -> d2   +15 u8 cnt6 -> db
     +16 100 x {i16 x, i16 y} -> DS:200c   (source coords in MOVSPCX; sequences separated
         by (-100,-100); compiler appends a final (-100,-100); unused = (-1,-1))
```
Compiler semantics (from demo data): start_k = index of the first point of sequence k in the flattened list
(separators included); cnt_1 = number of frames of seq 1; cnt_k (k>=2) = frames+1. Use stored values verbatim.
The first point of seq 1/2/3 is the standing pose; points 2.. are the walk cycle. Talk sequences cycle over all
their points. Left = seq 3 drawn mirrored (fc1b=1).

NEWMOVS only reloads the table; fc0d/fc0f/fc09/fc0b are refreshed by the next walker tick.

## 2. Walking

### 2.1 GOXY(x,y) (0x7fdc) – path planner. Also used for click-to-walk and THING GOTO.

```
gx=x; gy=y; rawx=rawy=0
if gx >= 3000: rawx=gx; gx -= 10000          # "+10000 ignores LIMIT"
if gy >= 3000: rawy=gy; gy -= 10000
for i in 0..39:                               # LIMIT DS:87f2, 14 bytes {used,x1,y1,x2,y2,tx,ty}; all 40 scanned, sequential
    L = LIMIT[i]; if L.used == 0: continue
    if L.x1 <= gx <= L.x2 and L.y1 <= gy <= L.y2:          # inclusive, uses the possibly already redirected gx,gy
        if L.tx != -100 and L.tx != -200: gx = L.tx
        if L.tx == -100: gx = fc01 + fc0d/2               # player's current feet x
        (tx == -200: keep clicked x)
        if L.ty != -100 and L.ty != -200: gy = L.ty
        if L.ty == -100: gy = fc03 + fc0f                  # player's current feet y
if rawx > 3000: gx = rawx - 10000             # restore (LIMIT result discarded for that axis)
if rawy > 3000: gy = rawy - 10000
if fd3c == gx and fd3e == gy: return          # already there: nothing changes (fcce untouched, path untouched)

# LIMITMOV (DS:6c72, 320 rules x 22 bytes, see 2.4). Three passes:
pass = 0
loop:
    curx, cury = fd3c, fd3e; k = 0 (path slot); n0/n1 counters (pass 0 / pass 1)
    order(c) = 319 - c  for pass 0 and 2 ;  c  for pass 1
    c = 0
    while c < 320:
        r = RULE[order(c)]
        if r.used == 1 and r.tx1<=gx<=r.tx2 and r.ty1<=gy<=r.ty2
                        and r.cx1<=curx<=r.cx2 and r.cy1<=cury<=r.cy2:
            wx = (r.wx == -100) ? curx : r.wx
            wy = (r.wy == -100) ? cury : r.wy
            path[k] = (wx,wy)
            if (wx,wy) != (curx,cury):
                count[pass]++ (pass 0/1 only); curx,cury = wx,wy; k++
                c = 0          # QUIRK: then c++ below -> rescan restarts at c=1:
                               # rule 319 (pass 0/2) / rule 0 (pass 1) is only tried on the first sweep
        c++
    if pass == 0: pass = 1; continue
    if pass == 1 and n0 < n1: pass = 2; continue        # descending result is recomputed when it is shorter
    break                                              # ties -> ascending (pass 1) result kept
fc1f = 0
path[k] = (gx,gy); path[k+1] = (-100,-100)
fcce = 1
```
Notes: no cycle guard (a rule cycle hangs / overflows path). GOXY does NOT reset fc1d/fc1e; the click handler
(and SPEAK) reset them, a script GOXY issued mid-walk keeps the old direction state (quirk).
-100 in GOXY args is not special here: it passes through (no LIMIT box contains x=-100) and becomes the
target; the walker treats a -100 coordinate as "don't move on this axis".
Interpreter: op 2 calls GOXY then yields. An ACTION script is only resumed when fcce==0 and no text (0x7ce6),
so ACTIONs wait for the walk; ANIMATION scripts (0x7f6f) do not wait.

### 2.2 Click-to-walk (0x500d, left button, called each main-loop iteration before the walker)
On left click when allowed (no action running i.e. fcc9==0 or >=0x32, etc.):
`fc1f=0; fc1d=fc1e=0; path[0]=(-100,-100)` (stops current walk), then if no inventory object selected,
no action pending, cursor not over a THING (fc22==0), clickY <= fd22 and verb (fd2c)==0 ("Ir a"):
`GOXY(mouseX+hotX, mouseY+hotY)` (fd5a,fd5c). Clicking a THING goes through 0x8362 (see 7).

### 2.3 Per-tick walker (0x45d2), called once per main-loop iteration

Main loop order (0x574e): level change → ANIMATIONs (0x7f6f) → action/verb state machine (0x7937) →
speech text (0x769c) → mouse (0x500d) → **walker (0x45d2)** → … → layer swap (0x5606) → draw.

```
P  = path[fc1f/2], Pn = path[fc1f/2 + 1]
fx = fc01 + fc0d/2 ; fy = fc03 + fc0f            (recomputed at each use; fc01/fc03 change during the tick)
dirX = fc1d, dirY = fc1e  (bytes; 0 = unset, 1 = +, 2 = -, 3/4 = X "minor axis" variants of 1/2, 0xff = done;
                            comparisons "dirY <= 0" are SIGNED byte compares: 0xff counts as -1)

if P == (-100,-100) and fcbf != 1: goto IDLE

# --- waypoint setup ---
if fcbf == 0 and ((dirX==0 and dirY==0) or (dirX==0xff and dirY==0xff)):
    if P.x == -100: dirX = 0xff
    if P.y == -100: dirY = 0xff
    if P.x > fx and dirX==0: dirX = 1        # right
    if P.x < fx and dirX==0: dirX = 2        # left
    if P.y > fy and dirY==0: dirY = 1        # down (towards camera, "front" frames)
    if P.y < fy and dirY==0: dirY = 2        # up ("back" frames)
    if P.x == fx: dirX = 0xff
    if P.y == fy: dirY = 0xff
    if dirX==0: dirX=0xff ; if dirY==0: dirY=0xff
    if dirX != 0xff and dirY != 0xff:
        ddx = |P.x - fx| ; ddy = |P.y - fy|
        if ddx <= 50 and ddy >= 10: dirX += 2      # 3 = right-minor, 4 = left-minor (vertical frames)

# --- speech animation request ---
if fcbf != 0:
    if fbf5 < [d3]:                              # not yet on a talk frame (SPEAK set fbf5 = 0)
        if fbf7 == [d1]: fbf5 = [d2]; fbf4 = 0           # talk side (flip kept)
        if fbf7 == [cf]: fbf5 = [d3]; fbf4 = 0; fc1b = 0 # talk back
        if fbf7 == [d0]: fbf5 = [d4]; fbf4 = 0; fc1b = 0 # talk front
    else: fbf4 = 1

# --- Y arrival test ---
if dirY == 1:
    if P.y > fy: fbf4 = 1
    else: dirY = 0xff; if Pn == (-100,-100): fc03 = P.y - fc0f
if dirY == 2:
    if P.y < fy: fbf4 = 1
    else: dirY = 0xff; if Pn == (-100,-100): fc03 = P.y - fc0f

# --- X arrival test + side-frame selection ---
if dirX in (1,3):
    if P.x > fx:
        fbf4 = 1
        if dirX == 1 or (dirX == 3 and dirY <= 0):
            if fbf5 > [d1]+[d8]-1 or fbf5 < [d1]+1: fbf5 = [d1]+1
            fc1b = 0
    else: dirX = 0xff
if dirX in (2,4):
    if P.x < fx:
        fbf4 = 1
        if dirX == 2 or (dirX == 4 and dirY <= 0):
            if fbf5 > [d1]+[d8]-1 or fbf5 < [d1]+1: fbf5 = [d1]+1
            fc1b = 1
    else: dirX = 0xff
goto NEXT

IDLE:
    fbf4 = 0
    if [d1] < fbf5 < [d1]+[d8]: fbf5 = [d1]       # back to standing pose of the sequence in use
    if [cf] < fbf5 < [cf]+[d6]: fbf5 = [cf]
    if [d0] < fbf5 < [d0]+[d7]: fbf5 = [d0]
    path[0] = (-100,-100); fcce = 0               # fc1f is NOT reset here
NEXT:
if dirX == 0xff and dirY == 0xff: fc1f += 2; dirX = dirY = 0     # next waypoint (used next tick)

# --- step (animation + movement are coupled) ---
if fbf4 == 1 and fccf == 0:
    fbf4 = 0
    fbf5++
    if fbf5 == [d1]+[d8]-1: fbf5 = [d1]+1         # side walk loop
    if fbf5 == [cf]+[cf]-1: fbf5 = [cf]+1         # BUG (uses cf twice): never true when cf=0
    if fbf5 == [d0]+[d0]-1: fbf5 = [d0]+1         # BUG (uses d0 twice): works only because d0==d7 in demo
    if fbf5 == [d2]+[db]-1: fbf5 = [d2]           # talk side loop (includes first talk frame)
    if fbf5 == [d3]+[d9]-1: fbf5 = [d3]           # talk back loop
    if fbf5 == [d4]+[da]-1: fbf5 = [d4]           # talk front loop
    # X move
    if dirX in (1,3):
        if dirX == 1 or (dirX == 3 and dirY <= 0): fc01 += fcfa
        if dirX == 3 and dirY > 0:                   fc01 += fcfa/2
        if P.x <= fc01 + fc0d/2: fc01 = P.x - fc0d/2         # snap (done twice if Pn is terminator)
    if dirX in (2,4):
        if dirX == 2 or (dirX == 4 and dirY <= 0): fc01 -= fcfa
        if dirX == 4 and dirY > 0:                   fc01 -= fcfa/2
        if P.x >= fc01 + fc0d/2: fc01 = P.x - fc0d/2
    # Y move
    if dirY == 1:
        if dirX in (1,2): v = fcfc/2
        else:
            if fbf5 > [d0]+[d7]-1 or fbf5 < [d0]+1: fbf5 = [d0]+1; fbf4 = 0; fc1b = 0
            v = fcfc
        fc03 += v
        if P.y <= fc03 + fc0f: fc03 = P.y - fc0f
    if dirY == 2:
        if dirX in (1,2): v = fcfc/2
        else:
            if fbf5 > [cf]+[d6]-1 or fbf5 < [cf]+1: fbf5 = [cf]+1; fbf4 = 0; fc1b = 0
            v = fcfc
        fc03 -= v
        if P.y >= fc03 + fc0f: fc03 = P.y - fc0f
    fccf = (fcbf == 0) ? [8a22] : 2*[8a22]

# --- frame source (every tick) ---
if fbf8 == 0: fc09,fc0b = point[fbf5]; fc0d,fc0f = [4334],[4336]; src = MOVSPCX
else:         fc09,fc0b,fc0d,fc0f = fbf9,fbfb,fbfd,fbff;  src = DIBUJOSPCX
fd3c = fc01 + fc0d/2 ; fd3e = fc03 + fc0f
```
Consequences / behaviour summary:
- One step per animation frame, every [8a22] timer ticks (demo: 3 ticks @50 Hz ≈ 16.7 steps/s); x and y move in
  the same step. Mostly-horizontal legs (dirX 1/2): side frames, x at fcfa, y at fcfc/2. Mostly-vertical legs
  (|dx|<=50 and |dy|>=10 at leg start → dirX 3/4): front/back frames, y at fcfc, x at fcfa/2. When one axis
  arrives, the other continues at full speed (and dirX 3/4 switches to side frames once Y is done).
- Arrival on an axis: snap in the step that overshoots; detected (dirX/dirY=0xff) at the next tick; when both are
  done fc1f advances; the following tick sets up the next leg (so each waypoint costs at least one extra tick).
- Walk cycles in the demo: side 13..19, front 7..10 (11 is caught by the d0 bug-rule), back 1..4 (5 is caught by
  the vertical clamp in the same step). With other DEFMOV data the two bugged wrap rules can let a separator
  frame (-100,-100) be displayed for front walking; reproduce literally.
- Final facing = standing frame of the sequence last used (IDLE normalisation); flip kept (left = mirrored side).
- Speech: SPEAK (0x75af) stops walking (fc1f=0, fc1d=fc1e=0, path[0]=term), normalises fbf5 to the standing pose
  (if cf<fbf5<cf+d6-1 etc., exclusive upper bound start+cnt-1), saves it in fbf7, sets fbf5=0, fcbf=1. The walker
  then picks the talk sequence (above) and animates it every 2*[8a22] ticks. If fbf7 is not one of the three
  standing frames, frame 0 is shown un-animated. At text end (0x76e8): fcbf=0, fbf5=fbf7, fc0d/fc0f=DEFMOV w,h
  (even under PMOV), fc17=0xffff (force redraw). PTEXT never animates the player.

### 2.4 LIMITMOV rules (DS:6c72, 320 x 22 bytes, compiler-generated)
```
+0 i16 used (runtime requires ==1)
+2 tx1, +4 ty1, +6 tx2, +8 ty2   box the TARGET must be in (inclusive)
+a cx1, +c cy1, +e cx2, +10 cy2  box the CURRENT/last-waypoint position must be in (inclusive)
+12 wx, +14 wy                   waypoint (-100 = keep that coordinate; compiler never emits it)
```
Stale entries from previously compiled levels may remain with used=0 (ignored).

Compiler (ED.EXE 0x46e3 → gen 0x93d9(x1,y1,w=x2-x1,h=y2-y1)): with X1=x1,Y1=y1,X2=x2,Y2=y2, eight neighbour
regions TL,T,TR,L,R,BL,B,BR all enabled, then: x1<=0 → no TL,L,BL; y1<=0 → no TL,T,TR; X2>=319 → no TR,R,BR;
Y2>=199 → no BL,B,BR. Rules add(cur box, target box, waypoint) are emitted in this order (exact, see
`re/limitmov.py`, verified identical to the compiled levels 1,4,5):
```
TL: R|BR → cur(0,0,X2,Y1)  tgt(X2,Y1,320,200) wp(X2,Y1);  B|BR → cur(0,0,X1,Y2) tgt(X1,Y2,320,200) wp(X1,Y2)
TR: B|BL → cur(X2,0,320,Y2) tgt(0,Y2,X2,200) wp(X2,Y2);   L|BL → cur(X1,0,320,Y1) tgt(0,Y1,X1,200) wp(X1,Y1)
BL: R|TR → cur(0,Y2,X2,200) tgt(X2,0,320,Y2) wp(X2,Y2);   T|TR → cur(0,Y1,X1,200) tgt(X1,0,320,Y1) wp(X1,Y1)
BR: T|TL → cur(X2,Y1,320,200) tgt(0,0,X2,Y1) wp(X2,Y1);   TL|L → cur(X1,Y2,320,200) tgt(0,0,X1,Y2) wp(X1,Y2)
T:  L|BL → cur(X1,0,320,Y1) tgt(0,Y1,X1,200) wp(X1,Y1);   R|BR → cur(0,0,X2,Y1) tgt(X2,Y1,320,200) wp(X2,Y1)
    if B&L:   cur(X1,0,X2,Y1) tgt(X1,Y2,X2,200) wp(X1,Y1); cur(0,Y1,X1,Y2) tgt(X1,Y2,X2,200) wp(X1,Y2)
    elif B&R: cur(X1,0,X2,Y1) tgt(X1,Y2,X2,200) wp(X2,Y1); cur(X2,Y1,320,Y2) tgt(X1,Y2,X2,200) wp(X2,Y2)
L:  T|TR → cur(0,Y1,X1,200) tgt(X1,0,320,Y1) wp(X1,Y1);   B|BR → cur(0,0,X1,Y2) tgt(X1,Y2,320,200) wp(X1,Y2)
    if R&B:   cur(0,Y1,X1,Y2) tgt(X2,Y1,320,Y2) wp(X1,Y2); cur(X1,Y2,X2,200) tgt(X2,Y1,320,Y1)[sic] wp(X2,Y2)
    elif R&T: cur(0,Y1,X1,Y2) tgt(X2,Y1,320,Y2) wp(X1,Y1); cur(X1,0,X2,Y1) tgt(X2,Y1,320,Y2) wp(X2,Y1)
R:  TL|T → cur(X2,Y1,320,200) tgt(0,0,X2,Y1) wp(X2,Y1);   BL|B → cur(X2,0,320,Y2) tgt(0,Y2,X2,200) wp(X2,Y2)
    if L&T:   cur(X2,Y1,320,Y2) tgt(0,Y1,X1,Y2) wp(X2,Y1); cur(X1,0,X2,Y1) tgt(0,Y1,X1,Y2) wp(X1,Y1)
    elif L&B: cur(X2,Y1,320,Y2) tgt(0,Y1,X1,Y2) wp(X2,Y2); cur(X1,Y2,X2,200) tgt(0,Y1,X1,Y2) wp(X1,Y2)
B:  TL|L → cur(X1,Y2,320,200) tgt(0,0,X1,Y2) wp(X1,Y2);   R|TR → cur(0,Y2,X2,200) tgt(X2,0,320,Y2) wp(X2,Y2)
    if T&R:   cur(X1,Y2,X2,200) tgt(X1,0,X2,Y1) wp(X2,Y2); cur(X2,Y1,320,Y2) tgt(X1,0,X2,Y1) wp(X2,Y1)
    elif T&L: cur(X1,Y2,X2,200) tgt(X1,0,X2,Y1) wp(X1,Y2); cur(0,Y1,X1,Y2) tgt(X1,0,X2,Y1) wp(X1,Y1)
```
add (0x9ae3): scan slots while used==1; if a slot has all 10 fields identical → drop. Else store in the first
slot with used==0 (silently dropped if none) and run sort (0x9c9c) over the whole table:
```
for si in 0..319 if T[si].used==1:
  for di in 0..319 if T[di].used==1:
    A=T[di]; B=T[si]
    if A.tx2==B.tx2 and A.ty2==B.ty2 and (A.tx2-A.tx1 > B.tx2-B.tx1 or A.ty2-A.ty1 > B.ty2-B.ty1): swap(T[si],T[di])
    A=T[di]; B=T[si]                       # re-read after possible swap
    if A.tx1==B.tx1 and A.ty1==B.ty1 and (A.tx2-A.tx1 < B.tx2-B.tx1 or A.ty2-A.ty1 < B.ty2-B.ty1): swap(T[si],T[di])
```
(Compiler also has a raw 10-number LIMITMOV form at ED 0x30a4 — old syntax, appends without sorting; unused by demo.)
Manual's advice applies: the target inside a LIMITMOV box is not prevented; cover it with a LIMIT.

## 3. Scaling: SCALEXY / SCALEMOV

SCALEXY(a,b) (op 43): `if a>=0: fcf2=a; if b>=0: fcf4=b` (so -100 = no change). SPEEDXY(a,b) (op 35): same
pattern for fcfa/fcfc. Init: fcf2=fcf4=0, fcfa=10, fcfc=4 (0xb78c, once per game – NOT reset per level).

SCALEMOV table DS:389a, 20 x 18 bytes {i16 used, x1,y1,x2,y2, redW, redH, speedX, speedY}. Evaluated (0x685f)
at level entry and in the draw step whenever fc01 or fc03 differs from the last drawn position:
```
for i in 0..19:
    e = SCALEMOV[i]; if e.used != 1: break            # stops at first unused entry
    if e.x1 <= feetx <= e.x2 and e.y1 <= feety <= e.y2:   # feet = fc01+fc0d/2, fc03+fc0f (inclusive)
        fcf2 = e.redW; fcf4 = e.redH
        if e.speedX != -100: fcfa = e.speedX
        if e.speedY != -100: fcfc = e.speedY
        break                                          # first match wins
# no match: keep previous scale/speeds
```
Speeds written by SCALEMOV persist (also across levels) until another SCALEMOV/SPEEDXY changes them
(e.g. ROOM1: entering y<=107 sets speed 1/2, nearer rows have -100 so the slow speed stays).

Scaled blit 0x429e(srcx,srcy,dstx,dsty,w,h,float redW,float redH,src,dst,flip) called with
dstx = fc01 + fcf2/2, dsty = fc03 + fcf4, w=fc0d, h=fc0f, redW=fcf2, redH=fcf4, flip=fc1b:
```
if not (h > redH): redH = h-1 ; if redH < 0: redH = 0     (same for w/redW)
kH = f32((h-redH)/h) ; kW = f32((w-redW)/w)              # stored as float32
fy = 0.0f ; lastRow = -1
for sr in 0..h-1:
    r = trunc(fy)
    if r != lastRow:
        fx = 0.0f ; lastCol = -1
        cols = (flip==1) ? [1..w-1] : [0..w-1]
        for c in cols:
            d = trunc(fx)
            if d != lastCol:
                sx = (flip==1) ? srcx + w-1 - c  : srcx + c        # flip: srcx+w-2 .. srcx; column srcx+w-1 never drawn
                p = src[(srcy+sr)*320 + sx]
                if p != 0:
                    Y = dsty + r ; X = dstx + d
                    if Y >= 0 and (flip? Y < fd22 : Y <= fd22) and 0 <= X < 320: dst[Y*320+X] = p
                lastCol = d
            fx = f32(fx + kW)
        lastRow = r            # = trunc(fy)
    fy = f32(fy + kH)
```
i.e. nearest-neighbour row/column DROPPING with float32 accumulated steps (truncation, x87 chop); the first
row/column is always kept; result ≈ (w-redW) x (h-redH), horizontally centred (dstx offset redW/2) and
bottom-aligned (dsty offset redH), so the feet point stays fixed. Colour 0 transparent. Quirks: mirrored sprite
loses its rightmost source column and is clipped at y < fd22 instead of <= fd22.
Speech text is anchored at (feetx, fc03+fcf4) i.e. the scaled top.

## 4. SHADOW / SHADOWMOV

SHADOW(first,count) (op 27): fd27=first, fd29=count (no immediate effect). Default 0,0 → shading disabled.
SHADOWMOV table DS:37aa, 20 x 12 bytes {i16 used,x1,y1,x2,y2,shade}. Evaluated (0x6930) right after SCALEMOV
(level entry, and whenever the player position changed):
```
for i in 0..19:
    e = SHADOWMOV[i]; if e.used != 1: break
    if e.x1 <= feetx <= e.x2 and e.y1 <= feety <= e.y2:
        if e.shade != fd25:
            fd25 = e.shade
            if 0 <= fd27 < 256 and fd29 > 0 and fd27+fd29 < 256:
                modpal(first=fd27, count=fd29, shade, src=DS:2280 orig palette, dst=DS:2580 current palette)
            fcd3 = 1        # palette dirty → uploaded to VGA
        break
```
modpal 0x587 (same routine as MODPAL): for each of count*3 bytes (6-bit VGA RGB) starting at first*3:
`dst = clamp(src + shade, 0, 63)` (if count>255 it uses 256-first). Positive brightens, negative darkens, 0
restores original. No match → nothing changes. fd25 is never reset (not even per level): re-entering a region
whose shade equals the last applied value does not re-apply it (e.g. coming back to a level after its palette was
reloaded) – reproduce if exact.

## 5. Drawing order and layer auto-swap

Layer swap (0x5606, every main-loop iteration after the walker): for each THING with mode(+0x2b)!=0 and
layer(+0x15) not 2/3: `bottom = POS.y(+0x1b) + height(+0x23)`; `pf = fc03 + [4336]` (DEFMOV height, not fc0f);
`layer = (bottom >= pf) ? 1 (above player) : 0 (below)`.
Incremental draw (0x5687…): if any of fc01,fc03,fcf2,fcf4,fc0d,fc0f,fc1b,fbf5,fc19 changed since last draw:
(if position changed: SCALEMOV+SHADOWMOV) restore background of old rect (fc05,fc07,fc11,fc13), draw things of
layer 0/2 in DS:8b49 order (0xff skipped), then the player (if fc19!=0) with the scaled blit, then things of
layer 1/3. Full redraw (0x65a0) draws in THING index order 0..29 instead (quirk). Things with mode 0 or 3 are not
drawn. fd22 (var23, default 187) = bottom of play area: player/thing clip, click-to-walk accepted only if
clickY <= fd22.

## 6. PPOS / PMOV / door entry

PPOS(d) (op 39): 0: fbf5=[d1], fc1b=1 (left); 1: fbf5=[d1], fc1b=0; 2: fbf5=[d0], fc1b=0; 3: fbf5=[cf], fc1b=0.
Same mapping is used for THING GOTO dir (+0x44) and DOOR dir.
PMOV(n) (op 45): fc17 = fc19+1; n<0: fbf8=0; else fbf8=1, fbf9..fbff = COORANIM[n] {x,y,w,h}. The walker keeps
animating fbf5 but the drawn image is the cooranim from DIBUJOSPCX (with fc1b mirroring). fc01/fc03 are not
adjusted when w/h change, so the feet point moves.

Level entry (0x1b64), door = DOOR[fcd1] (DS:685e, 10 bytes {used,x,y,dir,action}):
```
if door.used:
    fbf8 = 0 (PMOV off); fc0d,fc0f = [4334],[4336]
    if door.x != -100: fc01 = door.x - fc0d/2         # door coords = feet
    if door.y != -100: fc03 = door.y - fc0f
    if door.dir != -100: fbf6 = fbf5; apply PPOS(door.dir) mapping
    fc09,fc0b = point[fbf5]
    fcc9 = door.action (low byte)  → that ACTION runs (via 0x7ce6) after ACTION 0, on the first main-loop pass
else:
    fc0d,fc0f = DEFMOV w,h; fbf5 = [d1]; fc09 = point.x; fc09 = point.y  (BUG: fc0b not set); fcc9 = 0
    (position/PMOV kept from previous level)
SCALEMOV; SHADOWMOV; (0x6e9b); run ACTION 0 to completion (each yield clears fcce and fca9, so GOXY/SPEAK there
do not block; note the path array set by a GOXY stays and will be walked); fd3c/fd3e = feet; full redraw.
```
fc1d/fc1e/fc1f are not reset at level entry (SPEAK/click reset them).

## 7. THING GOTO via verbs (0x8362 sets up, 0x7937 state machine; fcc9=mode, fcca=state)
thing t = fc22-1 (8a8e), verb v = fd2c; a = ACTIONS[v] (+0x48+2v):
- a>=5000: fd36=a-5000, mode 0x64, state 1 (no walk: face, then SPEAK(fd36))
- a>=4000: mode 0x6a, state 0 (face; then run ACTION (a-4000)&0xff)
- a>=3000: fd36=a-3000, mode 0x64, state 2 (just SPEAK, undocumented)
- a>=2000: fd36=a-2000, mode 0x64, state 0 (walk, face, SPEAK)
- a>=1000: fcc9=(a+0x18)&0xff (= action number) → run immediately, no walk
- a>0: mode 0x65 (walk, face, then run ACTION a)
- a==0 and v==0: fd36=0, mode 0x64 state 0 (walk + face only)
State 0 of 0x64/0x65: `GOXY(t.GOTOx(+0x40), t.GOTOy(+0x42))` → state 1 (GOTO coords are feet coords and use the
full GOXY semantics: LIMIT, +10000, -100 = stay on that axis). State 1 waits fcce==0, then faces by +0x44
(PPOS mapping) → state 2 → speak/action. Mode 0x6a faces when fcce==0 without walking. ACINVTH (modes
0x66/0x68, thing fcdf) behave the same (1/4 walk, 3/6 face, 2/5 stay).

## 8. Uncertainties
- x87 precision: Borland default control word assumed (64-bit mantissa, chop only inside ftol 0xcd6f). Computing
  k = (h-red)/h in double then rounding to float32, and accumulating in float32, should match in practice.
- FREC is not in the compiler command table (ED.DAT) nor in the interpreter op list → unsupported in v2.0.
- Exact timer/main-loop interplay (main loop may run slower than 50 Hz) not measured; stepping is "first walker
  call after fccf reached 0".
