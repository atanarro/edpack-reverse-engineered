"""Differential-test oracle: runs one function of the original interpreter in isolation and
records test vectors (DS bytes in -> DS bytes out) for the Rust re-implementation.

Inputs are found by coverage-guided fuzzing: every DS word or byte the function reads becomes
an input, and mutations that reach new instructions of the original are kept. The vectors
record only what the original read or wrote, so the Rust side can replay them on a zeroed
`Memory` without needing the original data segment.

    python tools/re/oracle.py rundat 685f [--args=lo:hi,...] [--seeds FILE] [--cases 300] [--out DIR]
"""

import argparse
import json
import os
import random
import struct
import sys

from unicorn import UC_HOOK_CODE, UC_HOOK_MEM_READ, UC_HOOK_MEM_WRITE, UcError
from unicorn.x86_const import (
    UC_X86_REG_AX, UC_X86_REG_CS, UC_X86_REG_DS, UC_X86_REG_ES, UC_X86_REG_IP, UC_X86_REG_SP,
    UC_X86_REG_SS, UC_X86_REG_BP,
)

from image import Image

STACK_TOP = 0xfff0      # SS = DS, as in Borland's small/medium models
STACK_FLOOR = 0xfe00    # DS bytes at or above this are the test's stack, not state
MAX_INSNS = 2_000_000


class Fatal(Exception):
    pass


class Oracle:
    def __init__(self, prog, addr, fatal=None):
        self.img = Image(prog)
        self.uc = self.img.dos.uc
        self.addr = addr
        self.fatal = fatal
        self.ds_lin = self.img.ds * 16
        self.cs_lin = self.img.cs * 16
        self.pristine = bytes(self.uc.mem_read(0, 0x100000))
        self.uc.hook_add(UC_HOOK_MEM_READ, self._on_read)
        self.uc.hook_add(UC_HOOK_MEM_WRITE, self._on_write)
        self.uc.hook_add(UC_HOOK_CODE, self._on_code, begin=self.cs_lin, end=self.cs_lin + self.img.code_size)

    # -- hooks -------------------------------------------------------------------------
    def _ds_off(self, a):
        o = a - self.ds_lin
        return o if 0 <= o < STACK_FLOOR else None

    def _on_read(self, uc, _acc, a, size, _v, _ud):
        for k in range(size):
            o = self._ds_off(a + k)
            if o is None:
                if not (self.ds_lin + STACK_FLOOR <= a + k < self.ds_lin + 0x10000):
                    self.far.add(a + k)
                continue
            if o not in self.written:
                self.reads.setdefault(o, size)

    def _on_write(self, uc, _acc, a, size, _v, _ud):
        for k in range(size):
            o = self._ds_off(a + k)
            if o is None:
                if not (self.ds_lin + STACK_FLOOR <= a + k < self.ds_lin + 0x10000):
                    self.far.add(a + k)
                continue
            self.written.add(o)

    def _on_code(self, uc, a, _size, _ud):
        ip = a - self.cs_lin
        if ip == 0:  # the return trap
            self.returned = True
            uc.emu_stop()
            return
        self.cov.add(ip)
        if ip == self.fatal:
            sp = uc.reg_read(UC_X86_REG_SP)
            code = struct.unpack('<H', uc.mem_read(self.ds_lin + sp + 2, 2))[0]
            self.fatal_code = code
            uc.emu_stop()

    # -- one call ----------------------------------------------------------------------
    def call(self, inputs, args=()):
        """inputs: {ds_offset: (size, value)}. Returns a result dict."""
        uc = self.uc
        uc.mem_write(0, self.pristine)
        for o, (size, v) in inputs.items():
            uc.mem_write(self.ds_lin + o, (v & ((1 << 8 * size) - 1)).to_bytes(size, 'little'))
        before = bytes(uc.mem_read(self.ds_lin, 0x10000))
        uc.mem_write(self.cs_lin, b'\xeb\xfe')  # "jmp $" at CS:0, the return address
        uc.ctl_remove_cache(self.cs_lin, self.cs_lin + 16)  # the start-up code there was translated
        sp = STACK_TOP
        for v in reversed(args):
            sp -= 2
            uc.mem_write(self.ds_lin + sp, struct.pack('<h', v))
        sp -= 2
        uc.mem_write(self.ds_lin + sp, b'\x00\x00')
        for r, v in ((UC_X86_REG_CS, self.img.cs), (UC_X86_REG_DS, self.img.ds),
                     (UC_X86_REG_ES, self.img.ds), (UC_X86_REG_SS, self.img.ds),
                     (UC_X86_REG_SP, sp), (UC_X86_REG_BP, 0), (UC_X86_REG_IP, self.addr)):
            uc.reg_write(r, v)
        self.reads, self.written, self.far, self.cov = {}, set(), set(), set()
        self.fatal_code = None
        self.returned = False
        err = None
        try:
            uc.emu_start(self.cs_lin + self.addr, self.cs_lin, count=MAX_INSNS)
        except UcError as e:
            err = f'{e} at {uc.reg_read(UC_X86_REG_IP):04x}'
        ip = uc.reg_read(UC_X86_REG_IP)
        if err is None and self.fatal_code is None and not (self.returned or ip == 0):
            err = f'did not return (ip {ip:04x})'
        after = bytes(uc.mem_read(self.ds_lin, 0x10000))
        return {
            'pre': {o: before[o] for o in set(self.reads) | self.written},
            'out': {o: after[o] for o in self.written if after[o] != before[o]},
            'ax': uc.reg_read(UC_X86_REG_AX),
            'fatal': self.fatal_code,
            'far': self.far,
            'cov': set(self.cov),
            'reads': dict(self.reads),
            'err': err,
        }


