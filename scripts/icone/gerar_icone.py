#!/usr/bin/env python3
"""Skiá — gera o ícone (figura negra sobre terracota, como os outros apps da família): uma mão grega
faz sombra sobre a inscrição de uma coluna jônica e esconde o meio das linhas — o nome sai, a forma fica.
A moldura de meandro é a do Phylax (mac/icon/gerar_icone.py).
Uso: python3 scripts/icone/gerar_icone.py  →  scripts/icone/skia.svg e scripts/icone/icone.png (rsvg-convert)
Depois: npx tauri icon scripts/icone/icone.png
"""
import math
import os
import random
import subprocess

S, M = 1024, 14
BG, INK = "#D85010", "#000000"
AQUI = os.path.dirname(os.path.abspath(__file__))


class RoundRect:
    """Quadrado de cantos redondos parametrizado pelo comprimento, em sentido horário a partir do topo."""
    def __init__(self, half, r, c=S / 2):
        self.h, self.r, self.c = half, r, c
        self.side = 2 * (half - r)
        self.arc = math.pi * r / 2
        self.length = 4 * (self.side + self.arc)

    def at(self, s, offset=0.0):
        s %= self.length
        h, r, c = self.h, self.r, self.c
        for k in range(4):
            if s < self.side:
                x, y, nx, ny = -h + r + s, -h, 0, -1
                break
            s -= self.side
            if s < self.arc:
                a = s / r
                x, y, nx, ny = h - r + r * math.sin(a), -h + r - r * math.cos(a), math.sin(a), -math.cos(a)
                break
            s -= self.arc
        else:
            x, y, nx, ny, k = -h + r, -h, 0, -1, 0
        for _ in range(k):
            x, y, nx, ny = -y, x, -ny, nx
        return c + x + nx * offset, c + y + ny * offset


def rail(rr, off, width):
    pts = [rr.at(rr.length * i / 800, off) for i in range(800)]
    return f'<path d="M{" L".join(f"{x:.1f},{y:.1f}" for x, y in pts)} Z" fill="none" stroke="{INK}" stroke-width="{width}"/>'


def meander(rr, band, width):
    """Meandro dobrado ao longo do quadrado. Grade: 6 de largura, 5 de altura; y=5 é a linha de base interna."""
    tiles = round(rr.length / (band * 6 / 5))
    w = rr.length / tiles
    tile = [(0, 5), (0, 0), (5, 0), (5, 4), (2, 4), (2, 2), (3.1, 2)]
    out = []
    for t in range(tiles):
        pts = []
        for (a, b) in zip(tile, tile[1:]):
            for i in range(8):
                u = i / 8
                pts.append((a[0] + (b[0] - a[0]) * u, a[1] + (b[1] - a[1]) * u))
        pts.append(tile[-1])
        xy = [rr.at((t + p[0] / 6) * w, band / 2 - p[1] / 5 * band) for p in pts]
        out.append("M" + " L".join(f"{x:.1f},{y:.1f}" for x, y in xy))
        steps = [rr.at((t + i / 8) * w, -band / 2) for i in range(9)]
        out.append("M" + " L".join(f"{x:.1f},{y:.1f}" for x, y in steps))
    return f'<path d="{" ".join(out)}" fill="none" stroke="{INK}" stroke-width="{width}" stroke-linejoin="miter" stroke-linecap="square"/>'


rr = RoundRect(S / 2 - M - 55, 190)
frame = rail(rr, 46, 9) + rail(rr, -46, 9) + meander(rr, 58, 8.5)
inc = f'fill="none" stroke="{BG}" stroke-linecap="round" stroke-linejoin="round"'


def caminhos(ds):
    return "".join(f'<path d="{d}"/>' for d in ds)


# ── a mão (direita, palma para baixo, os dedos um pouco curvos; o polegar do lado de quem olha) ──────────────────
# Coordenadas locais: o pulso em (0,0), os dedos para -x, y para baixo. No ícone ela vem espelhada, da esquerda.
MAO = ("M 150,-52 C 105,-50 50,-46 10,-44 "
       "C -20,-52 -60,-62 -100,-60 C -130,-58 -150,-54 -165,-50 "            # dorso até os nós dos dedos
       "C -210,-42 -250,-30 -274,-12 "                                       # o indicador, por cima
       "C -290,0 -296,14 -286,22 "                                           # ponta do indicador
       "C -300,30 -302,44 -290,50 "                                          # ponta do médio, mais dobrado
       "C -284,54 -276,54 -268,52 "
       "C -276,60 -272,70 -260,72 "                                          # ponta do anelar
       "C -246,74 -232,68 -222,64 "
       "C -200,60 -172,56 -152,56 "                                          # por baixo dos dedos, até o vão
       "C -170,68 -196,82 -214,92 "                                          # o polegar, por cima
       "C -230,100 -232,114 -218,118 "                                       # ponta do polegar
       "C -196,122 -160,112 -128,102 "                                       # o polegar, por baixo
       "C -90,92 -45,72 4,58 "                                               # a palma
       "C 50,60 100,62 150,62 Z")
