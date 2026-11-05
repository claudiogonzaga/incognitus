#!/usr/bin/env python3
"""Σκιά · Skiá — teste de ponta a ponta com os 3 PDFs FICTÍCIOS (texto, escaneado, misto).

1. Gera os PDFs (gerar_pdfs.py) e guarda o sha256 e a data de cada original.
2. Roda o APP DE VERDADE em modo de teste (SKIA_E2E=plano.json; dados e cache numa pasta à parte, SKIA_DADOS): fila,
   pdftotext e OCR pelos sidecars, motor no Web Worker, PDF de saída. O plano pede também a atualização das regras de
   um servidor local: um motor republicado e assinado com a chave do Skiá (tem de ser instalado depois de passar nos
   casos de teste) e, numa segunda execução, um manifesto adulterado (tem de ser recusado).
3. Confere: nomes de saída iguais aos de entrada; nenhum dado de pessoa fictícia em claro na saída (PDF e .txt);
   originais intactos; o cabeçalho com a versão do motor; uma folha por página dos autos + a do cabeçalho; fonte
   monoespaçada de 11 pt.
4. Compara a extração do Skiá com a do serviço do Graphé (cloudrun-pdftotext/main.py rodando aqui, com as MESMAS
   ferramentas embutidas no Skiá e o mesmo pedido que o Graphé faz): tem de ser idêntica, byte a byte.
Uso: python3 testes/e2e.py [executável]   (padrão: src-tauri/target/debug/skia, de `npx tauri build --debug --no-bundle`)
"""
import hashlib
import http.server
import json
import os
import shutil
import socket
import subprocess
import sys
import threading
import time
import urllib.request
import uuid

import pymupdf

RAIZ = os.path.dirname(os.path.dirname(os.path.abspath(__file__)))
EXE = os.path.abspath(sys.argv[1] if len(sys.argv) > 1 else os.path.join(RAIZ, "src-tauri", "target", "debug", "skia"))
SAIDA = os.path.join(RAIZ, "testes", "saida")
PDFS = os.path.join(RAIZ, "testes", "pdfs")
MAIN_PY = os.path.join(os.path.dirname(RAIZ), "Graphe", "cloudrun-pdftotext", "main.py")
sys.path.insert(0, os.path.dirname(os.path.abspath(__file__)))
import gerar_pdfs  # noqa: E402

falhas = []


def confere(cond, msg):
    print(("  ✓ " if cond else "  ✗ ") + msg)
    if not cond:
        falhas.append(msg)


def sha(p):
    return hashlib.sha256(open(p, "rb").read()).hexdigest()


def porta_livre():
    s = socket.socket()
    s.bind(("127.0.0.1", 0))
    p = s.getsockname()[1]
    s.close()
    return p


def servir(pasta):
    """Servidor HTTP local (só leitura) com o motor "publicado"."""
    porta = porta_livre()
    h = lambda *a, **k: http.server.SimpleHTTPRequestHandler(*a, directory=pasta, **k)
    srv = http.server.ThreadingHTTPServer(("127.0.0.1", porta), h)
    srv.RequestHandlerClass.log_message = lambda *a: None
    threading.Thread(target=srv.serve_forever, daemon=True).start()
    return srv, f"http://127.0.0.1:{porta}/"


def publicar_motor_teste(pasta, adulterar=False):
    """Um motor 'novo': o mesmo motor.js com uma linha a mais (sha256 novo), manifesto com data posterior, assinado."""
    os.makedirs(pasta, exist_ok=True)
    js = open(os.path.join(RAIZ, "motor", "motor.js"), encoding="utf-8").read() + "\n// republicado para o teste de ponta a ponta\n"
    open(os.path.join(pasta, "motor.js"), "w", encoding="utf-8").write(js)
    m = json.load(open(os.path.join(RAIZ, "motor", "manifesto.json"), encoding="utf-8"))
    m["sha256"] = hashlib.sha256(js.encode("utf-8")).hexdigest()
    m["bytes"] = len(js.encode("utf-8"))
    m["data"] = "2099-01-01T00:00:00Z"
    open(os.path.join(pasta, "manifesto.json"), "w", encoding="utf-8").write(json.dumps(m, indent=2, ensure_ascii=False) + "\n")
    r = subprocess.run(["node", os.path.join(RAIZ, "scripts", "assinar_motor.mjs"), pasta], capture_output=True, text=True)
    if r.returncode:
        raise SystemExit("assinatura do motor de teste falhou: " + r.stderr)
    if adulterar:   # a assinatura passa a ser a de OUTRO conteúdo
        m["data"] = "2099-01-02T00:00:00Z"
        open(os.path.join(pasta, "manifesto.json"), "w", encoding="utf-8").write(json.dumps(m, indent=2, ensure_ascii=False) + "\n")
    return m


