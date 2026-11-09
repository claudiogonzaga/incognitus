#!/usr/bin/env python3
"""Gera os 3 PDFs FICTÍCIOS do teste de ponta a ponta (nenhum dado real: nomes no estilo da suíte do Graphé,
"TESTEFULANO", "FICTICIA", "PIRAMBOIA"; CPF de teste; endereços inventados).
    texto.pdf       3 páginas com camada de texto (capa do PJe, termo de depoimento, despacho)
    escaneado.pdf   2 páginas só de imagem (sem texto: tudo vai ao OCR)
    misto.pdf       3 páginas: texto, imagem, texto
Uso: python3 testes/gerar_pdfs.py [pasta]   (padrão testes/pdfs/; precisa de reportlab, Pillow e PyMuPDF)
"""
import io
import os
import sys

import pymupdf as fitz
from PIL import Image, ImageDraw, ImageFont
from reportlab.lib.pagesizes import A4
from reportlab.pdfgen import canvas

PASTA = os.path.abspath(sys.argv[1] if len(sys.argv) > 1 else os.path.join(os.path.dirname(__file__), "pdfs"))

# As pessoas privadas fictícias (o teste confere que NENHUM destes dados fica em claro na saída).
PESSOAS = {
    "reu": "JOAO TESTEFULANO PIRAMBOIA",
    "vitima": "MARIA TESTEFULANA PIRES",
    "testemunha": "PEDRO FICTICIO BANDEIRA",
    "pai": "GERALDO TESTEFULANO PIRAMBOIA",
    "mae": "ANTONIETA FICTICIA PIRAMBOIA",
    "vitima2": "ROSIMEIRE TESTEFULANA CAMPOS",
    "testemunha2": "JOSE FICTICIO GUIMARAES",
}
IDENT = {"cpf": "123.456.789-09", "tel": "99876-5432", "rg": "23456789", "nasc": "03/04/1991", "rua": "Rua das Mangueiras Fictícias"}
# Servidores e autoridades ficam em claro por regra do motor (também fictícios).
AUTORIDADES = ["ROBERVAL FICTICIO SILVA"]
# O rodapé que o PJe põe no PDF: quem assina e quem gerou o PDF (CPF mascarado). Saem da saída como "[omitido]".
ASSINANTE = "TESTEJUIZA HELENA FICTICIA MOURA"
USUARIO_PJE = "987.***.***-21"
RODAPE_PJE = [
    f"Assinado eletronicamente por: {ASSINANTE} - 15/03/2026 17:21:45",
    "https://pje.tjmt.jus.br:443/pje/Processo/ConsultaDocumento/listView.seam?x=26031517214512300000001234567",
    "Número do documento: 26031517214512300000001234567",
    "Num. 1234567 - Pág. 1",
    f"Este documento foi gerado pelo usuário {USUARIO_PJE} em 09/11/2026 10:00:00",
]

