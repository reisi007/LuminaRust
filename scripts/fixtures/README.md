# Fixture files for the golden-reference guard

Hand-made, machine-checkable inputs for `scripts/golden_ref_test.sh`. Nothing
in here is a product artefact, a render result or a licence-relevant image: the
guard's regression suite needs a few inputs whose bytes are **stated in the
repo** instead of produced by the code under test.

## `png_ihdr_probe.png`

33 bytes: the 8-byte PNG signature plus one `IHDR` chunk. It is **not** a
decodable image and is never decoded — `png_size()`
(`scripts/golden_ref.sh`) reads exactly the 8 bytes at offset 16..23, and that
is all the suite asks of it.

`IHDR` is, per the PNG spec, `width` (4x u32 **big endian**) then `height`
(4x u32 big endian) at offset 16 of the chunk data area, i.e. at file offset 16
because the signature is 8 bytes and the chunk length+type header is 8 more.
The bytes written here are:

```text
89 50 4e 47 0d 0a 1a 0a   PNG signature
00 00 00 0d               IHDR data length = 13
49 48 44 52               "IHDR"
01 02 03 04               width  = 0x01020304 = 16909060
05 06 07 08               height = 0x05060708 = 84281096
08                        bit depth 8
00                        colour type 0 (grayscale)
00                        compression method
00                        filter method
00                        interlace method
1a 48 52 84               CRC-32 of the 17 bytes "IHDR" + 13 data bytes
```

The dimensions are deliberately **implausible as an image** (16 909 060 x
84 281 096): every one of the four bytes in each dimension is non-zero and
distinct, so swapping the two high bytes, swapping the two halves, reading
little endian or reading only the first two bytes each all yield a *different*
`WxH` string. A plausible size like `1024x720` could not distinguish a correct
read from one that only looks at the leading two bytes.

Because the file is written from a literal byte list, it is reproducible
without the code it is used to test. Regenerate it with:

```sh
sh -c "umask 022; printf '\211PNG\r\n\032\n\000\000\000\015IHDR\001\002\003\004\005\006\007\010\010\000\000\000\000\032HR\204' > scripts/fixtures/png_ihdr_probe.png"
```

Verify it (both are independent of `scripts/golden_ref.sh`):

```sh
od -An -tx1 -v scripts/fixtures/png_ihdr_probe.png
file    scripts/fixtures/png_ihdr_probe.png   # -> PNG image data, 16909060 x 84281096
shasum -a 256 scripts/fixtures/png_ihdr_probe.png
# 929183a439924f32039d9a161b5479bd72393719a41753da53e793b62e5a6821
```

The `CRC-32` was taken once from the PNG spec's CRC over
`"IHDR" || width || height || 5 bytes` (`zlib.crc32`, value `0x1a485284`); it is
not asserted by the suite, because the guard never validates it and a
CRC implementation in the test would be a second thing to get wrong.

## `png_pixel_probe.png`

78 bytes: a **decodable** 4×2, 8-bit truecolour PNG. Unlike `png_ihdr_probe.png`
this one *is* decoded, and that is the point. `scripts/golden_ref_test.sh`
measures the committed goldens with an **external tool** (ImageMagick) to
implement `feature/quality/golden-fixtures.md` §2 rule 7 (P1 distinct colours,
P2 red fraction). A measurement whose correctness is taken on trust from a
third-party binary is a measurement nobody checked, so the suite asserts the
tool against **this** file, whose pixels are listed below and can be counted by
eye.

The eight pixels, in PNG scanline order (each scanline is prefixed by one
filter byte `0x00`, which is not a pixel):

```text
row 0:  (255, 0, 0)   (0, 255, 0)   (0, 0, 255)   (255, 0, 0)
row 1:  (255, 0, 0)   (0, 0, 0)     (0, 0, 255)   (0, 255, 0)
```

Derived by hand from that list, and asserted as **literals** by the suite (never
computed from the tool being tested):

| Property | Literal value | How it is counted |
| --- | --- | --- |
| distinct colours | **4** | red, green, blue, black — each occurs at least twice |
| strongly-red fraction | **0.375** | the three `(255, 0, 0)` pixels; `r > 0.5 ∧ g < 0.25 ∧ b < 0.25` holds for them and for nothing else. Green fails on `g`, blue on `b`, black on `r`. 3/8 = 0.375 |

The file is deliberately built so that **both** degenerate failure modes are
detectable: a tool that classified every pixel as red would report `1.0`, and
one that classified none would report `0.0`. Neither equals `0.375`.

Regenerate it (needs only Python's `zlib`, no image library, no ImageMagick):

```sh
python3 - <<'PY'
import struct, zlib
PIX = [(255,0,0),(0,255,0),(0,0,255),(255,0,0),
       (255,0,0),(0,0,0),(0,0,255),(0,255,0)]
W, H = 4, 2
raw = bytearray()
for y in range(H):
    raw.append(0)
    for x in range(W):
        raw += bytes(PIX[y * W + x])
def chunk(t, d):
    return struct.pack(">I", len(d)) + t + d + struct.pack(">I", zlib.crc32(t + d) & 0xFFFFFFFF)
out  = b"\x89PNG\r\n\x1a\n"
out += chunk(b"IHDR", struct.pack(">IIBBBBB", W, H, 8, 2, 0, 0, 0))
out += chunk(b"IDAT", zlib.compress(bytes(raw), 9))
out += chunk(b"IEND", b"")
open("scripts/fixtures/png_pixel_probe.png", "wb").write(out)
PY
```

Verify it — both of the first two lines are independent of any measurement code
in the suite:

```sh
od    -An -tx1 -v scripts/fixtures/png_pixel_probe.png
file  scripts/fixtures/png_pixel_probe.png   # -> PNG image data, 4 x 2, 8-bit/color RGB
shasum -a 256 scripts/fixtures/png_pixel_probe.png
# dd0a37aae1ad59c02d311f999dfa64ab5fc20eb4d043d623492cf48ddfd984e1
```