def rodar_app(plano, url_motor, dados):
    caminho = os.path.join(SAIDA, "plano-" + uuid.uuid4().hex[:6] + ".json")
    json.dump(plano, open(caminho, "w"), ensure_ascii=False)
    env = dict(os.environ, SKIA_E2E=caminho, SKIA_DADOS=dados, SKIA_MOTOR_URL=url_motor)
    t0 = time.time()
    r = subprocess.run([EXE], env=env, capture_output=True, text=True, timeout=900)
    if r.returncode or not os.path.exists(plano["relatorio"]):
        raise SystemExit(f"o app saiu com {r.returncode} sem relatório:\n{r.stderr[-3000:]}")
    rel = json.load(open(plano["relatorio"], encoding="utf-8"))
    print(f"  (app rodou em {time.time() - t0:.1f} s)")
    return rel


def texto_pdf(p):
    return "\f".join(pg.get_text() for pg in pymupdf.open(p))


def norm(s):
    return " ".join(s.upper().split())


def servico_local():
    """O main.py do Graphé rodando aqui, com as ferramentas embutidas no Skiá à frente do PATH."""
    venv = os.path.join(RAIZ, ".ferramentas", "venv-servico")
    py = os.path.join(venv, "bin", "python")
    if not os.path.exists(py):
        subprocess.run([sys.executable, "-m", "venv", venv], check=True)
        subprocess.run([py, "-m", "pip", "install", "-q", "flask==3.0.3", "requests==2.32.3"], check=True)
    pasta_bin = os.path.dirname(EXE)
    porta = porta_livre()
    env = dict(os.environ, PORT=str(porta), PATH=pasta_bin + os.pathsep + os.environ["PATH"], PDFTOTEXT_SECRET="",
               TESSDATA_PREFIX=os.path.join(pasta_bin, "tessdata"), OCR_TEMPO_MAX_S="900", OCR_TIMEOUT_PAGINA_S="300",
               FONTCONFIG_FILE=os.path.join(SAIDA, "dados", "cache", "fonts.conf"))
    proc = subprocess.Popen([py, MAIN_PY], env=env, stdout=subprocess.DEVNULL, stderr=subprocess.DEVNULL, cwd=os.path.dirname(MAIN_PY))
    for _ in range(100):
        try:
            urllib.request.urlopen(f"http://127.0.0.1:{porta}/", timeout=1)
            return proc, f"http://127.0.0.1:{porta}"
        except Exception:
            time.sleep(0.2)
    proc.kill()
    raise SystemExit("o main.py não subiu")


def extrair_servico(url, pdf, fracas):
    """O pedido do Graphé: sem página fraca, só o pdftotext; com, o OCR só das páginas da dica (_pdfComOcr_)."""
    q = "" if not fracas else "?ocr=1&ocr_somente=1&ocr_paginas=" + ",".join(map(str, fracas))
    fronteira = uuid.uuid4().hex
    corpo = (f"--{fronteira}\r\nContent-Disposition: form-data; name=\"pdf\"; filename=\"a.pdf\"\r\n"
             f"Content-Type: application/pdf\r\n\r\n").encode() + open(pdf, "rb").read() + f"\r\n--{fronteira}--\r\n".encode()
    req = urllib.request.Request(url + "/extract" + q, data=corpo, headers={"Content-Type": "multipart/form-data; boundary=" + fronteira})
    return json.load(urllib.request.urlopen(req, timeout=900))


