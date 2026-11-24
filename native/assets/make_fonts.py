"""Regenerates assets/fonts/*.ttf: static Archivo instances at the width/weight pairs index.html uses.

    python -m venv v && v/bin/pip install fonttools brotli
    v/bin/python assets/make_fonts.py

Source: Archivo variable font from github.com/google/fonts (SIL OFL 1.1, see fonts/OFL.txt).
GPUI can't set variable-font axes, so each (width, weight) pair becomes its own family name.
"""
import io
import pathlib
import urllib.request

from fontTools.subset import Options, Subsetter
from fontTools.ttLib import TTFont
from fontTools.varLib import instancer

URL = "https://github.com/google/fonts/raw/main/ofl/archivo/Archivo%5Bwdth%2Cwght%5D.ttf"
OUT = pathlib.Path(__file__).parent / "fonts"
# family name, wdth, wght  (names are what theme.rs refers to)
INSTANCES = [
    ("ArchivoR", 100, 400), ("ArchivoM", 100, 500), ("ArchivoSB", 100, 600), ("ArchivoB", 100, 650),
    ("ArchivoC66", 66, 780), ("ArchivoC66L", 66, 300), ("ArchivoC66M", 66, 600),
    ("ArchivoC80", 80, 720), ("ArchivoC82", 82, 680), ("ArchivoC85", 85, 600), ("ArchivoC88", 88, 750),
]
UNICODES = [*range(0x20, 0x7F), *range(0xA0, 0x100), 0x2013, 0x2014, 0x2018, 0x2019, 0x201C, 0x201D, 0x2026]

data = urllib.request.urlopen(URL).read()
for name, wdth, wght in INSTANCES:
    font = instancer.instantiateVariableFont(TTFont(io.BytesIO(data)), {"wdth": wdth, "wght": wght})
    for r in font["name"].names:
        if r.nameID in (1, 4, 6, 16):
            r.string = name
        elif r.nameID in (2, 17):
            r.string = "Regular"
    font["OS/2"].usWeightClass = 400
    opts = Options()
    opts.layout_features = ["*"]
    sub = Subsetter(opts)
    sub.populate(unicodes=UNICODES)
    sub.subset(font)
    font.save(OUT / f"{name}.ttf")
    print("wrote", name)