MAO_INCISOES = [
    "M -170,-22 C -215,-14 -258,0 -284,20",                  # indicador | médio
    "M -176,4 C -220,14 -258,28 -280,46",                    # médio | anelar
    "M -182,30 C -215,40 -240,52 -258,66",                   # anelar | mínimo
    "M -270,-16 C -262,-10 -258,-2 -259,6",                  # unha do indicador
    "M -222,-38 C -216,-30 -214,-22 -218,-14",               # dobra do indicador
    "M -152,56 C -126,66 -98,72 -66,74",                     # monte do polegar
    "M -208,94 C -200,100 -198,108 -202,115",                # unha do polegar
    "M -18,-40 C -60,-44 -100,-44 -140,-38",                 # tendão no dorso
]
# a manga (o quíton), com a barra pontilhada no punho e as dobras
MANGA = ("M 140,-74 C 220,-92 360,-118 520,-140 L 530,160 C 380,136 240,104 140,80 "
         "C 126,36 124,-30 140,-74 Z")
MANGA_INCISOES = [
    "M 170,-78 C 158,-28 158,30 170,84",                     # a barra do punho
    "M 214,-44 C 300,-58 400,-74 500,-88",
    "M 210,-8 C 300,-14 400,-18 510,-22",
    "M 208,28 C 300,34 400,42 512,50",
    "M 214,62 C 300,76 400,92 500,108",
]


def mao(t):
    pulseira = (f'<path d="M 24,-45 C 33,-16 33,20 26,57" {inc} stroke-width="5.5"/>'
                f'<path d="M 46,-47 C 55,-16 55,20 48,59" {inc} stroke-width="5.5"/>'
                + "".join(f'<circle cx="{37 + 1.0 * k}" cy="{-30 + 19 * k}" r="4.6" fill="{BG}"/>' for k in range(5)))
    pontos = "".join(f'<circle cx="{154 - 0.04 * (v + 60)}" cy="{v}" r="4.6" fill="{BG}"/>' for v in range(-60, 76, 17))
    return (f'<g transform="{t}">'
            f'<path d="{MANGA}" fill="{INK}" stroke="{BG}" stroke-width="10" paint-order="stroke"/>'
            f'<g {inc} stroke-width="6">{caminhos(MANGA_INCISOES)}</g>{pontos}'
            f'<path d="{MAO}" fill="{INK}" stroke="{BG}" stroke-width="12" paint-order="stroke"/>'
            f'<g {inc} stroke-width="5.5">{caminhos(MAO_INCISOES)}</g>'
            f'{pulseira}</g>')


def pose(p):
    x, y, a, e = p["mao"]
    return f"translate({x},{y}) rotate({a}) scale({-e},{e})"


# ── a coluna jônica, com a inscrição no fuste ───────────────────────────────────────────────────────────────────
def voluta(cx, cy, r, lado):
    """A voluta do capitel: o disco negro com a espiral incisa, enrolando para dentro (lado: -1 esquerda, 1 direita)."""
    pts = []
    for k in range(161):
        u = k / 160
        a = math.pi * (0.5 + 5.0 * u)
        rr = r * (0.78 - 0.58 * u)
        pts.append((cx + lado * rr * math.cos(a), cy - rr * math.sin(a)))
    espiral = "M" + " L".join(f"{x:.1f},{y:.1f}" for x, y in pts)
    return (f'<circle cx="{cx}" cy="{cy}" r="{r}" fill="{INK}"/>'
            f'<path d="{espiral}" {inc} stroke-width="5"/>'
            f'<circle cx="{cx}" cy="{cy}" r="{r * 0.16:.1f}" fill="{BG}"/>')


