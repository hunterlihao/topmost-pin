# -*- coding: utf-8 -*-
"""窗口置顶助手（TopMostPin）宣传片 —— 1920x1080 / 30fps / 24.8s。

素材来源
--------
界面素材**全部是真实运行中的应用窗口**，由 `scripts/shoot-window.ps1` 抓取：
SetForegroundWindow → GetWindowRect → Graphics.CopyFromScreen（150% 缩放下
物理像素 1432x1076）。PrintWindow 对 WebView2 实测返回纯黑（mean=0.0），
所以这条路线从一开始就被排除了，脚本里也不留这个分支。

窗口截图存成 `docs/promo/main@2x.png`（1410x1020，已裁掉 DWM 的粉色标题栏
和不可见的缩放边框，只留应用自己的报头以下）。片中的全景卡片与两处细节卡片
都是它的裁切与缩放 —— 没有一处 UI 是画出来的。

截图时这台机器上的**当前热键是 Ctrl+Shift+P**：README 里写了 `Ctrl+Alt+P`
在本机被别的程序占用，程序会退到候选组合。片中因此不用键帽特写去「演示默认值」，
热键这条事实只在事实页用文字写（默认 Ctrl+Alt+P，可改），不会和画面打架。

设计决定（沿用同系列，但这个产品有它自己的三处改动）
--------------------------------------------------
1. 视觉语言与 ClipHist 那支同源：石墨黑底 / 纸白卡片 / 银灰次级字；中文走
   微软雅黑（界面就是它），ASCII 走 Consolas（界面的进程名、类名、HWND 三列
   本来就是等宽），拉丁标题走 Bahnschrift。所有文案左对齐在 x=168 的固定基线，
   不做居中 —— 居中是最省事的做法，也最没有个性。
2. **圆角只用 2px**。这个产品的设计说明写得很清楚：对象 2px 圆角，结构 0 圆角。
   所以卡片是硬边的纸，不是 ClipHist 那种 22px 的软卡片。
3. **签名换了**：本片唯一一个记忆点是「图钉落下 → 卡片阴影加深并上浮」，
   讲 z 序（置顶 = 换一层）。ClipHist 的扫描条那套「从上往下印出来」的手法
   一个都没用，场景之间只做 0.12s 快速交叉溶解。图钉本身用的是
   src/components/PinGlyph.vue 里那条 SVG 路径（落定时由描边变实心，正是界面里
   「未置顶 / 已置顶」的第二编码）。
4. 全片单色，唯一的彩色是片尾那枚朱红印章，且只出现一次。

事实边界
--------
文案只写 README / 代码里能查到的：任意窗口一键置顶；两种方式（指向窗口按热键 /
列表点图钉）；热键可改、冲突时明确提示且不会丢原组合；便携单目录、数据在 data\\；
单 exe、除系统 WebView2 外零运行时依赖；Windows 10/11 x64。不写代码签名
（没有），不写内存占用（没测过）。

用法
----
    python scripts/make-promo.py                # 出整片
    python scripts/make-promo.py --stills 4.2,13 # 只出几个时间点的静帧，便于验画面
    python scripts/make-promo.py --sheet        # 从成片抽 8 帧拼出分镜总览.png
"""
import math
import os
import re
import subprocess
import sys

import numpy as np
from PIL import Image, ImageChops, ImageDraw, ImageFilter, ImageFont

W, H, FPS = 1920, 1080, 30
SCENES_T = []          # 在文件末尾填充

HERE = os.path.dirname(os.path.abspath(__file__))
ROOT = os.path.abspath(os.path.join(HERE, ".."))
PROMO = os.path.join(ROOT, "docs", "promo")
MAIN = os.path.join(PROMO, "main@2x.png")
OUT = os.path.join(PROMO, "TopMostPin-promo.mp4")
SHEET = os.path.join(PROMO, "分镜总览.png")

INK = (14, 15, 16)
INK2 = (22, 23, 25)
PAPER = (245, 244, 240)
SILVER = (168, 173, 181)
GRAPHITE = (58, 61, 66)
STAMP = (196, 46, 38)

F_CJK = r"C:\Windows\Fonts\msyh.ttc"
F_CJK_BD = r"C:\Windows\Fonts\msyhbd.ttc"
F_MONO = r"C:\Windows\Fonts\consola.ttf"
F_MONO_BD = r"C:\Windows\Fonts\consolab.ttf"
F_LATIN = r"C:\Windows\Fonts\bahnschrift.ttf"

_fc = {}


def _load(path, size, variation=None):
    f = ImageFont.truetype(path, size)
    if variation:
        try:
            f.set_variation_by_name(variation)
        except Exception:
            pass
    return f


def cjk(size, bold=True):
    k = ("cjk", size, bold)
    if k not in _fc:
        _fc[k] = _load(F_CJK_BD if bold else F_CJK, size)
    return _fc[k]


def mono(size, bold=False):
    k = ("mono", size, bold)
    if k not in _fc:
        _fc[k] = _load(F_MONO_BD if bold else F_MONO, size)
    return _fc[k]


