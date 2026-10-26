"""Print one function of an EDPACK interpreter.

    python tools/re/fn.py rundat 78a8
"""

import sys

from image import Image


def main():
    img = Image(sys.argv[1])
    start = int(sys.argv[2], 16)
    funcs = img.functions()
    prev = None
    for i in img.listing(start):
        if prev is not None and i.address != prev:
            print('        ...')
        tgt = ''
        if i.mnemonic == 'call' and i.op_str.startswith('0x'):
            tgt = '' if int(i.op_str, 16) in funcs else '   ; (not a known function)'
        print(f'  {i.address:04x}  {i.bytes.hex():<14} {i.mnemonic:<7} {i.op_str}{tgt}')
        prev = i.address + i.size


if __name__ == '__main__':
    main()
