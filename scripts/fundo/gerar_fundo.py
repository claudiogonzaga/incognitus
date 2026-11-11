#!/usr/bin/env python3
"""Incognitus — gera o pano de fundo da janela (modulo/fundo.webp): as três colunas do Fórum Romano, em aquarela, com a
inscrição na arquitrave — NOMEN · ABIT · FORMA · MANET, "o nome sai, a forma fica" — e o NOMEN raspado a cinzel,
como na damnatio memoriae, com uma sombra caindo sobre esse trecho. Sutil: fica atrás dos painéis.
Uso: python3 scripts/fundo/gerar_fundo.py   (rsvg-convert para o desenho; Pillow, com a Cinzel de modulo/fontes, para a
inscrição e o .webp)
"""
import math
import os
import random
import subprocess
import tempfile

from PIL import Image, ImageDraw, ImageFilter, ImageFont

AQUI = os.path.dirname(os.path.abspath(__file__))
RAIZ = os.path.dirname(os.path.dirname(AQUI))
FONTE = os.path.join(RAIZ, "modulo", "fontes")
W, H = 1000, 1000
SEPIA, OCRE, SOMBRA, POMPEIA = "#7a5a3c", "#b8946a", "#3e2a1c", "#8e2a1e"
sorte = random.Random(9)

FILTROS = """
<filter id="aquarela" x="-10%" y="-10%" width="120%" height="120%">
  <feTurbulence type="fractalNoise" baseFrequency="0.018" numOctaves="3" seed="4" result="ruido"/>
  <feDisplacementMap in="SourceGraphic" in2="ruido" scale="9" xChannelSelector="R" yChannelSelector="G" result="torto"/>
  <feTurbulence type="fractalNoise" baseFrequency="0.09" numOctaves="2" seed="7" result="grao"/>
  <feColorMatrix in="grao" type="matrix" values="0 0 0 0 0  0 0 0 0 0  0 0 0 0 0  0 0 0 -1.1 1.25" result="poros"/>
  <feComposite in="torto" in2="poros" operator="in" result="pigmento"/>
  <feGaussianBlur in="pigmento" stdDeviation="0.7"/>
</filter>
<filter id="difusa" x="-20%" y="-10%" width="140%" height="120%"><feGaussianBlur stdDeviation="14"/></filter>
<filter id="borda" x="-10%" y="-10%" width="120%" height="120%">
  <feTurbulence type="fractalNoise" baseFrequency="0.03" numOctaves="2" seed="11" result="r"/>
  <feDisplacementMap in="SourceGraphic" in2="r" scale="5" xChannelSelector="R" yChannelSelector="G" result="t"/>
  <feGaussianBlur in="t" stdDeviation="1.2"/>
</filter>
<filter id="raspado" x="-5%" y="-20%" width="110%" height="140%">
  <feTurbulence type="fractalNoise" baseFrequency="0.35 0.9" numOctaves="2" seed="3" result="cinzel"/>
  <feColorMatrix in="cinzel" type="matrix" values="0 0 0 0 0.62  0 0 0 0 0.52  0 0 0 0 0.40  0 0 0 -2.2 1.6" result="marcas"/>
  <feComposite in="marcas" in2="SourceGraphic" operator="in"/>
</filter>
<linearGradient id="sombra" x1="0" y1="0" x2="1" y2="1">
  <stop offset="0" stop-color="#3e2a1c" stop-opacity="0.55"/><stop offset="1" stop-color="#3e2a1c" stop-opacity="0"/>
</linearGradient>
"""


