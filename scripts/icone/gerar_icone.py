#!/usr/bin/env python3
"""Skiá — gera o ícone (figura negra sobre terracota, como os outros apps da família): uma mão grega
segura o guarda-sol grego (skiádeion), e a sombra dele esconde o meio das linhas da página — o nome sai, a forma fica.
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


# ── o guarda-sol grego (skiádeion, "o que faz sombra", da mesma raiz de skiá), seguro por uma mão ──────────────────
# Coordenadas locais: o punho em (0,0), a haste para cima (-y); o grupo todo gira em volta do punho.
def cupula(R, H, n=8, franja=22):
    """A cúpula aberta para baixo, como a dos vasos: o ápice em (0,0), a borda reta em y=H e a franja de pontas."""
    xs = [-R + 2 * R * k / n for k in range(n + 1)]
    d = f"M {-R},{H} C {-R * 0.92:.1f},{H * 0.52:.1f} {-R * 0.42:.1f},{H * 0.06:.1f} 0,0 C {R * 0.42:.1f},{H * 0.06:.1f} {R * 0.92:.1f},{H * 0.52:.1f} {R},{H}"
    m = 2 * n
    for k in range(m):
        xa, xb = R - 2 * R * k / m, R - 2 * R * (k + 1) / m
        d += f" L {xa:.1f},{H} L {(xa + xb) / 2:.1f},{H + franja} L {xb:.1f},{H}"
    return d + " Z", xs


def guarda_sol(p):
    R, H, L = p["raio"], p["altura"], p["haste"]
    dome, xs = cupula(R, H)
    ty = -L - H                                         # o ápice
    varetas = " ".join(f"M {x * 0.08:.1f},{ty + 16} L {x * 0.97:.1f},{ty + H - 4}" for x in xs[1:-1])
    faixa = f"M {-R * 0.93:.1f},{ty + H * 0.70:.1f} Q 0,{ty + H * 0.52:.1f} {R * 0.93:.1f},{ty + H * 0.70:.1f}"
    pontos = "".join(f'<circle cx="{x * 0.94:.1f}" cy="{ty + H * 0.86 - 0.10 * H * (1 - (x / R) ** 2):.1f}" r="4.4" fill="{BG}"/>'
                     for x in [-R + 2 * R * (k + 0.5) / 12 for k in range(12)])
    return (f'<rect x="-9" y="{ty - 10}" width="18" height="{L + H + 70}" rx="9" fill="{INK}"/>'          # a haste
            f'<circle cx="0" cy="{ty - 20}" r="14" fill="{INK}"/>'                                           # o remate
            f'<path d="{dome}" transform="translate(0,{ty})" fill="{INK}" stroke="{BG}" stroke-width="10" paint-order="stroke"/>'
            f'<path d="{varetas}" {inc} stroke-width="5"/>'
            f'<path d="{faixa}" {inc} stroke-width="5"/>{pontos}')


# o punho fechado em volta da haste (local, gira com ela): o polegar por cima, os nós dos dedos à direita
PUNHO = ("M -50,-30 C -40,-58 -6,-66 22,-62 C 40,-60 54,-52 60,-40 C 70,-32 72,-16 66,-8 "
         "C 74,2 74,18 66,26 C 72,36 70,50 58,56 C 44,66 20,70 -6,68 C -30,66 -52,54 -58,34 C -62,14 -58,-12 -50,-30 Z")
PUNHO_INCISOES = [
    "M -40,-40 C -14,-56 18,-56 40,-44 C 50,-38 52,-28 44,-22 C 30,-16 8,-20 -8,-26",          # o polegar
    "M 34,-8 C 46,-8 56,-6 64,-6", "M 34,18 C 46,18 56,20 64,22", "M 30,42 C 42,44 50,46 58,50",  # os dedos
]


def braco(X, Y):
    """O antebraço que sobe da borda de baixo até o punho, com a pulseira e a manga do quíton (sem girar)."""
    ante = f"M {X - 46},{Y + 30} C {X - 20},{Y + 50} {X + 14},{Y + 52} {X + 34},{Y + 44} L {X + 44},{Y + 170} L {X - 60},{Y + 170} Z"
    manga = (f"M {X - 74},{Y + 150} C {X - 30},{Y + 140} {X + 30},{Y + 140} {X + 62},{Y + 150} "
             f"L {X + 96},{Y + 330} L {X - 120},{Y + 330} Z")
    dobras = [f"M {X - 40},{Y + 192} L {X - 70},{Y + 320}", f"M {X - 6},{Y + 194} L {X - 12},{Y + 320}",
              f"M {X + 28},{Y + 192} L {X + 48},{Y + 320}"]
    barra = f"M {X - 70},{Y + 172} C {X - 30},{Y + 164} {X + 30},{Y + 164} {X + 64},{Y + 172}"
    pulseira = (f"M {X - 52},{Y + 96} C {X - 20},{Y + 92} {X + 14},{Y + 92} {X + 40},{Y + 96} "
                f"M {X - 54},{Y + 120} C {X - 20},{Y + 116} {X + 14},{Y + 116} {X + 41},{Y + 120}")
    pontos = "".join(f'<circle cx="{X - 40 + 19 * k}" cy="{Y + 107}" r="4.4" fill="{BG}"/>' for k in range(5))
    return (f'<path d="{manga}" fill="{INK}" stroke="{BG}" stroke-width="10" paint-order="stroke"/>'
            f'<g {inc} stroke-width="6">{caminhos(dobras + [barra])}</g>'
            f'<path d="{ante}" fill="{INK}" stroke="{BG}" stroke-width="10" paint-order="stroke"/>'
            f'<path d="{pulseira}" {inc} stroke-width="5"/>{pontos}')


def pose(p):
    x, y, a = p["mao"]
    return f"translate({x},{y}) rotate({a})"


def segurando(p):
    x, y, _ = p["mao"]
    return (f'<g transform="{pose(p)}">' + guarda_sol(p) + '</g>' + braco(x, y)
            + f'<g transform="{pose(p)}"><path d="{PUNHO}" fill="{INK}" stroke="{BG}" stroke-width="10" paint-order="stroke"/>'
            + f'<g {inc} stroke-width="5.5">{caminhos(PUNHO_INCISOES)}</g></g>')


def folha(p):
    """A página, com a orelha dobrada no canto, e as linhas do texto."""
    x0, y0, x1, y1 = p["folha"]
    o = 74
    contorno = f"M {x0},{y0} L {x1 - o},{y0} L {x1},{y0 + o} L {x1},{y1} L {x0},{y1} Z"
    orelha = f"M {x1 - o},{y0} L {x1 - o},{y0 + o} L {x1},{y0 + o}"
    sorte = random.Random(5)
    linhas = []
    for n, y in enumerate(p["linhas"]):
        x, fim = x0 + 34, x1 - 34 - (o if y < y0 + o + 10 else 0) - (110 if n == len(p["linhas"]) - 1 else 0)
        while x < fim - 20:
            w = min(sorte.choice([30, 42, 56, 70, 86]), fim - x)
            if w > 14:
                linhas.append(f"M {x},{y} L {x + w},{y}")
            x += w + 22
    texto = f'<path d="{" ".join(linhas)}" fill="none" stroke="{INK}" stroke-width="{p["grossura"]}" stroke-linecap="round"/>'
    return contorno, orelha, texto


def figura(p):
    contorno, orelha, texto = folha(p)
    R, H, L = p["raio"], p["altura"], p["haste"]
    dome, _ = cupula(R * 1.06, H * 0.96)
    dx, dy = p["sombra"]
    # a sombra da cúpula sobre a página: a mesma cúpula, deslocada (a luz vem do alto, à esquerda), só dentro da folha
    sombra = (f'<clipPath id="f"><path d="{contorno}"/></clipPath>'
              f'<g clip-path="url(#f)"><g transform="translate({dx},{dy}) {pose(p)}">'
              f'<path d="{dome}" transform="translate(0,{-L - H})" fill="{INK}"/></g></g>')
    pagina = (f'<path d="{contorno}" fill="none" stroke="{INK}" stroke-width="12" stroke-linejoin="round"/>'
              f'<path d="{orelha}" fill="none" stroke="{INK}" stroke-width="10" stroke-linejoin="round"/>')
    return texto + sombra + pagina + segurando(p)


P = dict(
    folha=(470, 404, 840, 872),          # x0, y0, x1, y1 da página
    linhas=(470, 520, 570, 620, 670, 720, 770, 820),
    grossura=15,
    mao=(240, 690, 20),                   # o punho (x, y) e o giro da haste
    raio=180, altura=120, haste=330,      # a cúpula (meia-largura, altura) e a haste do punho até a borda
    sombra=(240, 220),
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