def coluna(p):
    cx, y0, y1 = p["coluna"]                         # o eixo, o alto do fuste e o pé do fuste
    lt, lb = p["fuste"]                              # meia-largura do fuste em cima e embaixo (a êntase)
    fuste = f"M {cx - lt},{y0} L {cx + lt},{y0} L {cx + lb},{y1} L {cx - lb},{y1} Z"
    # caneluras curtas no alto e no pé do fuste; no meio, a faixa lisa da inscrição
    can = []
    for k in range(-3, 4):
        if k == 0:
            continue
        xt, xb = cx + k * lt / 3.6, cx + k * lb / 3.6
        can.append(f"M {xt:.1f},{y0 + 8} L {xt + (xb - xt) * 0.16:.1f},{y0 + 72}")
        can.append(f"M {xt + (xb - xt) * 0.84:.1f},{y1 - 72} L {xb:.1f},{y1 - 8}")
    lin_can = (f'<path d="{" ".join(can)}" stroke="{INK}" stroke-width="7" stroke-linecap="round"/>'
               f'<path d="M {cx - lt + 6},{y0 + 90} L {cx + lt - 6},{y0 + 90} M {cx - lb + 6},{y1 - 90} L {cx + lb - 6},{y1 - 90}" stroke="{INK}" stroke-width="6"/>')
    k = lt / 112                                     # o capitel acompanha a largura do fuste
    ab, vx, vr = 176 * k, 150 * k, 46 * k
    ova = "".join(f'<ellipse cx="{cx + j * 30 * k:.1f}" cy="{y0 - 52 * k:.1f}" rx="{10 * k:.1f}" ry="{14 * k:.1f}" fill="{BG}"/>' for j in range(-3, 4))
    capitel = (f'<rect x="{cx - ab:.1f}" y="{y0 - 118 * k:.1f}" width="{2 * ab:.1f}" height="{26 * k:.1f}" rx="4" fill="{INK}"/>'   # o ábaco
               f'<path d="M {cx - vx:.1f},{y0 - 92 * k:.1f} L {cx + vx:.1f},{y0 - 92 * k:.1f} C {cx + 128 * k:.1f},{y0 - 50 * k:.1f} {cx + 120 * k:.1f},{y0 - 22 * k:.1f} {cx + lt + 6},{y0 - 12} '
               f'L {cx - lt - 6},{y0 - 12} C {cx - 120 * k:.1f},{y0 - 22 * k:.1f} {cx - 128 * k:.1f},{y0 - 50 * k:.1f} {cx - vx:.1f},{y0 - 92 * k:.1f} Z" fill="{INK}"/>'
               + ova + voluta(cx - vx, y0 - 52 * k, vr, -1) + voluta(cx + vx, y0 - 52 * k, vr, 1)
               + f'<rect x="{cx - lt - 4}" y="{y0 - 14}" width="{2 * lt + 8}" height="14" fill="{INK}"/>')
    base = (f'<rect x="{cx - lb - 18}" y="{y1}" width="{2 * lb + 36}" height="30" rx="15" fill="{INK}"/>'
            f'<rect x="{cx - lb - 34}" y="{y1 + 32}" width="{2 * lb + 68}" height="34" rx="4" fill="{INK}"/>'
            f'<path d="M {cx - lb - 6},{y1 + 15} L {cx + lb + 6},{y1 + 15}" {inc} stroke-width="5"/>')
    return fuste, lin_can, capitel, base


def inscricao(p):
    cx, y0, y1 = p["coluna"]
    lt, lb = p["fuste"]
    sorte = random.Random(11)
    out = []
    for y in p["linhas"]:
        meia = lt + (lb - lt) * (y - y0) / (y1 - y0) - 26
        x, fim = cx - meia, cx + meia
        while x < fim - 12:
            w = min(sorte.choice([22, 30, 40, 52, 64]), fim - x)
            if w > 10:
                out.append(f"M {x:.1f},{y} L {x + w:.1f},{y}")
            x += w + 18
    return f'<path d="{" ".join(out)}" fill="none" stroke="{INK}" stroke-width="{p["grossura"]}" stroke-linecap="round"/>'


def figura(p):
    fuste, lin_can, capitel, base = coluna(p)
    dx, dy = p["sombra"]
    # a sombra da mão sobre o fuste: a mesma mão, deslocada (a luz vem do alto, à esquerda), só dentro do fuste
    sombra = (f'<clipPath id="f"><path d="{fuste}"/></clipPath>'
              f'<g clip-path="url(#f)"><g transform="translate({dx},{dy}) {pose(p)}">'
              f'<path d="{MAO}" fill="{INK}"/><path d="{MANGA}" fill="{INK}"/></g></g>')
    contorno = f'<path d="{fuste}" fill="none" stroke="{INK}" stroke-width="12" stroke-linejoin="round"/>'
    return inscricao(p) + lin_can + sombra + contorno + capitel + base + mao(pose(p))


P = dict(
    coluna=(612, 326, 806),              # o eixo da coluna, o alto e o pé do fuste
    fuste=(124, 136),                    # meia-largura do fuste em cima e embaixo
    linhas=(450, 492, 534, 576, 618, 660, 702),
    grossura=13,
    mao=(168, 420, -6, 1.08),             # o pulso (x, y), o giro e a escala da mão (que vem da esquerda)
    sombra=(206, 170),                   # deslocamento da sombra sobre o fuste (a luz vem do alto, à esquerda)
)


def montar(p=P):
    return f'''<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 {S} {S}" width="{S}" height="{S}">
<defs>
  <linearGradient id="t" x1="0" y1="0" x2="0" y2="1"><stop offset="0" stop-color="#E05610"/><stop offset="1" stop-color="#CF4A08"/></linearGradient>
  <clipPath id="q"><rect x="{S/2-494}" y="{S/2-494}" width="988" height="988" rx="241"/></clipPath>
  <clipPath id="i"><rect x="{S/2-397}" y="{S/2-397}" width="794" height="794" rx="144"/></clipPath>
</defs>
<g clip-path="url(#q)">
  <rect width="{S}" height="{S}" fill="url(#t)"/>
  {frame}
  <g clip-path="url(#i)">{figura(p)}</g>
</g>
</svg>
'''


def gravar(svg, svg_path, png_path):
    open(svg_path, "w").write(svg)
    subprocess.run(["rsvg-convert", "-w", str(S), "-h", str(S), svg_path, "-o", png_path], check=True)


if __name__ == "__main__":
    gravar(montar(), os.path.join(AQUI, "skia.svg"), os.path.join(AQUI, "icone.png"))
    print(os.path.join(AQUI, "icone.png"))