# -- input generation -----------------------------------------------------------------
INTERESTING = [0, 1, 2, 3, -1, -2, -100, 100, 160, 200, 255, 256, 319, 320, 0x7fff, -0x8000]


def rand_value(rng, size, pool):
    r = rng.random()
    if r < 0.35:
        v = rng.choice(INTERESTING)
    elif r < 0.6 and pool:
        v = rng.choice(pool) + rng.choice((0, 0, 0, 1, -1))
    elif r < 0.85:
        v = rng.randrange(0, 321)
    else:
        v = rng.randrange(-0x8000, 0x8000)
    return v if size > 1 else v & 0xff


def accesses(reads):
    """Collapse byte reads into (offset, size) accesses."""
    out, skip = {}, set()
    for o in sorted(reads):
        if o in skip:
            continue
        size = min(reads[o], 2)
        out[o] = size
        skip.update(range(o, o + size))
    return out


def fuzz(oracle, args_gen, n_cases, rng, rounds=4000, seeds=()):
    known = {}          # offset -> size of every input seen so far
    corpus = []         # (inputs, args) that reached new instructions
    covered = set()
    results = []

    def run(inputs, args):
        r = oracle.call(inputs, args)
        new_reads = {o: s for o, s in accesses(r['reads']).items() if o not in known}
        known.update(new_reads)
        if not r['cov'] <= covered or new_reads:
            covered.update(r['cov'])
            corpus.append((inputs, args))
        return r

    run({}, args_gen(rng))
    for inputs, args in seeds:
        run(inputs, args)
    for i in range(rounds):
        pool = [v for inp, _ in corpus for (_, v) in inp.values()]
        if corpus and rng.random() < 0.7:
            base, args = rng.choice(corpus)
            inputs = dict(base)
            for _ in range(rng.randint(1, 4)):
                o = rng.choice(list(known))
                inputs[o] = (known[o], rand_value(rng, known[o], pool))
            if rng.random() < 0.2:
                args = args_gen(rng)
        else:
            inputs = {o: (s, rand_value(rng, s, pool)) for o, s in known.items() if rng.random() < 0.5}
            args = args_gen(rng)
        run(inputs, args)
    # vectors: the whole corpus, then random mutations of it
    picks = list(corpus)
    while len(picks) < n_cases:
        base, args = rng.choice(corpus)
        inputs = dict(base)
        pool = [v for (_, v) in inputs.values()]
        for _ in range(rng.randint(1, 3)):
            o = rng.choice(list(known))
            inputs[o] = (known[o], rand_value(rng, known[o], pool))
        picks.append((inputs, args))
    for inputs, args in picks[:max(n_cases, len(corpus))]:
        results.append((args, oracle.call(inputs, args)))
    return results, covered


