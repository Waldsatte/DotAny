// HTJ2K (JPEG 2000 Part 15) is left out on purpose: a PDF carries JPX baseline (ISO 32000-2, 7.4.9), which is Part 1,
// so a Part 15 stream is not decoded even though PDF.js's OpenJPEG could. This looks at a JPEG 2000 stream before the
// decoder does and answers true when it uses anything of Part 15: an HT code block style in COD or COC, a CAP marker
// (the extended capabilities only Part 15 uses), or Rsiz saying so. Such a stream is refused the way a damaged picture
// is; the rest of the page draws. scripts/build.mjs puts this function into PDF.js's worker, ahead of
// JpxImage.decode. Kept free of imports so it can be copied in as it is.
export function isHtj2k(data) {
  const b = data instanceof Uint8Array ? data : new Uint8Array(data);
  let i = 0;
  // a JP2 file: its codestream is in the 'jp2c' box
  if (b.length > 12 && b[4] === 0x6a && b[5] === 0x50 && b[6] === 0x20 && b[7] === 0x20) {
    let at = 0;
    i = -1;
    while (at + 8 <= b.length) {
      let len = ((b[at] << 24) | (b[at + 1] << 16) | (b[at + 2] << 8) | b[at + 3]) >>> 0,
        head = 8;
      const type = String.fromCharCode(b[at + 4], b[at + 5], b[at + 6], b[at + 7]);
      if (len === 1 && at + 16 <= b.length) {
        len = Number(
          (BigInt(b[at + 8]) << 56n)
            | (BigInt(b[at + 9]) << 48n)
            | (BigInt(b[at + 10]) << 40n)
            | (BigInt(b[at + 11]) << 32n)
            | (BigInt(b[at + 12]) << 24n)
            | (BigInt(b[at + 13]) << 16n)
            | (BigInt(b[at + 14]) << 8n)
            | BigInt(b[at + 15])
        );
        head = 16;
      }
      if (type === 'jp2c') {
        i = at + head;
        break;
      }
      if (len === 0) break;
      at += len;
    }
    if (i < 0) return false;
  }
  // the codestream: SOC, then the main header's markers up to the first tile
  if (b[i] !== 0xff || b[i + 1] !== 0x4f) return false;
  i += 2;
  while (i + 4 <= b.length) {
    if (b[i] !== 0xff) return false;
    const m = b[i + 1],
      len = (b[i + 2] << 8) | b[i + 3];
    if (m === 0x90 || m === 0x93 || m === 0xd9) return false;
    const seg = i + 4;
    if (m === 0x50) return true;
    if (m === 0x51 && ((b[seg] << 8) | b[seg + 1]) & 0x4000) return true;
    // COD: Scod, then SGcod (4 bytes), then SPcod: levels, xcb, ycb, the code-block style
    if (m === 0x52 && b[seg + 8] & 0x40) return true;
    // COC: Ccoc (1 or 2 bytes, 2 with more than 256 components; HT sits in the same style bit either way), Scoc, then SPcoc
    if (m === 0x53 && (b[seg + 5] & 0x40 || (b[seg + 6] & 0x40 && len > 9))) return true;
    i += 2 + len;
  }
  return false;
}