def coluna(cx, topo, pe, larg):
    """Fuste corintio com caneluras, capitel de acanto e base, em lavagens de ocre."""
    lt, lb = larg * 0.44, larg * 0.5
    fuste = f'<path d="M {cx - lt},{topo} L {cx + lt},{topo} L {cx + lb},{pe} L {cx - lb},{pe} Z" fill="{OCRE}" fill-opacity="0.55"/>'
    sombra = f'<path d="M {cx + lt * 0.25},{topo} L {cx + lt},{topo} L {cx + lb},{pe} L {cx + lb * 0.25},{pe} Z" fill="{SEPIA}" fill-opacity="0.35"/>'
    can = "".join(f'<path d="M {cx + k * lt / 4:.1f},{topo + 6} L {cx + k * lb / 4:.1f},{pe - 6}" stroke="{SEPIA}" stroke-opacity="0.32" stroke-width="2.2"/>'
                  for k in (-3, -2, -1, 1, 2, 3))
    capitel = (f'<path d="M {cx - larg * 0.95},{topo - 48} L {cx + larg * 0.95},{topo - 48} L {cx + larg * 0.62},{topo} L {cx - larg * 0.62},{topo} Z" '
               f'fill="{OCRE}" fill-opacity="0.6"/>'
               + "".join(f'<path d="M {cx + k * larg * 0.3:.1f},{topo} q {-larg * 0.12:.1f},-22 {k * 4:.1f},-40 q {larg * 0.14:.1f},14 {larg * 0.04:.1f},40 Z" '
                         f'fill="{SEPIA}" fill-opacity="0.35"/>' for k in (-2, -1, 0, 1, 2))
               + f'<circle cx="{cx - larg * 0.78:.1f}" cy="{topo - 36}" r="9" fill="none" stroke="{SEPIA}" stroke-opacity="0.55" stroke-width="3"/>'
               + f'<circle cx="{cx + larg * 0.78:.1f}" cy="{topo - 36}" r="9" fill="none" stroke="{SEPIA}" stroke-opacity="0.55" stroke-width="3"/>')
    base = (f'<rect x="{cx - lb - 10}" y="{pe}" width="{2 * lb + 20}" height="16" rx="7" fill="{OCRE}" fill-opacity="0.6"/>'
            f'<rect x="{cx - lb - 20}" y="{pe + 16}" width="{2 * lb + 40}" height="18" fill="{SEPIA}" fill-opacity="0.35"/>')
    return fuste + sombra + can + capitel + base


def montar():
    x0, x1 = 400, 985                   # a arquitrave
    topo, pe = 312, 852                 # o fuste
    cols = "".join(coluna(cx, topo, pe, 72) for cx in (488, 688, 888))
    entab = (f'<rect x="{x0}" y="196" width="{x1 - x0}" height="66" fill="{OCRE}" fill-opacity="0.58"/>'            # arquitrave
             f'<rect x="{x0 - 6}" y="150" width="{x1 - x0 + 12}" height="46" fill="{OCRE}" fill-opacity="0.42"/>'     # friso
             f'<rect x="{x0 - 16}" y="118" width="{x1 - x0 + 32}" height="32" fill="{SEPIA}" fill-opacity="0.42"/>'   # cornija
             + "".join(f'<rect x="{x0 - 10 + k * 19}" y="150" width="9" height="11" fill="{SEPIA}" fill-opacity="0.38"/>'
                       for k in range(int((x1 - x0 + 20) / 19)))
             + f'<path d="M {x0},{204} L {x1},{204} M {x0},{254} L {x1},{254}" stroke="{SEPIA}" stroke-opacity="0.35" stroke-width="2"/>')
    podio = (f'<rect x="{x0 - 30}" y="{pe + 34}" width="{x1 - x0 + 60}" height="120" fill="{OCRE}" fill-opacity="0.45"/>'
             f'<path d="M {x0 - 30},{pe + 74} L {x1 + 30},{pe + 74}" stroke="{SEPIA}" stroke-opacity="0.3" stroke-width="2"/>')
    # ao longe, à esquerda: um arco e dois tocos de coluna, bem claros
    longe = (f'<path d="M 60,860 L 60,520 L 330,520 L 330,860 L 250,860 L 250,690 Q 195,600 140,690 L 140,860 Z" fill="{OCRE}" fill-opacity="0.22"/>'
             f'<rect x="40" y="490" width="310" height="34" fill="{OCRE}" fill-opacity="0.2"/>'
             f'<rect x="352" y="720" width="34" height="140" fill="{OCRE}" fill-opacity="0.22"/>'
             f'<rect x="20" y="760" width="30" height="100" fill="{OCRE}" fill-opacity="0.18"/>')
    return f'<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 {W} {H}" width="{W}" height="{H}"><defs>{FILTROS}</defs>' \
           f'<g filter="url(#aquarela)">{longe}{podio}{cols}{entab}</g></svg>'


