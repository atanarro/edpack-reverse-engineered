"""Minimal real-mode DOS loader on top of unicorn, enough to run the EDPACK interpreters'
start-up code and then call single functions of them in isolation."""

import struct

from unicorn import Uc, UcError, UC_ARCH_X86, UC_MODE_16, UC_HOOK_INTR, UC_HOOK_CODE
from unicorn.x86_const import (
    UC_X86_REG_AX, UC_X86_REG_BX, UC_X86_REG_CX, UC_X86_REG_DX, UC_X86_REG_SI, UC_X86_REG_DI,
    UC_X86_REG_BP, UC_X86_REG_SP, UC_X86_REG_IP, UC_X86_REG_CS, UC_X86_REG_DS, UC_X86_REG_ES,
    UC_X86_REG_SS, UC_X86_REG_FLAGS,
)

MEM_SIZE = 0x110000
PSP_SEG = 0x0ff0          # program loaded at 0x1000
LOAD_SEG = PSP_SEG + 0x10
HALT_SEG = 0xf000         # far address F000:0000 holds a HLT used as a return trap
TOP_SEG = 0x9fff


class Stop(Exception):
    pass


class Dos:
    def __init__(self, exe_path):
        self.uc = Uc(UC_ARCH_X86, UC_MODE_16)
        self.uc.mem_map(0, MEM_SIZE)
        self.log = []
        self.stop_on_ah30 = False
        self.entry = None
        self._load(open(exe_path, 'rb').read())
        self.uc.mem_write(HALT_SEG * 16, b'\xf4')
        self.uc.hook_add(UC_HOOK_INTR, self._intr)

    # -- loading -------------------------------------------------------------------------
    def _load(self, exe):
        (magic, last, pages, nrel, hdr, minalloc, maxalloc, ss, sp, _ck, ip, cs, relo) = \
            struct.unpack_from('<2sHHHHHHHHHHHH', exe)
        assert magic == b'MZ'
        size = pages * 512 - (512 - last if last else 0)
        image = exe[hdr * 16:size]
        self.uc.mem_write(LOAD_SEG * 16, image)
        for i in range(nrel):
            off, seg = struct.unpack_from('<HH', exe, relo + 4 * i)
            a = (LOAD_SEG + seg) * 16 + off
            v = struct.unpack('<H', self.uc.mem_read(a, 2))[0]
            self.uc.mem_write(a, struct.pack('<H', (v + LOAD_SEG) & 0xffff))
        psp = bytearray(256)
        psp[0:2] = b'\xcd\x20'
        psp[2:4] = struct.pack('<H', TOP_SEG)
        psp[0x2c:0x2e] = struct.pack('<H', 0x0f00)  # environment
        psp[0x80] = 0
        psp[0x81] = 0x0d
        self.uc.mem_write(PSP_SEG * 16, bytes(psp))
        self.uc.mem_write(0x0f00 * 16, b'PATH=C:\\\x00\x00\x01\x00C:\\RUNDAT.EXE\x00')
        r = self.uc.reg_write
        r(UC_X86_REG_CS, LOAD_SEG + cs); r(UC_X86_REG_IP, ip)
        r(UC_X86_REG_SS, LOAD_SEG + ss); r(UC_X86_REG_SP, sp)
        r(UC_X86_REG_DS, PSP_SEG); r(UC_X86_REG_ES, PSP_SEG)
        self.heap_seg = LOAD_SEG + (len(image) + 15) // 16 + 0x1000

    # -- DOS services --------------------------------------------------------------------
    def reg(self, r):
        return self.uc.reg_read(r)

    def _cf(self, on):
        f = self.uc.reg_read(UC_X86_REG_FLAGS)
        self.uc.reg_write(UC_X86_REG_FLAGS, (f | 1) if on else (f & ~1))

    def _intr(self, uc, intno, _ud):
        ax = self.reg(UC_X86_REG_AX)
        ah = ax >> 8
        if intno == 0x21:
            if ah == 0x30:
                if self.stop_on_ah30:
                    # Borland's c0 starts "mov dx,DGROUP; mov cs:[..],dx; mov ah,30h; int 21h"
                    self.entry = (self.reg(UC_X86_REG_CS), self.reg(UC_X86_REG_IP) - 12)
                    raise Stop()
                uc.reg_write(UC_X86_REG_AX, 0x0005)  # DOS 5.0
            elif ah == 0x4a:  # resize block
                self._cf(False)
            elif ah == 0x48:  # allocate
                uc.reg_write(UC_X86_REG_AX, self.heap_seg)
                self.heap_seg += self.reg(UC_X86_REG_BX)
                self._cf(False)
            elif ah == 0x35:
                uc.reg_write(UC_X86_REG_ES, 0); uc.reg_write(UC_X86_REG_BX, 0)
            elif ah in (0x25, 0x1a, 0x0e, 0x19):
                pass
            elif ah == 0x44:
                uc.reg_write(UC_X86_REG_DX, 0x80d3 if self.reg(UC_X86_REG_BX) < 3 else 0)
                self._cf(False)
            elif ah == 0x40:
                uc.reg_write(UC_X86_REG_AX, self.reg(UC_X86_REG_CX)); self._cf(False)
            elif ah == 0x4c:
                raise Stop('exit')
            else:
                self.log.append(('int21', hex(ax)))
                self._cf(True); uc.reg_write(UC_X86_REG_AX, 2)
        else:
            self.log.append(('int', hex(intno), hex(ax)))

    def run(self, max_insns=50_000_000):
        cs, ip = self.reg(UC_X86_REG_CS), self.reg(UC_X86_REG_IP)
        try:
            self.uc.emu_start(cs * 16 + ip, MEM_SIZE, count=max_insns)
        except Stop:
            pass
        except UcError as e:
            raise RuntimeError(f'{e} at {self.reg(UC_X86_REG_CS):04x}:{self.reg(UC_X86_REG_IP):04x}')


def unpack(exe_path):
    """Run the EXE until Borland's start-up asks for the DOS version (int 21h/30h): at that
    point a packed program has been decompressed. Returns (dos, entry_cs, entry_ip)."""
    d = Dos(exe_path)
    d.stop_on_ah30 = True
    d.run()
    assert d.entry, 'never reached the C start-up'
    return d, d.entry[0], d.entry[1]
