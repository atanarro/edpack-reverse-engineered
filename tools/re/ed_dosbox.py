"""Runs the original ED.EXE and ED2DAT.EXE in DOSBox to get reference output for edc.

    python3 tools/re/ed_dosbox.py <kit dir> <sources dir> <out dir> [--chain 4,2,0,1,3,5] [--font LETRAS1.FNT]

<kit dir> holds ED.EXE, ED.DAT and ED2DAT.EXE. Without --chain every .ED file is compiled in
its own DOSBox session, so nothing carries over between files. With --chain the ROOMn.ED files
are compiled in one ED.EXE run in that order (the other .ED files fresh), as the shipped demos
were. Then ED2DAT packs EDGAME.DAT, with --font copied as NIVEL.FNT. <out dir> gets the
NIVEL.* files, EDGAME.DAT and the logs.
"""

import argparse
import glob
import os
import shutil
import subprocess
import tempfile

DOSBOX = os.environ.get('DOSBOX', '/Applications/dosbox.app/Contents/MacOS/DOSBox')
CONF = """[sdl]
output=surface
[cpu]
cycles=max
[mixer]
nosound=true
[sblaster]
sbtype=none
[speaker]
pcspeaker=false
"""


def dosbox(work, *commands):
    conf = os.path.join(work, '..', 'dosbox.conf')
    with open(conf, 'w') as f:
        f.write(CONF)
    args = [DOSBOX, '-conf', conf, '-c', f'mount c {work}', '-c', 'c:']
    for c in commands:
        args += ['-c', c]
    args += ['-c', 'exit']
    subprocess.run(args, stdout=subprocess.DEVNULL, stderr=subprocess.DEVNULL, timeout=300, check=False)


def workdir(root, kit, src):
    w = tempfile.mkdtemp(dir=root)
    for f in glob.glob(os.path.join(src, '*')):
        shutil.copy(f, w)
    for f in ('ED.EXE', 'ED.DAT', 'ED2DAT.EXE'):
        shutil.copy(os.path.join(kit, f), w)
    return w


def main():
    ap = argparse.ArgumentParser()
    ap.add_argument('kit')
    ap.add_argument('src')
    ap.add_argument('out')
    ap.add_argument('--chain', help='room numbers in compile order, e.g. 4,2,0,1,3,5')
    ap.add_argument('--font', default='LETRAS1.FNT')
    a = ap.parse_args()
    os.makedirs(a.out, exist_ok=True)
    root = tempfile.mkdtemp()
    sources = sorted(os.path.basename(f) for f in glob.glob(os.path.join(a.src, '*.ED')))
    chain = [f'ROOM{n}.ED' for n in a.chain.split(',')] if a.chain else []
    for name in sources:
        if name in chain:
            continue
        w = workdir(root, a.kit, a.src)
        dosbox(w, f'ED {name} > LOG.TXT')
        for f in glob.glob(os.path.join(w, 'NIVEL.*')):
            shutil.copy(f, a.out)
    if chain:
        w = workdir(root, a.kit, a.src)
        for i, name in enumerate(chain):  # ED.EXE takes one wildcard; name the files in order
            os.rename(os.path.join(w, name), os.path.join(w, f'Z{i}.ED'))
        dosbox(w, 'ED Z?.ED > LOG.TXT')
        for f in glob.glob(os.path.join(w, 'NIVEL.*')):
            shutil.copy(f, a.out)
        shutil.copy(os.path.join(w, 'LOG.TXT'), os.path.join(a.out, 'CHAIN.LOG'))
    w = workdir(root, a.kit, a.src)
    for f in glob.glob(os.path.join(a.out, 'NIVEL.*')):
        shutil.copy(f, w)
    shutil.copy(os.path.join(a.src, a.font), os.path.join(w, 'NIVEL.FNT'))
    dosbox(w, 'ED2DAT > LOG.TXT')
    shutil.copy(os.path.join(w, 'EDGAME.DAT'), a.out)
    shutil.copy(os.path.join(w, 'LOG.TXT'), os.path.join(a.out, 'ED2DAT.LOG'))
    shutil.rmtree(root)
    for f in sorted(os.listdir(a.out)):
        print(f'{f:<12} {os.path.getsize(os.path.join(a.out, f))}')


if __name__ == '__main__':
    main()
