#!/usr/bin/env python3
"""Incognitus — três propostas de logo mais avermelhadas (18/11/2026), a partir da pedra atual (scripts/icone/icone.png):
A) a mesma pedra, em terracota; B) a pedra assentada em tesselas (mosaico); C) mosaico em negativo: fundo vermelho
pompeiano, letras claras e moldura de tesselas. Uso: python3 scripts/icone/propostas/gerar_propostas.py"""
import os
import random
from PIL import Image, ImageDraw, ImageFont, ImageFilter

AQUI = os.path.dirname(os.path.abspath(__file__))
BASE = Image.open(os.path.join(AQUI, "..", "icone.png")).convert("RGBA").resize((1024, 1024), Image.LANCZOS)
sorte = random.Random(7)


def rampa(paradas):
    """Uma tabela de 256 cores a partir de [(posição 0..1, (r,g,b))]."""
    lut = []
    for v in range(256):
        t = v / 255
        for (p0, c0), (p1, c1) in zip(paradas, paradas[1:]):
            if p0 <= t <= p1:
                k = (t - p0) / (p1 - p0 or 1)
                lut.append(tuple(round(c0[j] + (c1[j] - c0[j]) * k) for j in range(3)))
                break
    return lut


def colorir(im, paradas):
    lum, alfa = im.convert("L"), im.getchannel("A")
    lut = rampa(paradas)
    out = Image.merge("RGB", [lum.point([c[j] for c in lut]) for j in range(3)]).convert("RGBA")
    out.putalpha(alfa)
    return out


def mosaico(im, n, paleta, rejunte, moldura=None):
    """A imagem em n×n tesselas: cada uma leva a cor da paleta mais perto da média do pedaço, com um leve desvio."""
    lado = im.size[0]
    peq = im.resize((n, n), Image.BOX)
    out = Image.new("RGBA", im.size, (0, 0, 0, 0))
    d = ImageDraw.Draw(out)
    t = lado / n
    for y in range(n):
        for x in range(n):
            r, g, b, a = peq.getpixel((x, y))
            if a < 110:
                continue
            borda = moldura and (min(x, y, n - 1 - x, n - 1 - y) in (1, 2))
            if borda:
                cor = moldura[(x + y) % len(moldura)]
            else:
                cor = min(paleta, key=lambda c: (c[0] - r) ** 2 + (c[1] - g) ** 2 + (c[2] - b) ** 2)
            j = sorte.randint(-9, 9)
            cor = tuple(max(0, min(255, v + j)) for v in cor)
            f = sorte.uniform(0.6, 1.4)
            d.rounded_rectangle([x * t + f, y * t + f, (x + 1) * t - f, (y + 1) * t - f], radius=t * 0.16, fill=cor + (255,))
    fundo = Image.new("RGBA", im.size, rejunte + (255,))
    fundo.putalpha(im.getchannel("A").point(lambda v: 255 if v > 110 else 0).filter(ImageFilter.GaussianBlur(1.2)))
    fundo.alpha_composite(out)
    return fundo


TERRACOTA = [(0, (46, 14, 10)), (0.35, (112, 36, 26)), (0.62, (176, 78, 52)), (0.82, (214, 138, 104)), (1, (242, 208, 178))]
A = colorir(BASE, TERRACOTA)


def desenho(fundo, faixa, letra, letra_na_faixa):
    """O desenho limpo que vira mosaico: o quadrado de cantos redondos, a faixa de sombra no meio e as três linhas do nome."""
    im = Image.new("RGBA", (1024, 1024), (0, 0, 0, 0))
    d = ImageDraw.Draw(im)
    d.rounded_rectangle([44, 44, 980, 980], radius=170, fill=fundo + (255,))
    d.rectangle([44, 396, 980, 628], fill=faixa + (255,))
    fonte = ImageFont.truetype(os.path.join(AQUI, "..", "..", "..", "modulo", "fontes", "Cinzel.ttf"), 218)
    try:
        fonte.set_variation_by_name("Black")
    except Exception:
        pass
    for texto, y, cor in (("INCO", 290, letra), ("GNI", 512, letra_na_faixa), ("TUS", 736, letra)):
        d.text((512, y), texto, font=fonte, fill=cor + (255,), anchor="mm", stroke_width=7, stroke_fill=cor + (255,))
    return im


B = mosaico(desenho((228, 176, 138), (150, 52, 36), (98, 30, 22), (84, 26, 18)), 60,
            [(240, 206, 172), (228, 176, 138), (214, 150, 112), (150, 52, 36), (98, 30, 22), (84, 26, 18)], (244, 230, 210))
C = mosaico(desenho((122, 36, 27), (38, 14, 10), (250, 236, 214), (168, 62, 42)), 60,
            [(250, 236, 214), (168, 62, 42), (122, 36, 27), (104, 30, 22), (38, 14, 10)], (70, 30, 22))

for nome, im in (("A-pedra-terracota", A), ("B-mosaico", B), ("C-mosaico-negativo", C)):
    im.save(os.path.join(AQUI, nome + ".png"))

# a folha com as três, sobre o fundo da janela
folha = Image.new("RGB", (1500, 620), (241, 223, 204))
d = ImageDraw.Draw(folha)
try:
    fonte = ImageFont.truetype(os.path.join(AQUI, "..", "..", "..", "modulo", "fontes", "Cinzel.ttf"), 30)
    pequena = ImageFont.truetype("/System/Library/Fonts/Supplemental/Arial.ttf", 22)
except Exception:
    fonte = pequena = ImageFont.load_default()
for k, (rot, sub, im) in enumerate((("A", "a pedra de hoje, em terracota", A), ("B", "a pedra em mosaico", B), ("C", "mosaico em negativo", C))):
    x = 60 + k * 480
    folha.paste(im.resize((420, 420), Image.LANCZOS), (x, 40), im.resize((420, 420), Image.LANCZOS))
    folha.paste(im.resize((64, 64), Image.LANCZOS), (x + 356, 500), im.resize((64, 64), Image.LANCZOS))
    d.text((x, 492), rot, font=fonte, fill=(94, 28, 21))
    d.text((x, 540), sub, font=pequena, fill=(90, 70, 58))
folha.save(os.path.join(AQUI, "propostas.png"))
print("ok")