def latin(size, variation="SemiBold Condensed"):
    k = ("lat", size, variation)
    if k not in _fc:
        try:
            _fc[k] = _load(F_LATIN, size, variation)
        except Exception:
            _fc[k] = _load(F_MONO_BD, size)
    return _fc[k]


def ease(p):
    p = max(0.0, min(1.0, p))
    return p * p * (3 - 2 * p)


def ease_out(p):
    p = max(0.0, min(1.0, p))
    return 1 - (1 - p) ** 3


# ---------------------------------------------------------------- 背景与纹理
def build_bg():
    img = Image.new("RGB", (W, H), INK)
    d = ImageDraw.Draw(img)
    # 极细网格：密度做得像坐标纸。界面里全是发丝线，这条纹理是它的放大回声。
    for x in range(0, W, 160):
        d.line([(x, 0), (x, H)], fill=(25, 26, 28))
    for y in range(0, H, 160):
        d.line([(0, y), (W, y)], fill=(23, 24, 26))
    vig = Image.new("L", (W, H), 0)
    ImageDraw.Draw(vig).ellipse([-W // 4, -H // 3, W + W // 4, H + H // 3], fill=46)
    vig = vig.filter(ImageFilter.GaussianBlur(220))
    dark = Image.new("RGB", (W, H), (0, 0, 0))
    img = Image.composite(img, dark, vig.point(lambda v: 255 - v // 3))

    # 卡片后面那一片很淡的光。不是装饰：石墨底本来就接近纯黑，落影压不出
    # 差别（实测只差 8 级），光池给阴影一块能落下去的地面，抬起来才看得见。
    pool = Image.new("L", (W, H), 0)
    ImageDraw.Draw(pool).ellipse([1236 - 900, 600 - 600, 1236 + 900, 600 + 600], fill=255)
    pool = pool.filter(ImageFilter.GaussianBlur(260)).point(lambda v: int(v * 20 / 255))
    img = ImageChops.add(img, Image.merge("RGB", (pool, pool, pool)))
    return img.convert("RGBA")


BG = build_bg()

_rng = np.random.default_rng(11)
_noise = (_rng.normal(0, 4, (H, W, 1))).repeat(3, axis=2)
GRAIN = Image.fromarray(np.clip(128 + _noise, 0, 255).astype("uint8"), "RGB")


def grain(img, amount=1.0):
    """加性粒度：img + (grain-128)。

    不能用 Image.blend(img, gray_noise, k) —— 那是向 128 混合，会把黑场整体
    抬到 ~24，卡片落影再深也压不下去（实测阴影只比背景暗 8 级）。加法保留
    黑场，阴影才真的能「变深」。
    """
    if amount == 1.0:
        return ImageChops.add(img, GRAIN, scale=1.0, offset=-128)
    sh = GRAIN.point(lambda v: 128 + int((v - 128) * amount))
    return ImageChops.add(img, sh, scale=1.0, offset=-128)


# ---------------------------------------------------------------- 图钉字形
# 直接搬 src/components/PinGlyph.vue 里的 SVG 路径，按 24 单位坐标系解析成折线。
# 用它而不是自己画一个「图钉」：落定时描边变实心，和界面里未置顶/已置顶的
# 第二编码是同一件事。
PIN_PATH = ("M16 9V4h1c.55 0 1-.45 1-1s-.45-1-1-1H7c-.55 0-1 .45-1 1s.45 1 1 1"
            "h1v5c0 1.66-1.34 3-3 3v2h5.97v7l1 1 1-1v-7H19v-2c-1.66 0-3-1.34-3-3z")


def parse_path(d):
    toks = re.findall(r"[MmLlHhVvCcSsZz]|-?\d*\.?\d+", d)
    polys, cur = [], []
    x = y = sx = sy = 0.0
    i, cmd = 0, None
    while i < len(toks):
        t = toks[i]
        if t.isalpha():
            cmd = t
            i += 1
            if cmd in "Zz":
                if cur:
                    polys.append(cur)
                    cur = []
                x, y = sx, sy
                continue
        rel = cmd.islower()
        c = cmd.upper()

        def num(k=1):
            nonlocal i
            v = [float(toks[i + j]) for j in range(k)]
            i += k
            return v

        if c == "M":
            if cur:
                polys.append(cur)
            a, b = num(2)
            x, y = (x + a, y + b) if rel else (a, b)
            sx, sy = x, y
            cur = [(x, y)]
        elif c == "L":
            a, b = num(2)
            x, y = (x + a, y + b) if rel else (a, b)
            cur.append((x, y))
        elif c == "H":
            (a,) = num(1)
            x = x + a if rel else a
            cur.append((x, y))
        elif c == "V":
            (a,) = num(1)
            y = y + a if rel else a
            cur.append((x, y))
        elif c in ("C", "S"):
            if c == "C":
                a, b, cc, dd, e, f = num(6)
                x1, y1 = (x + a, y + b) if rel else (a, b)
                x2, y2 = (x + cc, y + dd) if rel else (cc, dd)
                x3, y3 = (x + e, y + f) if rel else (e, f)
            else:
                a, b, e, f = num(4)
                x2, y2 = (x + a, y + b) if rel else (a, b)
                x3, y3 = (x + e, y + f) if rel else (e, f)
                x1, y1 = x, y
            for s in range(1, 21):
                u = s / 20
                m = 1 - u
                cur.append((m ** 3 * x + 3 * m * m * u * x1 + 3 * m * u * u * x2 + u ** 3 * x3,
                            m ** 3 * y + 3 * m * m * u * y1 + 3 * m * u * u * y2 + u ** 3 * y3))
            x, y = x3, y3
        else:
            raise ValueError(cmd)
    if cur:
        polys.append(cur)
    return polys


_PIN_POLY = parse_path(PIN_PATH)[0]
_PIN_BOX = (min(p[0] for p in _PIN_POLY), min(p[1] for p in _PIN_POLY),
            max(p[0] for p in _PIN_POLY), max(p[1] for p in _PIN_POLY))
_pin_cache = {}


def pin(hpx, filled=False, color=PAPER, sw_units=1.7):
    """按「图钉高度 = hpx」渲染，4 倍超采样后缩回来换取抗锯齿。

    sw_units 是 24 单位坐标系里的描边宽度：界面里用 1.7，开场那枚 430px 的
    残影要用 0.45 —— 同一个 1.7 放大 24 倍会粗得像个实心块。
    """
    key = (hpx, filled, color, sw_units)
    if key in _pin_cache:
        return _pin_cache[key]
    S = 4
    bx0, by0, bx1, by1 = _PIN_BOX
    unit = hpx * S / (by1 - by0)
    pad = int(round(2.4 * S * unit / 24))
    cw = int(round((bx1 - bx0) * unit)) + pad * 2
    ch = int(round((by1 - by0) * unit)) + pad * 2
    big = Image.new("RGBA", (cw, ch), (0, 0, 0, 0))
    d = ImageDraw.Draw(big)
    pts = [((px - bx0) * unit + pad, (py - by0) * unit + pad) for px, py in _PIN_POLY]
    if filled:
        d.polygon(pts, fill=color)
        d.line(pts + [pts[0]], fill=color, width=int(round(sw_units * unit)), joint="curve")
    else:
        d.line(pts + [pts[0]], fill=color, width=max(2, int(round(sw_units * unit))), joint="curve")
    out = big.resize((cw // S, ch // S), Image.LANCZOS)
    _pin_cache[key] = out
    return out


# ---------------------------------------------------------------- 卡片
_cards = {}


def card(name, scale, radius=2, shadow=42, pad=210):
    """真实截图 → 纸面卡片 + 两层可单独调 alpha 的阴影遮罩。

    - drop：偏移大、模糊重的落影，抬起来时它会往下走、变深、变宽；
    - contact：贴边 8px 的接触影，卡片贴着台面时才明显，抬起时先散掉。

    两张遮罩都只模糊一次，之后每帧只改 alpha 与偏移 —— 这样「阴影变深」是
    连续渐变，而不是换个贴图硬切。石墨底本来就很暗，所以落影必须够大够深才看得见。
    """
    key = (name, scale, radius, shadow, pad)
    if key in _cards:
        return _cards[key]
    src = Image.open(os.path.join(PROMO, name)).convert("RGBA")
    w, h = int(src.width * scale), int(src.height * scale)
    body = src.resize((w, h), Image.LANCZOS)
    mask = Image.new("L", (w, h), 0)
    ImageDraw.Draw(mask).rounded_rectangle([0, 0, w - 1, h - 1], radius=radius, fill=255)
    body.putalpha(mask)

    def _blur(px):
        sh = Image.new("L", (w + pad * 2, h + pad * 2), 0)
        ImageDraw.Draw(sh).rounded_rectangle([pad, pad, w + pad, h + pad],
                                             radius=radius + 2, fill=255)
        return sh.filter(ImageFilter.GaussianBlur(px))

    _cards[key] = {"body": body, "drop": _blur(shadow), "contact": _blur(9),
                   "pad": pad, "w": w, "h": h}
    return _cards[key]


def _shadow_rgba(mask):
    key = ("shadowrgba", id(mask))
    if key not in _cards:
        img = Image.new("RGBA", mask.size, (0, 0, 0, 0))
        img.putalpha(mask)
        _cards[key] = img
    return _cards[key]


def put_card(dst, spr, cx, cy, alpha=255, sh_w=0.55, sh_off=24, ctc=0.5, dx=0, dy=0, zoom=1.0):
    body, pad = spr["body"], spr["pad"]
    if zoom != 1.0:
        nw, nh = int(body.width * zoom), int(body.height * zoom)
        body = body.resize((nw, nh), Image.LANCZOS)
    w, h = body.width, body.height
    x = int(cx - w / 2 + dx)
    y = int(cy - h / 2 + dy)
    sh_off = int(round(sh_off))

    def _composite(mask, weight, off):
        if weight <= 0.003:
            return
        m = mask
        if zoom != 1.0:
            m = m.resize((int(m.width * zoom), int(m.height * zoom)), Image.LANCZOS)
            pp = int(pad * zoom)
        else:
            pp = pad
        sh = _shadow_rgba(m)
        if weight < 0.999:
            sh = sh.copy()
            sh.putalpha(sh.getchannel("A").point(lambda v: int(v * weight)))
        dst.alpha_composite(sh, (x - pp, y - pp + off))

    _composite(spr["drop"], sh_w, sh_off)
    _composite(spr["contact"], ctc, 6)
    if alpha < 255:
        body = body.copy()
        body.putalpha(body.getchannel("A").point(lambda v: v * alpha // 255))
    dst.alpha_composite(body, (x, y))
    return (x, y, w, h)


# ---------------------------------------------------------------- 文本
CAP_X = 168           # 文案左基线，全片统一
TEXT_COL_R = 740      # 文案列右界：溶解时这一列硬切，见 compose()
RAIL_X = 76
COL_R = 740           # 左栏文字区的右界（再往右就是卡片）
EYEBROW_Y = 300
HEAD_Y = 396
SUB_Y = 472
NOTE_Y = 530

_warn = []


def _is_cjk(ch):
    o = ord(ch)
    return o > 0x2E80 or ch in "①②③④⑤⑥⑦⑧⑨"


def mixed(d, xy, s, size_cjk, size_mono, color=PAPER, anchor="ls", gap=0):
    """中文用雅黑、ASCII 用等宽，逐段绘制并累加宽度。

    整行交给 Consolas 会出豆腐块（它没有 CJK 字形），整行换雅黑又丢掉
    「机器给的东西走等宽」这条设计意图，所以按字符类别切段再拼。
    """
    x, y = xy
    runs = []
    for ch in s:
        kind = "cjk" if _is_cjk(ch) else "mono"
        if runs and runs[-1][0] == kind:
            runs[-1][1] += ch
        else:
            runs.append([kind, ch])
    widths = []
    for kind, text in runs:
        f = cjk(size_cjk, True) if kind == "cjk" else mono(size_mono, True)
        widths.append(d.textlength(text, font=f))
    total = sum(widths) + gap * (len(runs) - 1)
    if anchor == "ms":
        x -= total / 2
    for (kind, text), w in zip(runs, widths):
        f = cjk(size_cjk, True) if kind == "cjk" else mono(size_mono, True)
        d.text((x, y), text, font=f, fill=color, anchor="ls")
        x += w + gap
    return total


def _tracked(d, xy, s, size_cjk, size_mono, color=PAPER, gap=6, track=3):
    """逐字加字距。区域标题要像机器铭牌，逐字间距是这件事唯一的手法。"""
    x, y = xy
    total = 0.0
    for ch in s:
        f = cjk(size_cjk, True) if _is_cjk(ch) else mono(size_mono, True)
        d.text((x, y), ch, font=f, fill=color, anchor="ls")
        adv = d.textlength(ch, font=f) + track
        x += adv
        total += adv
    return max(0.0, total - track)


def eyebrow(d, s, color=SILVER, size=24, y=EYEBROW_Y):
    # 字距拉开，像机器铭牌
    w = _tracked(d, (CAP_X, y), s, size + 2, size, color, gap=6)
    _check(s, CAP_X + w, COL_R)
    return w


def head(d, s, size=70, color=PAPER, y=HEAD_Y):
    d.text((CAP_X, y), s, font=cjk(size), fill=color, anchor="ls")
    _check(s, CAP_X + d.textlength(s, font=cjk(size)), COL_R)


def sub(d, s, color=SILVER, size=29, y=SUB_Y):
    w = mixed(d, (CAP_X, y), s, size - 1, size, color)
    _check(s, CAP_X + w, COL_R)
    return w


def note(d, s, color=GRAPHITE, size=25, y=NOTE_Y):
    w = mixed(d, (CAP_X, y), s, size - 1, size, color)
    _check(s, CAP_X + w, COL_R)
    return w


def _check(s, w, limit):
    if w > limit:
        _warn.append(f"溢出 {int(w)}>{limit}: {s}")


def rule(d, y, x0=CAP_X, x1=W - CAP_X, color=(255, 255, 255, 26), w=1):
    d.line([(x0, y), (x1, y)], fill=color, width=w)


# ---------------------------------------------------------------- 场景家具
def rail(img, idx, total):
    d = ImageDraw.Draw(img)
    d.line([(RAIL_X, 210), (RAIL_X, 840)], fill=(52, 55, 60), width=1)
    for i in range(total):
        y = 300 + i * 78
        active = i == idx
        col = PAPER if active else (60, 63, 68)
        d.line([(RAIL_X - 10, y), (RAIL_X + 10, y)], fill=col, width=2 if active else 1)
        if active:
            d.text((RAIL_X + 24, y), f"{i:02d}", font=mono(21, True), fill=PAPER, anchor="lm")


def footer(img, t, dur):
    d = ImageDraw.Draw(img)
    mixed(d, (CAP_X, H - 96), "TOPMOSTPIN / TAURI v2 + RUST / 便携单目录", 18, 20, (74, 78, 84))
    d.line([(CAP_X, H - 74), (W - CAP_X, H - 74)], fill=(44, 46, 50), width=2)
    d.line([(CAP_X, H - 74), (CAP_X + int((W - CAP_X * 2) * (t / dur)), H - 74)],
           fill=PAPER, width=2)


# ---------------------------------------------------------------- 场景
layer = None


def s_open(t, dur):
    """冷开场：只排字 + 一枚 6% 的图钉残影在右侧。

    那枚残影是刻意的：它在 S4 会真的落下来砸在卡片上 —— 开场是「概念」，
    中段是「动作」。除此之外右半张画面留空，留着让标题呼吸。
    """
    d = ImageDraw.Draw(layer)
    q0 = ease(t / 0.7)
    if q0 > 0:
        ghost = pin(430, False, (44, 47, 52), sw_units=0.34)
        ghost.putalpha(ghost.getchannel("A").point(lambda v: int(v * q0)))
        layer.alpha_composite(ghost, (1500 - ghost.width // 2, 300))
    eyebrow(d, "TOP MOST PIN", y=344)
    for i, line in enumerate(["把任意窗口", "钉在最上层。"]):
        q = ease((t - 0.10 - i * 0.15) / 0.55)
        if q <= 0:
            continue
        y = 470 + i * 112 + int((1 - q) * 12)
        d.text((CAP_X, y), line, font=cjk(94), fill=tuple(int(PAPER[c] * q) for c in range(3)),
               anchor="ls")
    if t > 1.05:
        q = ease((t - 1.05) / 0.5)
        d.line([(CAP_X, 646), (CAP_X + int(82 * q), 646)], fill=PAPER, width=2)
    if t > 1.25:
        q = ease((t - 1.25) / 0.55)
        mixed(d, (CAP_X, 706), "Windows 10 / 11 x64   ·   便携单目录   ·   双击即用", 26, 26,
              tuple(int(SILVER[c] * q) for c in range(3)))


def s_ui(t, dur):
    """全景：真实窗口整张进来。这里是全片唯一一次把 1410x1020 全放出来。
    没有扫描条 —— 只淡入 + 轻微上浮，因为「印出来」是另一支片子的签名。"""
    d = ImageDraw.Draw(layer)
    eyebrow(d, "01 / 界面")
    head(d, "一屏扫完所有窗口", 66)
    if t > 0.5:
        sub(d, "标题 · 进程 · 类名 · 句柄，四列对齐")
    if t > 1.6:
        q = ease((t - 1.6) / 0.6)
        note(d, "列对齐了，才能竖着扫下来", (int(96 * q), int(100 * q), int(107 * q)))
    p = ease_out((t - 0.10) / 0.9)
    if p > 0:
        put_card(layer, card("main@2x.png", 0.66), 1246, 582,
                 alpha=int(255 * min(1, p * 1.2)), sh_w=0.62, sh_off=28, ctc=0.6,
                 dy=int((1 - p) * 34))


def s_deck(t, dur):
    """细节一：空台面。界面 99% 的时间停在这一屏，那三行字就是这个工具的全部说明。
    卡片是 100% 的真实像素（裁 760x138，放大到 1.30），不重排、不改字。"""
    d = ImageDraw.Draw(layer)
    eyebrow(d, "02 / 怎么开始")
    head(d, "空位里写着怎么用", 66)
    if t > 0.6:
        sub(d, "指向窗口按键，或点窗口左边的图钉")
    p = ease_out((t - 0.15) / 0.85)
    if p > 0:
        put_card(layer, card("deck@2x.png", 1.30), 1236, 620,
                 alpha=int(255 * min(1, p * 1.2)), sh_w=0.55, sh_off=26, ctc=0.55,
                 dy=int((1 - p) * 30))
    if t > 1.5:
        q = ease((t - 1.5) / 0.6)
        note(d, "45° 斜线 = 预留给一个还没放下的窗口", (int(92 * q), int(96 * q), int(102 * q)),
             y=704)


def s_pin(t, dur):
    """全片唯一的签名：图钉落下 → 卡片阴影变深、整张抬起来。

    为什么这样讲：置顶讲的不是「列表里多一行」，是 z 序。在这个几乎全黑的环境里，
    z 轴唯一看得见的东西就是「卡和台面之间那一层阴影」—— 所以落定之后，
    阴影往下走、变深，卡片抬起 40px 并轻微推向镜头（缩放 1.024）。
    落定那一刻图钉由描边变实心，正好是界面里「已置顶」的那一档。
    除此之外不再加第二个花招：没有扫描条，没有粒子，没有第二枚印章。
    """
    d = ImageDraw.Draw(layer)
    eyebrow(d, "03 / 置顶")
    head(d, "钉住，它就升一层", 70)
    if t > 1.45:
        sub(d, "按一次置顶，再按一次取消")
    if t > 1.95:
        q = ease((t - 1.95) / 0.5)
        mixed(d, (CAP_X, NOTE_Y), "默认 Ctrl+Alt+P", 25, 27,
              (int(96 * q), int(100 * q), int(107 * q)))

    impact = 0.80
    cx, base_y = 1246, 566
    p_in = ease_out((t - 0.05) / 0.55)
    if t < impact:
        dy = int((1 - p_in) * 34)
        sh_w, sh_off, ctc, zoom = 0.30, 12, 0.95, 1.0
    else:
        k = t - impact
        # 落地回弹：先被砸下去 7px，再抬起 44px
        dip = -7 * math.sin(min(1.0, k / 0.17) * math.pi) * math.exp(-k * 5)
        lift = 44 * ease_out((k - 0.05) / 0.55)
        z = ease_out((k - 0.05) / 0.55)
        dy = int(dip - lift)
        sh_w = 0.30 + 0.70 * z
        sh_off = 12 + 62 * z
        ctc = 0.95 * (1 - 0.89 * z)
        zoom = 1.0 + 0.026 * z

    put_card(layer, card("main@2x.png", 0.62), cx, base_y,
             alpha=int(255 * min(1, p_in * 1.3)), sh_w=sh_w, sh_off=sh_off,
             ctc=ctc, dy=dy, zoom=zoom)

    # 图钉：0.30s 在画面上方淡入、停一拍，0.47s 落下，0.80s 砸在卡片上沿。
    # 两处都是被实测修正过的：起点必须落在画面内（tip=118），否则三次缓入会让
    # 它整段都在画外，最后两帧凭空跳出来；位移曲线必须是 t²（真实自由落体），
    # 三次缓入的加速度太陡，同样会变成「悬停→瞬移」。
    pin_h = 78
    spr_out = pin(pin_h, False, PAPER)
    spr_fill = pin(pin_h, True, PAPER)
    pin_pad = int(round(2.4 * (pin_h / 18.0)))     # pin() 四周留的空白
    card_top = base_y - 316 + dy - int((zoom - 1) * 316)   # 632 = 1020 * 0.62
    tip_end = card_top + 12
    tip_start = 40 + pin_h
    fall0 = 0.47
    if t >= 0.30:
        qa = ease((t - 0.30) / 0.22)
        if t < fall0:
            tip = tip_start
            spr, a = spr_out, qa
        elif t < impact:
            k = (t - fall0) / (impact - fall0)
            tip = tip_start + (tip_end - tip_start) * k * k
            spr, a = spr_out, 255
        else:
            k2 = t - impact
            dmp = math.exp(-k2 * 16) if k2 < 0.3 else 0.0
            spr = spr_fill.resize((int(spr_fill.width * (1.0 + 0.09 * dmp)),
                                   int(spr_fill.height * (1.0 - 0.11 * dmp))), Image.LANCZOS)
            tip, a = tip_end, 255
        if a < 255:
            spr = spr.copy()
            spr.putalpha(spr.getchannel("A").point(lambda v: int(v * a)))
        layer.alpha_composite(spr, (int(cx - spr.width // 2),
                                    int(tip - spr.height + pin_pad)))


def s_rows(t, dur):
    """细节二：两种方式里的第二种。真实两行 + 图钉按钮上的一圈标注。
    标注是唯一的「画上去的东西」，它只是框住真实控件，没有伪造控件。"""
    d = ImageDraw.Draw(layer)
    eyebrow(d, "04 / 另一种方式")
    head(d, "点窗口左边的图钉", 66)
    if t > 0.55:
        sub(d, "列表里点它，和按热键是一回事")
    p = ease_out((t - 0.12) / 0.8)
    if p <= 0:
        return
    cx, cy = 1236, 618
    x, y, w, h = put_card(layer, card("rows@2x.png", 1.5), cx, cy,
                          alpha=int(255 * min(1, p * 1.2)), sh_w=0.55, sh_off=26,
                          ctc=0.55, dy=int((1 - p) * 30))
    # 图钉按钮中心（源图 640x144 里约 (48, 39)，放大 1.5）
    pcx, pcy = x + int(48 * 1.5), y + int(38 * 1.5)
    q = ease((t - 0.75) / 0.5)
    if q > 0:
        a = int(190 * q)
        r = 42
        ring = Image.new("RGBA", (r * 2 + 8, r * 2 + 8), (0, 0, 0, 0))
        rd = ImageDraw.Draw(ring)
        rd.rounded_rectangle([4, 4, 4 + r * 2, 4 + r * 2], radius=3,
                             outline=(PAPER[0], PAPER[1], PAPER[2], a), width=2)
        layer.alpha_composite(ring, (pcx - r - 4, pcy - r - 4))


def s_facts(t, dur):
    """事实页：不配卡片。四条，全部能在 README 里查到出处。"""
    d = ImageDraw.Draw(layer)
    eyebrow(d, "05 / 事实")
    rows = [
        ("热键", "默认 Ctrl+Alt+P，可改"),
        ("冲突", "明确提示，且不丢原组合"),
        ("数据", "data\\data.db，删掉目录即卸载"),
        ("依赖", "单 exe，除系统 WebView2 外零运行时依赖"),
    ]
    for i, (label, value) in enumerate(rows):
        q = ease((t - 0.25 - i * 0.34) / 0.55)
        if q <= 0:
            continue
        y = 384 + i * 92
        mixed(d, (CAP_X, y), label, 27, 27, tuple(int(SILVER[c] * q) for c in range(3)))
        w = mixed(d, (440, y), value, 33, 34, tuple(int(PAPER[c] * q) for c in range(3)))
        _check(value, 440 + w, W - CAP_X)
        d.line([(410, y + 20), (W - CAP_X, y + 20)],
               fill=(255, 255, 255, int(14 * q)), width=1)
    if t > 1.85:
        q = ease((t - 1.85) / 0.6)
        mixed(d, (CAP_X, 800), "Windows 10 / 11 x64   ·   MIT License   ·   便携单目录", 26, 27,
              tuple(int(SILVER[c] * q) for c in range(3)))


def s_end(t, dur):
    d = ImageDraw.Draw(layer)
    q = ease(t / 0.6)
    icon = Image.open(os.path.join(ROOT, "src-tauri", "icons", "icon.png")).convert("RGBA")
    icon = icon.resize((92, 92), Image.LANCZOS)
    if q < 1:
        icon.putalpha(icon.getchannel("A").point(lambda v: int(v * q)))
    # 图标自己是黑底，落在石墨黑上会糊成一团，用一根 14% 白描边把它托出来
    d.rounded_rectangle([CAP_X, 352, CAP_X + 91, 443], radius=21,
                        outline=(255, 255, 255, int(36 * q)), width=1)
    layer.alpha_composite(icon, (CAP_X, 352))
    d.text((284, 424), "窗口置顶助手", font=cjk(64), fill=PAPER, anchor="ls")
    if t > 0.25:
        q2 = ease((t - 0.25) / 0.5)
        d.text((286, 470), "TopMostPin", font=latin(34), fill=SILVER, anchor="ls")
    if t > 0.45:
        q3 = ease((t - 0.45) / 0.55)
        d.text((CAP_X, 604), "github.com/hunterlihao/topmost-pin", font=mono(33),
               fill=tuple(int(PAPER[c] * q3) for c in range(3)), anchor="ls")
        mixed(d, (CAP_X, 662), "MIT License   ·   Windows 10 / 11 x64   ·   便携单目录", 24, 25,
              tuple(int(SILVER[c] * q3) for c in range(3)))
    if t > 0.8:
        # 朱红印章：全片唯一一处彩色，只出现一次。压到 186px，别抢标题
        q4 = ease_out((t - 0.8) / 0.42)
        ss = 1.0 + 0.12 * (1 - q4)
        box = Image.new("RGBA", (186, 186), (0, 0, 0, 0))
        bd = ImageDraw.Draw(box)
        a = int(228 * q4)
        bd.rectangle([7, 7, 178, 178], outline=(STAMP[0], STAMP[1], STAMP[2], a), width=6)
        bd.text((92, 66), "置", font=cjk(52), fill=(STAMP[0], STAMP[1], STAMP[2], a), anchor="mm")
        bd.text((92, 128), "顶", font=cjk(52), fill=(STAMP[0], STAMP[1], STAMP[2], a), anchor="mm")
        box = box.rotate(-7, expand=True, resample=Image.BICUBIC)
        box = box.resize((int(box.width * ss), int(box.height * ss)), Image.LANCZOS)
        layer.alpha_composite(box, (1418, 470))


SCENES = [
    (0.0, 2.8, s_open, "冷开场"),
    (2.8, 7.2, s_ui, "界面"),
    (7.2, 11.2, s_deck, "空台面"),
    (11.2, 15.6, s_pin, "签名"),
    (15.6, 18.8, s_rows, "图钉"),
    (18.8, 22.6, s_facts, "事实"),
    (22.6, 24.8, s_end, "片尾"),
]
DUR = SCENES[-1][1]
NF = int(round(FPS * DUR))
FADE = 0.12


def _scene_rgb(idx, lt, dur):
    """单独把一个场景渲染成整帧（底 + 该场景的图层）。

    场景内部时间是 lt + FADE：新场景在自己的切点之前 0.36s 就开始动，
    这样溶解的时候两张卡片同时在动，而不是「旧卡已经淡没了、新卡还没进来」——
    后者会在每个切点留下一段几乎空白的画面（实测均值掉到 28）。
    """
    global layer
    layer = Image.new("RGBA", (W, H), (0, 0, 0, 0))
    SCENES[idx][2](lt + FADE, dur)
    out = BG.copy()
    out.alpha_composite(layer)
    return out.convert("RGB")


def compose(t):
    """场景交叉溶解。

    之前是「每个场景只画自己 [s0,s1) 内，并在首尾各自淡入淡出」——
    于是每个切点前后各 0.36s 全黑，实测 7.0~7.3s 之间画面均值从 81 掉到 25，
    也就是片子中间有 6 个 0.7 秒的黑洞。现在改成：新场景提前 FADE 秒入场，
    两帧整帧混合（Image.blend），溶解期间不掉亮度。

    但整帧溶解会把两行标题叠在一起（实测 7.1s：新标题正常，旧标题在下面
    映成一团脏影）。所以溶解期间**文案列硬切**：左栏 x<740 直接取当前场景那一帧。
    740 是按卡片最左沿挑的（三张卡分别在 742 / 756 / 809），压不到任何一张。
    """
    active = []
    for idx, (s0, s1, fn, _tag) in enumerate(SCENES):
        dur = s1 - s0
        lt = t - s0
        if lt < -FADE or lt >= dur:
            continue
        active.append((idx, lt, dur))

    if not active:
        base = BG.copy().convert("RGB")
    elif len(active) == 1:
        idx, lt, dur = active[0]
        base = _scene_rgb(idx, lt, dur)
    else:
        a_i, a_lt, a_dur = active[0]
        b_i, b_lt, b_dur = active[1]
        w = ease(min(1.0, max(0.0, (b_lt + FADE) / FADE)))
        fa = _scene_rgb(a_i, a_lt, a_dur)
        fb = _scene_rgb(b_i, b_lt, b_dur)
        base = Image.blend(fa, fb, w)
        col = fb if b_lt >= 0 else fa
        base.paste(col.crop((0, 200, TEXT_COL_R, 940)), (0, 200))

    idx_now = active[-1][0] if active and active[-1][1] >= 0 else (active[0][0] if active else 0)
    img = base.convert("RGBA")
    if idx_now > 0:
        rail(img, idx_now, len(SCENES))
        footer(img, t, DUR)
    out = grain(img.convert("RGB"), 1.0).convert("RGB")
    if t < 0.45:                      # 片头从黑里出来
        out = Image.blend(Image.new("RGB", (W, H), (0, 0, 0)), out, t / 0.45)
    return out


def render():
    cmd = ["ffmpeg", "-y", "-f", "rawvideo", "-pix_fmt", "rgb24", "-s", f"{W}x{H}",
           "-r", str(FPS), "-i", "-", "-an", "-c:v", "libx264", "-preset", "slow",
           "-crf", "19", "-pix_fmt", "yuv420p", "-movflags", "+faststart", OUT]
    proc = subprocess.Popen(cmd, stdin=subprocess.PIPE, stdout=subprocess.DEVNULL,
                            stderr=subprocess.DEVNULL)
    for fi in range(NF):
        proc.stdin.write(compose(fi / FPS).tobytes())
        if fi % 90 == 0:
            print(f"  frame {fi}/{NF}", flush=True)
    proc.stdin.close()
    proc.wait()
    print("完成:", OUT, os.path.getsize(OUT) // 1024, "KB")
    if _warn:
        print("!! 文案溢出：")
        for w in sorted(set(_warn)):
            print("  ", w)


def stills(times):
    os.makedirs(os.path.join(PROMO, "_stills"), exist_ok=True)
    for tt in times:
        p = os.path.join(PROMO, "_stills", f"t{tt:05.2f}.png")
        compose(float(tt)).save(p)
        print("still", p)
    if _warn:
        print("!! 文案溢出：")
        for w in sorted(set(_warn)):
            print("  ", w)


def sheet():
    """从成片抽 8 帧拼图 —— 用编码后的帧，不是渲染中间件，避免「预览好看成片翻车」。"""
    beats = [
        (1.90, "冷开场"),
        (5.00, "界面"),
        (9.20, "空台面"),
        (11.95, "图钉落下"),
        (13.60, "签名"),
        (17.20, "图钉"),
        (20.70, "事实"),
        (24.20, "片尾"),
    ]
    rows, cols = 2, 4
    cw, ch = 460, 259
    gap, margin, lab = 12, 20, 30
    sw = margin * 2 + cw * cols + gap * (cols - 1)
    sh = margin * 2 + (ch + lab) * rows + gap * (rows - 1)
    out = Image.new("RGB", (sw, sh), (10, 10, 11))
    d = ImageDraw.Draw(out)
    tmp = os.path.join(PROMO, "_stills")
    os.makedirs(tmp, exist_ok=True)
    for i, (tt, tag) in enumerate(beats):
        p = os.path.join(tmp, f"sheet{i}.png")
        subprocess.run(["ffmpeg", "-y", "-ss", f"{tt:.3f}", "-i", OUT, "-frames:v", "1",
                        "-vf", f"scale={cw}:{ch}:flags=lanczos", p],
                       stdout=subprocess.DEVNULL, stderr=subprocess.DEVNULL, check=True)
        im = Image.open(p).convert("RGB")
        cx = margin + (i % cols) * (cw + gap)
        cy = margin + (i // cols) * (ch + lab + gap)
        out.paste(im, (cx, cy))
        mixed(d, (cx, cy + ch + 22), f"{i + 1:02d}  {tag}  {tt:.1f}s", 17, 16, SILVER)
    out.save(SHEET)
    print("分镜总览:", SHEET, out.size)


if __name__ == "__main__":
    args = sys.argv[1:]
    if args and args[0] == "--stills":
        stills([float(x) for x in args[1].split(",")])
    elif args and args[0] == "--sheet":
        sheet()
    else:
        render()
