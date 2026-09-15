"""tfs ikonu: basit, SSH + dosya transferi anlatan mark.

Her boyut kendi çözünürlüğünden 8x supersample ile çizilir; tek büyük görüntüyü
küçültmeye göre 16-32 px'te belirgin şekilde daha net.
"""
import io
import sys
from PIL import Image, ImageDraw

BG = (13, 22, 32)          # koyu terminal zemini
EDGE = (34, 211, 238)      # cyan
CYAN = (34, 211, 238)
AMBER = (251, 191, 36)
WHITE = (236, 245, 250)


def body(S, d, img):
    """Yuvarlak kare gövde + ince cyan çizgi."""
    m = round(S * 0.04)
    r = round(S * 0.235)
    d.rounded_rectangle([m, m, S - m - 1, S - m - 1], radius=r, fill=BG + (255,))
    d.rounded_rectangle(
        [m, m, S - m - 1, S - m - 1], radius=r,
        outline=EDGE + (80,), width=max(1, round(S * 0.016)),
    )


def cap_line(d, p0, p1, w, color):
    """Yuvarlak uçlu kalın çizgi (ImageDraw'da round-cap yok)."""
    d.line([p0, p1], fill=color + (255,), width=round(w))
    for (px, py) in (p0, p1):
        d.ellipse([px - w / 2, py - w / 2, px + w / 2, py + w / 2], fill=color + (255,))


def arrow_h(d, S, y, x_tail, x_tip, w, color):
    """Yatay ok: gövde + V uçlu baş."""
    cap_line(d, (x_tail, y), (x_tip, y), w, color)
    hl = S * 0.15                      # baş kanadı uzunluğu
    sgn = 1 if x_tip > x_tail else -1
    cap_line(d, (x_tip, y), (x_tip - sgn * hl, y - hl), w, color)
    cap_line(d, (x_tip, y), (x_tip - sgn * hl, y + hl), w, color)


def arrow_v(d, S, x, y_tail, y_tip, w, color):
    cap_line(d, (x, y_tail), (x, y_tip), w, color)
    hl = S * 0.15
    sgn = 1 if y_tip > y_tail else -1
    cap_line(d, (x, y_tip), (x - hl, y_tip - sgn * hl), w, color)
    cap_line(d, (x, y_tip), (x + hl, y_tip - sgn * hl), w, color)


def v_yatay(S, d):
    """A: karşılıklı iki yatay ok (⇄) — klasik transfer işareti."""
    w = S * 0.095
    arrow_h(d, S, S * 0.375, S * 0.24, S * 0.74, w, CYAN)
    arrow_h(d, S, S * 0.655, S * 0.76, S * 0.26, w, AMBER)


def v_dikey(S, d):
    """B: yukarı/aşağı ok (⇅) — her SFTP istemcisinin yükle/indir işareti."""
    w = S * 0.095
    arrow_v(d, S, S * 0.375, S * 0.74, S * 0.26, w, CYAN)
    arrow_v(d, S, S * 0.655, S * 0.26, S * 0.74, w, AMBER)


def v_panel(S, d):
    """C: iki panel + aradan geçen ok — uygulamanın kendi düzeni."""
    w = S * 0.075
    # sol/sağ panel çerçeveleri
    for x0, x1 in ((0.18, 0.40), (0.60, 0.82)):
        d.rounded_rectangle(
            [S * x0, S * 0.26, S * x1, S * 0.74],
            radius=S * 0.05, outline=WHITE + (120,), width=round(w * 0.8),
        )
    arrow_h(d, S, S * 0.50, S * 0.42, S * 0.60, w, CYAN)


def v_prompt(S, d):
    """D: prompt oku + transfer oku — SSH + transfer birlikte."""
    w = S * 0.10
    # ">" prompt
    cap_line(d, (S * 0.24, S * 0.26), (S * 0.46, S * 0.46), w, CYAN)
    cap_line(d, (S * 0.46, S * 0.46), (S * 0.24, S * 0.66), w, CYAN)
    # altında sağa transfer oku
    arrow_h(d, S, S * 0.72, S * 0.26, S * 0.74, w * 0.9, AMBER)


VARIANTS = {"A": v_yatay, "B": v_dikey, "C": v_panel, "D": v_prompt}


def draw_icon(size, variant, ss=8):
    S = size * ss
    img = Image.new("RGBA", (S, S), (0, 0, 0, 0))
    d = ImageDraw.Draw(img)
    body(S, d, img)
    VARIANTS[variant](S, d)
    return img.resize((size, size), Image.LANCZOS)


def ico_bytes(images):
    """.ico üretir.

    <=48 px girdiler klasik BMP (en geniş uyumluluk), büyükler PNG olarak
    gömülür — 256x256'yı BMP yazmak tek başına 256 KB tutuyordu, PNG'yle
    dosya ~20 KB'a iniyor. PNG girdileri Vista'dan beri destekleniyor.
    """
    import struct
    entries, payloads, offset = [], [], 6 + 16 * len(images)
    for im in images:
        w, h = im.size
        if w > 48:
            buf = io.BytesIO()
            im.save(buf, format="PNG", optimize=True)
            data = buf.getvalue()
        else:
            px = im.load()
            hdr = struct.pack("<IiiHHIIiiII", 40, w, h * 2, 1, 32, 0, 0, 0, 0, 0, 0)
            xor = bytearray()
            for y in range(h - 1, -1, -1):          # BMP alttan üste
                for x in range(w):
                    r_, g_, b_, a_ = px[x, y]
                    xor += bytes((b_, g_, r_, a_))  # BGRA
            # AND maskesi 32bpp'de kullanılmaz ama satırlar 4 bayta hizalı olmalı.
            and_mask = bytes((((w + 31) // 32) * 4) * h)
            data = hdr + bytes(xor) + and_mask
        entries.append(struct.pack("<BBBBHHII", w % 256, h % 256, 0, 0, 1, 32, len(data), offset))
        payloads.append(data)
        offset += len(data)
    return struct.pack("<HHH", 0, 1, len(images)) + b"".join(entries) + b"".join(payloads)


def preview(out):
    cols, pad = list(VARIANTS), 20
    cell = 256
    W = pad + len(cols) * (cell + pad)
    H = pad + cell + pad + 48 * 4 + pad
    im = Image.new("RGBA", (W, H), (44, 44, 52, 255))
    for i, v in enumerate(cols):
        x = pad + i * (cell + pad)
        big = draw_icon(cell, v)
        im.paste(big, (x, pad), big)
        # altında 16/24/32/48 gerçek boyutların 4x büyütülmüşü
        y = pad + cell + pad
        xx = x
        for s in (16, 24, 32, 48):
            sm = draw_icon(s, v).resize((s * 4, s * 4), Image.NEAREST)
            im.paste(sm, (xx, y), sm)
            xx += s * 4 + 8
    im.save(out + "/preview.png")


if __name__ == "__main__":
    out = sys.argv[1]
    if len(sys.argv) > 2:                     # seçilen varyantı yaz
        v = sys.argv[2]
        sizes = [16, 24, 32, 48, 64, 128, 256]
        with open(out + "/tfs.ico", "wb") as f:
            f.write(ico_bytes([draw_icon(s, v) for s in sizes]))
        draw_icon(256, v).save(out + "/tfs.png")
        print("yazildi:", v)
    else:
        preview(out)
        print("onizleme")