def write_vectors(path, header, results):
    with open(path, 'w') as f:
        f.write(f'# {header}\n')
        for args, r in results:
            if r['err']:
                continue
            f.write('case ' + ' '.join(str(a) for a in args) + '\n')
            f.write('in ' + ' '.join(f'{o:04x}={v:02x}' for o, v in sorted(r['pre'].items())) + '\n')
            f.write('out ' + ' '.join(f'{o:04x}={v:02x}' for o, v in sorted(r['out'].items())) + '\n')
            f.write(f"ax {r['ax']:04x}\n")
            if r['fatal'] is not None:
                f.write(f"fatal {r['fatal']}\n")
            f.write('end\n')


FATAL = {'rundat': 0xa836}


def main():
    ap = argparse.ArgumentParser()
    ap.add_argument('prog')
    ap.add_argument('addr')
    ap.add_argument('--args', default='', help="argument ranges, e.g. '-50:400,-50:250'")
    ap.add_argument('--cases', type=int, default=300)
    ap.add_argument('--rounds', type=int, default=4000)
    ap.add_argument('--seed', type=int, default=1)
    ap.add_argument('--seeds', help='JSON file of hand-made starting cases: '
                    '[{"args": [..], "words": {"hex offset": value}, "bytes": {...}}]')
    ap.add_argument('--out', default=os.path.join(os.path.dirname(__file__), '..', '..', 'tests', 'oracle'))
    a = ap.parse_args()
    addr = int(a.addr, 16)
    ranges = [tuple(int(x) for x in r.split(':')) for r in a.args.split(',') if r]

    def args_gen(rng):
        return tuple(rng.choice(INTERESTING) if rng.random() < 0.2 else rng.randint(lo, hi) for lo, hi in ranges)

    o = Oracle(a.prog, addr, FATAL.get(a.prog))
    rng = random.Random(a.seed)
    seeds = []
    if a.seeds:
        for c in json.load(open(a.seeds)):
            inputs = {int(k, 16): (2, v) for k, v in c.get('words', {}).items()}
            inputs.update({int(k, 16): (1, v) for k, v in c.get('bytes', {}).items()})
            seeds.append((inputs, tuple(c.get('args', ()))))
    results, covered = fuzz(o, args_gen, a.cases, rng, a.rounds, seeds)
    body = {i.address for i in o.img.listing(addr)}
    errs = [r['err'] for _, r in results if r['err']]
    far = set().union(*(r['far'] for _, r in results))
    os.makedirs(a.out, exist_ok=True)
    path = os.path.join(a.out, f'{a.prog}_{addr:04x}.vec')
    hit = len(body & covered)
    write_vectors(path, f'{a.prog} {addr:04x} coverage {hit}/{len(body)} instructions', results)
    print(f'{a.prog} {addr:04x}: {len(results) - len(errs)} vectors, '
          f'{hit}/{len(body)} instructions of the function covered -> {os.path.relpath(path)}')
    missed = sorted(body - covered)
    if missed:
        print('  never executed: ' + ' '.join(f'{x:04x}' for x in missed[:40]))
    if far:
        print(f'  WARNING: touched {len(far)} bytes outside DS (far data is not captured)')
    if errs:
        print(f'  {len(errs)} calls failed, e.g. {errs[0]}')


if __name__ == '__main__':
    sys.exit(main())