CAPA = [
    "PODER JUDICIÁRIO DO ESTADO DE MATO GROSSO",
    "COMARCA DE CIDADELA - VARA ÚNICA",
    "Processo nº 1000123-45.2026.8.11.0027",
    "AUTOR: MINISTÉRIO PÚBLICO DO ESTADO DE MATO GROSSO",
    f"RÉU: {PESSOAS['reu']}",
    f"VÍTIMA: {PESSOAS['vitima']}",
    "",
    "DENÚNCIA",
    "",
    f"O Ministério Público oferece denúncia contra {PESSOAS['reu']}, brasileiro, solteiro,",
    f"nascido em {IDENT['nasc']}, inscrito no CPF nº {IDENT['cpf']}, filho de {PESSOAS['pai']}",
    f"e {PESSOAS['mae']}, residente na {IDENT['rua']}, nº 410, Bairro Jardim Inventado,",
    f"telefone (65) {IDENT['tel']}, pelos fatos a seguir narrados.",
    f"No dia 10/03/2026, o denunciado agrediu a vítima {PESSOAS['vitima']}, brasileira, solteira,",
    f"portadora do RG {IDENT['rg']} SSP MT, causando-lhe as lesões descritas no laudo de fls. 12.",
]
DEPOIMENTO = [
    "TERMO DE DEPOIMENTO",
    "",
    f"Testemunha: {PESSOAS['testemunha']}, brasileiro, casado, comerciante.",
    "Às perguntas respondeu que no dia dos fatos estava em casa quando ouviu os gritos",
    "da vizinha,",
    f"que saiu à rua e viu o acusado {PESSOAS['reu']} empurrando a vítima",
    "contra o muro, que a vítima gritava por socorro e que o acusado fugiu logo depois",
    "numa motocicleta vermelha, sem placa visível.",
    "",
    "Nada mais disse nem lhe foi perguntado.",
    "",
    f"{AUTORIDADES[0]}",
    "Delegado de Polícia",
]
DESPACHO = [
    "DESPACHO",
    "",
    "Recebo a denúncia, pois presentes os requisitos do art. 41 do Código de Processo Penal.",
    "Cite-se o acusado para responder à acusação no prazo de dez dias.",
    "Intime-se a vítima e as testemunhas arroladas, com as cautelas de praxe.",
    "",
    "Cidadela, 15 de março de 2026.",
]
ESCANEADA1 = [
    "BOLETIM DE OCORRENCIA",
    "",
    f"Comunicante: {PESSOAS['vitima2']}, brasileira, casada,",
    "do lar, residente nesta cidade.",
    "Relata a comunicante que teve a bicicleta furtada",
    "da porta de casa durante a madrugada e que suspeita",
    f"do vizinho {PESSOAS['testemunha2']}, que foi visto",
    "rondando a residencia na noite anterior.",
]
ESCANEADA2 = [
    "TERMO DE DECLARACOES",
    "",
    f"Declarante: {PESSOAS['testemunha2']}, brasileiro,",
    "solteiro, pedreiro, nega a autoria do furto e afirma",
    "que passou a noite na casa da mae, no bairro vizinho,",
    "e que nunca entrou na residencia da comunicante.",
]


def pagina_texto(c, linhas, rodape=None):
    y = A4[1] - 60
    for l in linhas:
        c.setFont("Helvetica", 11)
        c.drawString(56, y, l)
        y -= 16
    y = 90
    for l in rodape or []:
        c.setFont("Helvetica", 7)
        c.drawString(56, y, l)
        y -= 10
    c.showPage()


def pdf_texto(paginas, rodapes=None):
    b = io.BytesIO()
    c = canvas.Canvas(b, pagesize=A4)
    c.setTitle("teste ficticio")
    for k, p in enumerate(paginas):
        pagina_texto(c, p, (rodapes or {}).get(k))
    c.save()
    return b.getvalue()


def fonte():
    for f in ("/System/Library/Fonts/Supplemental/Arial.ttf", "/Library/Fonts/Arial.ttf", "/System/Library/Fonts/Helvetica.ttc",
              "C:/Windows/Fonts/arial.ttf"):
        if os.path.exists(f):
            return ImageFont.truetype(f, 38)
    raise SystemExit("sem fonte para desenhar a página escaneada")


def imagem_pagina(linhas):
    """Uma folha A4 a 200 dpi, só imagem (como um scanner faria)."""
    w, h = 1654, 2339
    im = Image.new("L", (w, h), 255)
    d = ImageDraw.Draw(im)
    f = fonte()
    y = 180
    for l in linhas:
        d.text((150, y), l, fill=0, font=f)
        y += 62
    return im


def pdf_imagens(paginas_imagem):
    doc = fitz.open()
    for im in paginas_imagem:
        b = io.BytesIO()
        im.save(b, format="PNG")
        pg = doc.new_page(width=A4[0], height=A4[1])
        pg.insert_image(pg.rect, stream=b.getvalue())
    return doc


def main():
    os.makedirs(PASTA, exist_ok=True)
    open(os.path.join(PASTA, "texto.pdf"), "wb").write(pdf_texto([CAPA, DEPOIMENTO, DESPACHO], {2: RODAPE_PJE}))
    pdf_imagens([imagem_pagina(ESCANEADA1), imagem_pagina(ESCANEADA2)]).save(os.path.join(PASTA, "escaneado.pdf"))
    misto = fitz.open(stream=pdf_texto([CAPA]), filetype="pdf")
    misto.insert_pdf(pdf_imagens([imagem_pagina(ESCANEADA1)]))
    misto.insert_pdf(fitz.open(stream=pdf_texto([DEPOIMENTO]), filetype="pdf"))
    misto.save(os.path.join(PASTA, "misto.pdf"))
    print("3 PDFs fictícios em", PASTA)


if __name__ == "__main__":
    main()
