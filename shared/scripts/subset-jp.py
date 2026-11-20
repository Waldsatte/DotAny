"""Japanese for the family: Noto Sans JP (SIL OFL 1.1), cut to what Japanese interfaces use.

Takes the variable Noto Sans JP from Google Fonts (ofl/notosansjp/NotoSansJP[wght].ttf, version 2.004), fixes it at
Regular (400) and Bold (700), and keeps about 3000 common kanji with kana and punctuation:
  - JIS X 0208 level 1 (2,965 kanji: the everyday ones) and its non-kanji rows (symbols, kana, Greek, Cyrillic, box
    drawing); a kanji cut away is drawn by the system's Japanese font, which family.css names next (Yu Gothic UI,
    Hiragino Sans);
  - the kana blocks whole, small-kana extensions and half-width katakana;
  - CJK symbols and punctuation, full-width forms, Latin-1, general punctuation (Stack Sans draws Latin first anyway).
The family is renamed "Noto Sans JP DotAny", so a full Noto Sans JP someone has installed is never shadowed by this cut
copy (the OFL allows a changed copy under a new name; Noto reserves none). Glyph outlines are unchanged.

    python -I shared/scripts/subset-jp.py <NotoSansJP[wght].ttf> <out folder>
"""
import sys
from pathlib import Path
from fontTools.ttLib import TTFont
from fontTools.varLib import instancer
from fontTools import subset

src, out = Path(sys.argv[1]), Path(sys.argv[2])
NAME = 'Noto Sans JP DotAny'

chars = set()
for hi in [*range(0xA1, 0xA9), *range(0xB0, 0xD0)]:  # JIS X 0208 through EUC-JP: rows 1-8 (symbols, kana) and 16-47 (level 1)
    for lo in range(0xA1, 0xFF):
        try:
            chars.add(ord(bytes([hi, lo]).decode('euc_jp')))
        except UnicodeDecodeError:
            pass
for a, b in [(0x20, 0x7E), (0xA0, 0xFF), (0x2000, 0x206F), (0x3000, 0x303F), (0x3040, 0x309F), (0x30A0, 0x30FF),
             (0x31F0, 0x31FF), (0xFF00, 0xFFEF), (0x2190, 0x21FF), (0x2460, 0x24FF), (0x25A0, 0x25FF)]:
    chars |= set(range(a, b + 1))

for weight, style in [(400, 'Regular'), (700, 'Bold')]:
    font = instancer.instantiateVariableFont(TTFont(src), {'wght': weight})
    opts = subset.Options()
    opts.layout_features = ['*']  # keep vertical forms, kerning, proportional alternates as Noto has them
    opts.name_IDs = ['*']
    opts.name_languages = ['*']
    opts.notdef_outline = True
    opts.glyph_names = False
    sub = subset.Subsetter(opts)
    sub.populate(unicodes=chars)
    sub.subset(font)
    name = font['name']
    for rec in list(name.names):
        if rec.nameID in (1, 16):
            rec.string = NAME
        elif rec.nameID in (2, 17):
            rec.string = style
        elif rec.nameID == 4:
            rec.string = f'{NAME} {style}' if style != 'Regular' else NAME
        elif rec.nameID == 6:
            rec.string = f'NotoSansJPDotAny-{style}'
        elif rec.nameID == 3:
            rec.string = f'2.004;DotAny;NotoSansJPDotAny-{style}'
    font['OS/2'].usWeightClass = weight
    font['OS/2'].fsSelection = (font['OS/2'].fsSelection & ~0b1100001) | (0b100000 if style == 'Bold' else 0b1000000)
    font['head'].macStyle = 1 if style == 'Bold' else 0
    path = out / f'NotoSansJP-DotAny-{style}.ttf'
    font.save(path)
    print(path, round(path.stat().st_size / 1048576, 2), 'MB', len(font.getBestCmap()), 'characters')
