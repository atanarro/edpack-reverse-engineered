"""The unpacked interpreters: code image, data segment and function discovery."""

import os
import struct
from functools import lru_cache

from capstone import Cs, CS_ARCH_X86, CS_MODE_16
from capstone.x86 import X86_OP_IMM

import dosemu

GAMES = os.path.join(os.path.dirname(__file__), '..', '..', 'games')

# name -> (zip, member, jump tables as (offset, count))
PROGRAMS = {
    'rundat': ('edpack-v2.0-shareware.zip', 'edpack-v2.0-shareware/RUNDAT.EXE', [(0x3c11, 66)]),
    'runvga': ('edpack-v1-demo.zip', 'edpack-v1-demo/RUNVGA.EXE', []),
}


def exe_path(name):
    import zipfile
    zname, member, _ = PROGRAMS[name]
    cache = os.path.join(os.path.dirname(__file__), '.cache')
    out = os.path.join(cache, os.path.basename(member))
    if not os.path.exists(out):
        os.makedirs(cache, exist_ok=True)
        with zipfile.ZipFile(os.path.join(GAMES, zname)) as z, open(out, 'wb') as f:
            f.write(z.read(member))
    return out


class Image:
    def __init__(self, name):
        self.name = name
        self.dos, self.cs, entry_ip = dosemu.unpack(exe_path(name))
        self.entry = entry_ip
        # Borland start-up: "mov dx, DGROUP" is the first instruction
        code = self.read(entry_ip, 3)
        assert code[0] == 0xba, 'unexpected start-up'
        self.ds = struct.unpack_from('<H', code, 1)[0]
        self.code_size = (self.ds - self.cs) * 16
        self.md = Cs(CS_ARCH_X86, CS_MODE_16)
        self.md.detail = True
        self.jump_tables = PROGRAMS[name][2]

    def read(self, off, n):
        return bytes(self.dos.uc.mem_read(self.cs * 16 + off, n))

    def data(self, off, n):
        return bytes(self.dos.uc.mem_read(self.ds * 16 + off, n))

    def insns(self, start, end=None):
        end = end or self.code_size
        return self.md.disasm(self.read(start, end - start), start)

    @lru_cache(maxsize=None)
    def functions(self):
        """Near-call targets reached by recursive descent from the entry point and the jump
        tables. Returns {start: end} where end is the address past the last instruction
        reached from start without following calls."""
        starts = {self.entry}
        for tab, n in self.jump_tables:
            for i in range(n):
                starts.add(struct.unpack_from('<H', self.read(tab + 2 * i, 2))[0])
        funcs = {}
        todo = sorted(starts)
        while todo:
            f = todo.pop()
            if f in funcs:
                continue
            seen, calls = self._walk(f)
            funcs[f] = max(seen) if seen else f
            for c in calls:
                if c not in funcs and 0 <= c < self.code_size:
                    todo.append(c)
        return funcs

    def _walk(self, start):
        seen, calls, work = {}, set(), [start]
        while work:
            pc = work.pop()
            while pc not in seen and 0 <= pc < self.code_size:
                try:
                    i = next(self.md.disasm(self.read(pc, 16), pc))
                except StopIteration:
                    break
                seen[pc] = pc + i.size
                m = i.mnemonic
                imm = i.operands[0].imm & 0xffff if i.operands and i.operands[0].type == X86_OP_IMM else None
                if m == 'call' and imm is not None and i.size == 3:
                    calls.add(imm)
                elif m == 'jmp':
                    if imm is not None and i.size in (2, 3):
                        work.append(imm)
                    break
                elif m.startswith('j') or m in ('loop', 'loope', 'loopne', 'jcxz'):
                    if imm is not None:
                        work.append(imm)
                elif m in ('ret', 'retf', 'iret', 'hlt'):
                    break
                pc += i.size
        return set(seen.values()), calls

    def listing(self, start):
        """Disassembly of one function: the instructions reached from start, in address order."""
        seen = {}
        work = [start]
        while work:
            pc = work.pop()
            while pc not in seen and 0 <= pc < self.code_size:
                i = next(self.md.disasm(self.read(pc, 16), pc))
                seen[pc] = i
                m = i.mnemonic
                imm = i.operands[0].imm & 0xffff if i.operands and i.operands[0].type == X86_OP_IMM else None
                if m == 'jmp':
                    if imm is not None and i.size in (2, 3):
                        work.append(imm)
                    break
                if (m.startswith('j') or m.startswith('loop')) and imm is not None:
                    work.append(imm)
                if m in ('ret', 'retf', 'iret', 'hlt'):
                    break
                pc += i.size
        return [seen[a] for a in sorted(seen)]
