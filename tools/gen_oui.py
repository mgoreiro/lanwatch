#!/usr/bin/env python3
"""Genera data/oui.bin (base compacta de fabricantes por MAC) a partir del CSV oficial de la IEEE.

    curl -o oui.csv https://standards-oui.ieee.org/oui/oui.csv        # MA-L (24 bit)
    curl -o oui28.csv https://standards-oui.ieee.org/oui28/mam.csv    # MA-M (28 bit, opcional)
    curl -o oui36.csv https://standards-oui.ieee.org/oui36/oui36.csv  # MA-S (36 bit, opcional)
    python3 tools/gen_oui.py oui.csv [oui28.csv] [oui36.csv]

Formato (little endian): "OUI2", 3 x (u32 n, n x (u64 prefijo, u32 offset)) para 24/28/36 bit,
y al final el pool de nombres (cadenas UTF-8 terminadas en NUL, deduplicadas).
"""
import csv, struct, sys, re

def short(name):
    name = re.sub(r"\s+", " ", name).strip()
    name = re.sub(r"[,\s]+(inc|llc|ltd|co|corp|corporation|limited|gmbh|s\.?a\.?|b\.?v\.?|ag|oy|ab|plc)\.?(?=[,\s]|$)", "", name, flags=re.I)
    return name.strip(" ,.") or "?"

def load(path):
    rows = {}
    with open(path, newline="", encoding="utf-8", errors="replace") as f:
        r = csv.reader(f); next(r)
        for row in r:
            if len(row) >= 3 and row[1]:
                rows[int(row[1], 16)] = short(row[2])
    return rows

tables = [load(p) if i < len(sys.argv) - 1 else {} for i, p in enumerate(sys.argv[1:4])]
while len(tables) < 3: tables.append({})
pool, offs = bytearray(b"\0"), {}
def off(n):
    if n not in offs:
        offs[n] = len(pool); pool.extend(n.encode() + b"\0")
    return offs[n]
out = bytearray(b"OUI2")
for t in tables:
    out += struct.pack("<I", len(t))
    for k in sorted(t): out += struct.pack("<QI", k, off(t[k]))
out += pool
open("data/oui.bin", "wb").write(out)
print("data/oui.bin:", len(out), "bytes;", [len(t) for t in tables], "entradas")