INSCRICAO, PALAVRA = "NOMEN · ABIT · FORMA · MANET", "NOMEN"
X0, X1, BASE, CORPO, ESPACO = 400, 985, 241, 31, 3     # a arquitrave (em unidades do desenho), a linha de base e o corpo


def letras(lado):
    """A inscrição em Cinzel (negrito), letra a letra com o espaçamento: [(letra, x)] em pixels e onde fica o NOMEN."""
    k = lado / W
    fonte = ImageFont.truetype(os.path.join(FONTE, "Cinzel.ttf"), round(CORPO * k))
    try:
        fonte.set_variation_by_name("Bold")
    except Exception:
        pass
    larg = [fonte.getlength(c) for c in INSCRICAO]
    total = sum(larg) + ESPACO * k * (len(INSCRICAO) - 1)
    x = (X0 + X1) / 2 * k - total / 2
    pos = []
    for c, w in zip(INSCRICAO, larg):
        pos.append((c, x))
        x += w + ESPACO * k
    ini = pos[0][1] / k
    fim = (pos[len(PALAVRA) - 1][1] + larg[len(PALAVRA) - 1]) / k
    return fonte, pos, (ini, fim)


def topo_svg(rx0, rx1):
    """Por cima da inscrição: o NOMEN raspado a cinzel (borda irregular) e a sombra que cai sobre ele."""
    pts = [(rx0 + sorte.uniform(-4, 4), 206 + sorte.uniform(-2, 2))]
    for k in range(1, 9):
        pts.append((rx0 + (rx1 - rx0) * k / 8 + sorte.uniform(-3, 3), 205 + sorte.uniform(-3, 3)))
    for k in range(8, -1, -1):
        pts.append((rx0 + (rx1 - rx0) * k / 8 + sorte.uniform(-3, 3), 253 + sorte.uniform(-3, 3)))
    poli = "M " + " L ".join(f"{x:.1f},{y:.1f}" for x, y in pts) + " Z"
    golpes = "".join(f'<path d="M {rx0 + 6 + j * 9:.1f},249 l 7,-38" stroke="{SEPIA}" stroke-opacity="0.22" stroke-width="1.6"/>'
                     for j in range(int((rx1 - rx0 - 10) / 9)))
    sombra = f'<path d="M {rx0 - 20},170 L {rx1 + 50},170 L 560,900 L 420,900 Z" fill="url(#sombra)" opacity="0.42" filter="url(#difusa)"/>'
    return (f'<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 {W} {H}" width="{W}" height="{H}"><defs>{FILTROS}</defs>'
            f'<path d="{poli}" fill="#d9c7a6"/><path d="{poli}" fill="#ffffff" filter="url(#raspado)"/>{golpes}{sombra}</svg>')


def render(svg, lado, tmp, nome):
    sv, png = os.path.join(tmp, nome + ".svg"), os.path.join(tmp, nome + ".png")
    open(sv, "w").write(svg)
    subprocess.run(["rsvg-convert", "-w", str(lado), "-h", str(lado), sv, "-o", png], check=True)
    return Image.open(png).convert("RGBA")


def gravar(saida_webp, lado=1500, opacidade=0.62):
    fonte, pos, (ini, fim) = letras(lado)
    with tempfile.TemporaryDirectory() as tmp:
        im = render(montar(), lado, tmp, "base")
        # a inscrição, gravada na pedra: sépia translúcida, a borda um pouco borrada como a tinta na aquarela
        txt = Image.new("RGBA", im.size, (0, 0, 0, 0))
        d = ImageDraw.Draw(txt)
        for c, x in pos:
            d.text((x, BASE * lado / W), c, font=fonte, fill=(62, 42, 28, 190), anchor="ls")
        im.alpha_composite(txt.filter(ImageFilter.GaussianBlur(0.7)))
        im.alpha_composite(render(topo_svg(ini - 7, fim + 7), lado, tmp, "topo"))
    im.putalpha(im.getchannel("A").point(lambda v: int(v * opacidade)))
    im.save(saida_webp, "WEBP", quality=82, method=6)
    return saida_webp


if __name__ == "__main__":
    print(gravar(os.path.join(RAIZ, "modulo", "fundo.webp")))
