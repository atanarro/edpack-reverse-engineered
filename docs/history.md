# EDPACK versions and games

What is known about the EDPACK family, from the programs themselves and from two archived web
sites: the official EDPACK 2000 site
(<https://web.archive.org/web/20010405161017/http://edpack.hypermart.net/inicio.html>, 2000–2001)
and the site of Dadton's author (<https://web.archive.org/web/2001/http://www.telecable.es/personales/dadtrox1/>).

## The three generations

| Generation | Platform | Interpreter | Data | Supported here |
|---|---|---|---|---|
| EDPACK v1.x (1996) | MS-DOS, 320×200×256 | `RUNVGA.EXE` (v1.03 seen) | `RESOURCE.DAT` (1000-entry table) or `<base>.nnn` files | yes: `src/v1/` |
| EDPACK v2.0x (1997–1998) | MS-DOS, 320×200×256 | `RUNDAT.EXE` (packed) / `RUNFILES.EXE` (loose files) | `EDGAME.DAT` or `NIVEL.nnn` files | yes: `src/engine.rs` |
| EDPACK 2000 (2000–2001) | Windows, DirectX | `runner.exe` (editor `editor.exe`) | its own format | no |

### EDPACK v1

The first version: DOS, 320×200 in 256 colours. The official site later offered it as a free
download ("EDPACK v1.0 … es la primera versión del edpack que hice y funciona en MSDOS … con una
resolución de 320x200 con 256 colores", about 350 KB). The v2.0 order form (`PEDIDO.TXT`) still
sold it ("EDPACK v1.0 … 3000 Pts."). Its interpreter is `RUNVGA.EXE`; the copy shipped with Manolo
Kabezabolo identifies itself as "RUNVGA.EXE v1.03 Por Jose Luis Serrano Moreno" (Sep 1996).
`NUEVO.TXT` of v2.0 lists what v1 lacked: scaling, light/shadow zones, scrolling, areas to walk
around, 30 objects per room and in the inventory, FIRSTFRAME/NEXTFRAME/PMOV, the font editor,
STEXT dialogues, FLI animations. Reverse-engineering notes: [`v1-runvga/`](v1-runvga/).

### EDPACK v2.0x

Shareware and registered editions of the DOS v2. All interpreters identify themselves as
"ED PACK v2.0 … Por Jose Luis Serrano Moreno"; the runtime has its own version number:

| Kit | Runtime | Seen in | Differences |
|---|---|---|---|
| v2.0 / v2.02 shareware (PCManía 67, May 1998) | `RUNDAT.EXE v1.0`, "Version SHAREWARE" | the demo *Peter's Adventure I* | levels 0–5 only, nag screen, 12-entry `EDGAME.DAT` table, filler bytes in level files |
| registered | `RUNDAT.EXE v1.0` (dated 26 Jun 1997) | *El Dios de Nauru* (BYK Software, 2001; its credits say "Creada con EDPACK v2.03") | 1000 levels (1006-entry table), u16 level number, no filler bytes |
| registered v2.02 | `RUNDAT.EXE v1.02` | *Dadton Aguinández* (Coctel Software, 1998: "Desarrollada en Edpack v.2.02") | as registered, plus 12-byte animation records, SKIPTO / PXY / ISANIM, 200-op limit, new variables, UI changes |

Behavioural differences between these runtimes: [`v2-rundat/07-versions.md`](v2-rundat/07-versions.md).

Note that Nauru (2001, "EDPACK v2.03") ships the older v1.0 runtime while Dadton (1998, v2.02)
ships v1.02: the runtime version number does not follow the kit version.

### EDPACK 2000

A complete rewrite for Windows by the author of the official site (zeroq@terra.es), with a new
development environment. Versions, from the site's news: v1.0 / v1.01 beta (August 2000), v1.05
beta (11 Nov 2000, `editor.exe` + `runner.exe`, two demos), v1.051 beta (Dec 2000, adds the help
manual), v1.06 beta (24 Feb 2001, 2.8 MB). Manuals in Spanish and English (Dec 2000); language
files for the editor from v1.05b.

Features listed by the site: resolutions from 320×200 to 1024×768, 8 and 16-bit colour, PCX, JPG
and BMP pictures, DirectX, video and sound through Windows (AVI, MPG, MOV, ASF, WAV, MP3…), a
Pascal-like script language, TrueType text, path finding around objects, walk areas with light,
scaling and priority, several characters on screen, multiple languages, dialogue choices.

Nothing in this repository targets EDPACK 2000: no game made with it has been found, and none
of its files were archived (only the web pages).

## Games

| Game | Authors | Year | Made with | Notes |
|---|---|---|---|---|
| *Las Aventuras de Manolo Kabezabolo* | Jesús Abizanda (Digital Legion), Barbastro | 1997 | EDPACK v1 (`RUNVGA.EXE v1.03`) | `MANOLO.COM` launcher; `RESOURCE.DAT` |
| *Peter's Adventure I* | (the demo shipped with the kit) | 1998 | EDPACK v2.0 shareware | with its sources |
| *Dadton Aguinández* (also spelled Aguinandez / Anguinez) | Tony and Dadtrox (Coctel Software), Avilés | summer 1998, released 16 Jul 1999 (v1.0); 2 Nov 1999 self-extracting re-release | EDPACK v2.02 | freeware; a homage to Monkey Island; `Coctel.exe` shows the Coctel Software logo before the game. The authors later ported its scripts to IndyJava at 640×480 |
| *El Dios de Nauru* | Víctor López Ayerbe (BYK Software) | 2001 | EDPACK v2.03 | ships `SOLUCION.DOC` |

The Dadtrox site describes the controls of Dadton as the standard v2 ones: left button confirms
the action, right button changes it, SPACE opens the load/save/quit menu.

## Related links from the period

From the official site's links page: CAAD (Club de Aventuras AD), IndyJava (a Java engine for
LucasArts-style adventures), ScummRev, PapadaSoft, and Dadtrox's page ("un juego de piratas
creado con el EDPACK v2.0").