def main():
    if not os.path.exists(EXE):
        raise SystemExit("falta o executável: npx tauri build --debug --no-bundle")
    shutil.rmtree(SAIDA, ignore_errors=True)
    os.makedirs(SAIDA)
    gerar_pdfs.PASTA = PDFS
    gerar_pdfs.main()
    nomes = ["texto.pdf", "escaneado.pdf", "misto.pdf"]
    originais = {n: (sha(os.path.join(PDFS, n)), os.path.getmtime(os.path.join(PDFS, n))) for n in nomes}
    dados = os.path.join(SAIDA, "dados")
    pub = os.path.join(SAIDA, "publicado")
    novo = publicar_motor_teste(pub)
    srv, url = servir(pub)

    print("1. o app com os 3 PDFs (e a atualização assinada das regras no fim)")
    plano = {"arquivos": [os.path.join(PDFS, n) for n in nomes], "saida": os.path.join(SAIDA, "anonimizados"), "txt": True, "mapa": True,
             "extracao": os.path.join(SAIDA, "extracao"), "relatorio": os.path.join(SAIDA, "relatorio1.json"), "atualizar": True}
    rel = rodar_app(plano, url, dados)
    for it in rel["itens"]:
        print(f"  {it['nome']}: {it['etapa']} · {it.get('paginas')} pág. · OCR {it.get('e2e', {}).get('ocr', {}).get('paginas')} · "
              f"{(it.get('stats') or {}).get('pessoas')} pessoa(s) · {it.get('segundos')} s" + (f" · ERRO {it['erro']}" if it.get("erro") else ""))
    confere(all(it["etapa"] == "pronto" for it in rel["itens"]) and len(rel["itens"]) == 3, "os 3 arquivos ficaram prontos")

    print("2. a saída")
    pasta = plano["saida"]
    saidas = sorted(f for f in os.listdir(pasta) if f.lower().endswith(".pdf"))
    confere(saidas == sorted(nomes), f"nomes de saída iguais aos de entrada: {saidas}")
    proibidos = list(gerar_pdfs.PESSOAS.values()) + list(gerar_pdfs.IDENT.values()) + \
        ["PIRAMBOIA", "TESTEFULANO", "TESTEFULANA", "ROSIMEIRE", "ANTONIETA", "GERALDO", "GUIMARAES", "123456789", "998765432"]
    for n in nomes:
        p = os.path.join(pasta, n)
        doc = pymupdf.open(p)
        orig = pymupdf.open(os.path.join(PDFS, n))
        t = texto_pdf(p)
        txt = open(os.path.join(pasta, n[:-4] + ".txt"), encoding="utf-8").read()
        achados = [x for x in proibidos if norm(x) in norm(t) or norm(x) in norm(txt) or norm(x).replace(".", "").replace("-", "") in norm(t).replace(".", "").replace("-", "")]
        confere(not achados, f"{n}: nenhum dado de pessoa fictícia em claro" + (f" — ACHEI {achados}" if achados else ""))
        confere(doc.page_count == orig.page_count + 1, f"{n}: {doc.page_count} folhas = {orig.page_count} páginas dos autos + 1 do cabeçalho")
        confere("AUTOS ANONIMIZADOS — GRAPHE v" + rel["motor"] in doc[0].get_text(), f"{n}: cabeçalho de autos anonimizados com a versão do motor ({rel['motor']})")
        tamanhos = {round(s["size"], 1) for pg in doc for b in pg.get_text("dict")["blocks"] for l in b.get("lines", []) for s in l["spans"]}
        fontes = {s["font"] for pg in doc for b in pg.get_text("dict")["blocks"] for l in b.get("lines", []) for s in l["spans"]}
        confere(min(tamanhos) >= 11 and all("Mono" in f for f in fontes), f"{n}: fonte monoespaçada de 11 pt ({fontes}, {tamanhos})")
        for pagina in range(1, orig.page_count + 1):
            if f"PÁGINA {pagina} =====" not in doc[pagina].get_text():
                confere(False, f"{n}: a folha {pagina + 1} é a página {pagina} dos autos")
                break
        mapa = os.path.join(pasta, n[:-4] + " — MAPA (nomes reais).json")
        confere(os.path.exists(mapa) and "TESTEFULAN" in open(mapa, encoding="utf-8").read().upper(), f"{n}: mapa gravado (pedido no plano)")
        s, m = originais[n]
        confere(sha(os.path.join(PDFS, n)) == s and os.path.getmtime(os.path.join(PDFS, n)) == m, f"{n}: original intacto (sha256 e data)")
    confere(not os.listdir(os.path.join(dados, "cache", "tmp")), "pasta temporária vazia ao fim")

    print("3. a atualização das regras")
    a = rel.get("atualizacao") or {}
    confere(a.get("trocou") is True, f"motor republicado e assinado: baixado, testado e instalado ({a})")
    confere(novo["data"] in rel.get("motorFinal", ""), f"o motor em uso passou a ser o novo ({rel.get('motorFinal')})")
    publicar_motor_teste(pub, adulterar=True)
    plano2 = {"arquivos": [], "relatorio": os.path.join(SAIDA, "relatorio2.json"), "atualizar": True, "extracao": os.path.join(SAIDA, "extracao")}
    shutil.rmtree(os.path.join(dados, "dados", "motor"), ignore_errors=True)   # volta ao embutido
    rel2 = rodar_app(plano2, url, dados)
    a2 = rel2.get("atualizacao") or {}
    confere(a2.get("trocou") is False and "assinatura" in (a2.get("erro") or ""), f"manifesto adulterado recusado ({a2.get('erro')})")
    srv.shutdown()

    print("4. a extração do Skiá × o serviço do Graphé (main.py aqui, com as mesmas ferramentas)")
    if not os.path.exists(MAIN_PY):
        confere(False, "sem o main.py do Graphé em " + MAIN_PY)
    else:
        proc, base = servico_local()
        try:
            for it in rel["itens"]:
                fr = it["e2e"]["fracas"]
                js = extrair_servico(base, it["caminho"], fr)
                sk = open(os.path.join(plano["extracao"], it["nome"] + ".extracao.txt"), encoding="utf-8").read()
                igual = js["text"] == sk
                info = f"OCR do serviço {js.get('ocr', {}).get('paginas') if fr else '—'} × Skiá {it['e2e']['ocr'].get('paginas')}"
                confere(igual, f"{it['nome']}: texto idêntico ao do serviço ({len(sk)} caracteres; {info})")
                if not igual:
                    open(os.path.join(SAIDA, it["nome"] + ".servico.txt"), "w", encoding="utf-8").write(js["text"])
        finally:
            proc.kill()

    print()
    print("RESULTADO: " + ("tudo certo" if not falhas else f"{len(falhas)} falha(s)"))
    sys.exit(1 if falhas else 0)


if __name__ == "__main__":
    main()
