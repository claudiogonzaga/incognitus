#!/usr/bin/env python3
"""Incognitus — teste de ponta a ponta com os 3 PDFs FICTÍCIOS (texto, escaneado, misto).

1. Gera os PDFs (gerar_pdfs.py) e guarda o sha256 e a data de cada original.
2. Roda o APP DE VERDADE em modo de teste (SKIA_E2E=plano.json; dados e cache numa pasta à parte, SKIA_DADOS): fila,
   pdftotext e OCR pelos sidecars, motor no Web Worker, PDF de saída. O plano pede também a atualização das regras de
   um servidor local: um motor republicado e assinado com a chave do Incognitus (tem de ser instalado depois de passar nos
   casos de teste) e, numa segunda execução, um manifesto adulterado (tem de ser recusado).
3. Confere: nomes de saída iguais aos de entrada; nenhum dado de pessoa fictícia em claro na saída (PDF e .txt);
   originais intactos; o cabeçalho com a versão do motor; uma folha por página dos autos + a do cabeçalho; fonte
   monoespaçada de 11 pt; o mapa ao lado de cada PDF.
4. A reidentificação: a minuta da IA no card do caso (na primeira execução) e, na segunda, a aba Reidentificar, com o mapa que o app gravou: um texto colado, uma minuta .txt e uma .docx
   com um token partido em dois trechos de formatação, no corpo, no cabeçalho e no nome do arquivo. Os nomes voltam,
   nenhum token fica, o que está fora do mapa é apontado, as minutas não mudam e o resto do .docx sai igual.
5. Compara a extração do Incognitus com a do serviço do Graphé (cloudrun-pdftotext/main.py rodando aqui, com as MESMAS
   ferramentas embutidas no Incognitus e o mesmo pedido que o Graphé faz): tem de ser idêntica, byte a byte.
Uso: python3 testes/e2e.py [executável]   (padrão: src-tauri/target/debug/incognitus, de `npx tauri build --debug --no-bundle`)
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
import zipfile

import tempfile

import pymupdf

sys.stdout.reconfigure(encoding="utf-8", errors="replace", line_buffering=True)

RAIZ = os.path.dirname(os.path.dirname(os.path.abspath(__file__)))
EXE = os.path.abspath(sys.argv[1] if len(sys.argv) > 1 else os.path.join(RAIZ, "src-tauri", "target", "debug", "incognitus" + (".exe" if os.name == "nt" else "")))
CHAVE = os.path.join(RAIZ, "_segredo", "motor_ed25519.pem")   # no CI não há a chave: só o teste da recusa
# No disco interno (o externo já se desmontou no meio de um teste): SKIA_TESTE_DIR troca o lugar.
BASE = os.environ.get("SKIA_TESTE_DIR") or (os.path.expanduser("~/Library/Caches/incognitus-testes") if sys.platform == "darwin"
                                            else os.path.join(tempfile.gettempdir(), "incognitus-testes"))
SAIDA = os.path.join(BASE, "saida")
PDFS = os.path.join(BASE, "pdfs")
MAIN_PY = os.path.join(os.path.dirname(RAIZ), "Graphe", "cloudrun-pdftotext", "main.py")
sys.path.insert(0, os.path.dirname(os.path.abspath(__file__)))
import gerar_pdfs  # noqa: E402

falhas = []


def A(n):
    """O nome de saída (sem a extensão): o do original com _anon no fim."""
    return os.path.splitext(n)[0] + "_anon"


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
    http.server.SimpleHTTPRequestHandler.log_message = lambda *a: None
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
    if os.path.exists(CHAVE):
        r = subprocess.run(["node", os.path.join(RAIZ, "scripts", "assinar_motor.mjs"), pasta], capture_output=True, text=True)
        if r.returncode:
            raise SystemExit("assinatura do motor de teste falhou: " + r.stderr)
    else:   # sem a chave privada: uma assinatura qualquer (o app tem de recusar)
        open(os.path.join(pasta, "manifesto.json.sig"), "w").write("A" * 86 + "==\n")
    if adulterar:   # a assinatura passa a ser a de OUTRO conteúdo
        m["data"] = "2099-01-02T00:00:00Z"
        open(os.path.join(pasta, "manifesto.json"), "w", encoding="utf-8").write(json.dumps(m, indent=2, ensure_ascii=False) + "\n")
    return m


def rodar_app(plano, url_motor, dados):
    caminho = os.path.join(SAIDA, "plano-" + uuid.uuid4().hex[:6] + ".json")
    json.dump(plano, open(caminho, "w", encoding="utf-8"), ensure_ascii=False)
    env = dict(os.environ, SKIA_E2E=caminho, SKIA_DADOS=dados, SKIA_MOTOR_URL=url_motor,
               SKIA_APP_URL=url_motor + "release.json", SKIA_DOWNLOADS=os.path.join(SAIDA, "downloads"))
    os.makedirs(env["SKIA_DOWNLOADS"], exist_ok=True)
    t0 = time.time()
    r = subprocess.run([EXE], env=env, capture_output=True, timeout=900)
    log = r.stderr.decode("utf-8", "replace")
    open(plano["relatorio"] + ".log", "w", encoding="utf-8").write(log)
    if r.returncode or not os.path.exists(plano["relatorio"]):
        raise SystemExit(f"o app saiu com {r.returncode} sem relatório:\n{log[-3000:]}")
    rel = json.load(open(plano["relatorio"], encoding="utf-8"))
    print(f"  (app rodou em {time.time() - t0:.1f} s)")
    if any(it.get("erro") or it.get("avisos") for it in rel.get("itens", [])):
        print("  log do app:\n    " + "\n    ".join(log.strip().splitlines()[-30:]))
    return rel


def texto_pdf(p):
    return "\f".join(pg.get_text() for pg in pymupdf.open(p))


def norm(s):
    return " ".join(s.upper().split())


def servico_local():
    """O main.py do Graphé rodando aqui, com as ferramentas embutidas no Incognitus à frente do PATH."""
    venv = os.path.join(RAIZ, ".ferramentas", "venv-servico")
    py = os.path.join(venv, "Scripts", "python.exe") if os.name == "nt" else os.path.join(venv, "bin", "python")
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


W_DOC = ('<?xml version="1.0" encoding="UTF-8" standalone="yes"?>\n'
         '<w:document xmlns:w="http://schemas.openxmlformats.org/wordprocessingml/2006/main"><w:body>{}</w:body></w:document>')
W_HDR = ('<?xml version="1.0" encoding="UTF-8" standalone="yes"?>\n'
         '<w:hdr xmlns:w="http://schemas.openxmlformats.org/wordprocessingml/2006/main">{}</w:hdr>')
ESTILOS = '<?xml version="1.0" encoding="UTF-8"?>\n<w:styles xmlns:w="http://schemas.openxmlformats.org/wordprocessingml/2006/main"/>'


def minutas(mapa_path):
    """As minutas fictícias da IA, com os tokens do mapa que o app gravou."""
    mapa = json.load(open(mapa_path, encoding="utf-8"))["tokens"]
    pessoas = [t for t, v in mapa.items() if v.get("variantes") and v.get("tipo") != "ALIAS"]
    cpf = next((t for t, v in mapa.items() if v.get("tipo") == "CPF"), None)
    a, b = pessoas[0], pessoas[1]
    nu = lambda t: t.strip("«»")
    linhas = [f"MINUTA — {a}", f"O requerente {a} move ação contra <<{nu(b)}>>."] + ([f"CPF do requerente: {cpf}."] if cpf else [])
    texto = "\n".join(linhas) + "\n"
    pasta = os.path.join(SAIDA, "minutas")
    os.makedirs(pasta, exist_ok=True)
    txt = os.path.join(pasta, "minuta-texto.txt")
    open(txt, "w", encoding="utf-8").write(texto)
    k = len(a) // 2   # o token partido em dois trechos (o primeiro em negrito), como o Word às vezes grava
    corpo = (f'<w:p><w:r><w:t xml:space="preserve">MINUTA — </w:t></w:r><w:r><w:rPr><w:b/></w:rPr><w:t>{a[:k]}</w:t></w:r>'
             f'<w:r><w:t xml:space="preserve">{a[k:]}, requerente.</w:t></w:r></w:p>'
             f'<w:p><w:r><w:t xml:space="preserve">Contra {b}, e só.</w:t></w:r></w:p>')
    docx = os.path.join(pasta, f"Minuta {a}.docx")
    with zipfile.ZipFile(docx, "w", zipfile.ZIP_DEFLATED) as z:
        z.writestr("[Content_Types].xml", '<?xml version="1.0"?><Types xmlns="http://schemas.openxmlformats.org/package/2006/content-types"/>')
        z.writestr("word/document.xml", W_DOC.format(corpo))
        z.writestr("word/header1.xml", W_HDR.format(f"<w:p><w:r><w:t>Parte: {b}</w:t></w:r></w:p>"))
        z.writestr("word/styles.xml", ESTILOS)
    v = lambda t: mapa[t]["valor"]
    return {"texto": texto + "Citação solta: «Fulaninho».\n", "n_texto": 3 + (1 if cpf else 0), "txt": txt, "docx": docx,
            "tokens": [a, b] + ([cpf] if cpf else []), "valores": [v(a), v(b)] + ([v(cpf)] if cpf else []),
            "titulo_docx": f"Minuta {v(a)}", "originais": {p: sha(p) for p in (txt, docx)}}


def conferir_reid(rel2, mm):
    r = rel2.get("reid") or {}
    confere((r.get("tokens") or 0) >= 2, f"o mapa abriu na aba Reidentificar ({r.get('tokens')} tokens)")
    t = r.get("texto") or {}
    confere(t.get("n") == mm["n_texto"] and all(x in t.get("texto", "") for x in mm["valores"]) and
            not any(x in t.get("texto", "") for x in mm["tokens"]), f"texto colado: {t.get('n')} tokens trocados pelos nomes, nenhum token ficou")
    confere(t.get("sobras") == ["Fulaninho"], f"o «Fulaninho», fora do mapa, ficou no aviso ({t.get('sobras')})")
    arqs = {os.path.basename(x["caminho"]): x for x in r.get("arquivos") or []}
    for nome, p in (("txt", mm["txt"]), ("docx", mm["docx"])):
        it = arqs.get(os.path.basename(p)) or {}
        s = it.get("saida") or ""
        confere(it.get("etapa") == "pronto" and os.path.exists(s) and "(reidentificado)" in s,
                f".{nome}: cópia reidentificada ao lado da minuta ({os.path.basename(s) or it.get('erro')})")
        confere(not it.get("sobras"), f".{nome}: nenhuma sobra ({it.get('sobras')})")
        confere(sha(p) == mm["originais"][p], f".{nome}: a minuta original não mudou")
        if not os.path.exists(s):
            continue
        if nome == "txt":
            c = open(s, encoding="utf-8").read()
            confere(all(x in c for x in mm["valores"]) and not any(x in c for x in mm["tokens"]), ".txt: os nomes voltaram, nenhum token ficou")
        else:
            z = zipfile.ZipFile(s)
            doc, hdr = z.read("word/document.xml").decode("utf-8"), z.read("word/header1.xml").decode("utf-8")
            confere(mm["valores"][0] in doc and mm["valores"][1] in doc and not any(x.strip("«»") in doc for x in mm["tokens"][:2]),
                    ".docx: o token partido em dois trechos voltou inteiro, e o do segundo parágrafo também")
            confere(mm["valores"][1] in hdr, ".docx: o cabeçalho também foi reidentificado")
            confere(z.read("word/styles.xml").decode("utf-8") == ESTILOS and z.namelist() == zipfile.ZipFile(p).namelist(),
                    ".docx: as outras partes saíram iguais, na mesma ordem")
            confere(os.path.basename(s) == mm["titulo_docx"] + " (reidentificado).docx", f".docx: o nome do arquivo também ({os.path.basename(s)})")
            confere(it.get("n") == 4, f".docx: 4 trocas (corpo, cabeçalho e nome do arquivo) — {it.get('n')}")


def incognitus_aberto():
    """No Mac, um Incognitus instalado e ABERTO (mesma identidade do app de teste) atrapalha o teste: o worker do
    motor baixado não respondeu em 5 min (visto em 10/11/2026; fechado o app, passou em 18 s)."""
    if sys.platform != "darwin" or os.environ.get("SKIA_E2E_OUTRA_IDENTIDADE"):
        # SKIA_E2E_OUTRA_IDENTIDADE=1: o app de teste foi montado com outro identifier (então não divide nada com o instalado):
        #   npx tauri build --debug --no-bundle --config '{"identifier":"br.eco.consciencia.incognitus.teste"}'
        return False
    r = subprocess.run(["pgrep", "-f", "Incognitus.app/Contents/MacOS/"], capture_output=True, text=True)
    return r.returncode == 0 and r.stdout.strip() != ""


def secao_rigorosa(dados):
    print("6. a anonimização rigorosa (modelo de NER, com SKIA_NER_DIR apontando para o modelo já baixado)")
    ner_dir = os.environ.get("SKIA_NER_DIR", "")
    if not (ner_dir and os.path.exists(os.path.join(ner_dir, "model.onnx"))):
        print("  (pulei: sem o modelo em SKIA_NER_DIR)")
    else:
        pasta_r = os.path.join(BASE, "rigoroso")
        shutil.rmtree(pasta_r, ignore_errors=True)
        p = gerar_pdfs.gerar_rigoroso(pasta_r)
        saida_n, saida_r = os.path.join(pasta_r, "normal"), os.path.join(pasta_r, "rigorosa")
        relr = {}
        for modo, saida in (("normal", saida_n), ("rigorosa", saida_r)):
            relr[modo] = rodar_app({"arquivos": [p], "saida": saida, "txt": True, "rigoroso": modo == "rigorosa",
                                    "extracao": os.path.join(SAIDA, "extracao-" + modo),
                                    "relatorio": os.path.join(SAIDA, "relatorio-" + modo + ".json")}, "http://127.0.0.1:9/", dados)
        it = relr["rigorosa"]["itens"][0]
        print(f"  rigorosa: {it['etapa']} · {it.get('segundos')} s · {(it.get('stats') or {}).get('rigorosoAceitos')} a mais · avisos {it.get('avisos')}"
              + (f" · ERRO {it['erro']}" if it.get("erro") else ""))
        tn = texto_pdf(os.path.join(saida_n, "rigoroso_anon.pdf")) if os.path.exists(os.path.join(saida_n, "rigoroso_anon.pdf")) else ""
        tr = texto_pdf(os.path.join(saida_r, "rigoroso_anon.pdf")) if os.path.exists(os.path.join(saida_r, "rigoroso_anon.pdf")) else ""
        confere("Gertrudes" in tn, "sem a opção, as amigas citadas só pelo prenome ficam em claro (o motor não as conhece)")
        confere(it["etapa"] == "pronto" and tr and not any(x in tr for x in ("Gertrudes", "Leocádia", "Leocadia")),
                "com a anonimização rigorosa, o modelo as acha e elas saem tarjadas")
        confere(bool(tr) and "Palmeiras" not in tr, "o endereço sem rótulo também sai")
        confere(bool(tr) and "TESTEFULANO" not in tr.upper(), "o que o motor já tarjava continua tarjado")
        conf = os.path.join(saida_r, "NÃO ENVIAR À IA (nomes reais)", "rigoroso_anon — CONFIRA (nomes reais).txt")
        confere(os.path.exists(conf) and "Anonimização rigorosa" in open(conf, encoding="utf-8").read(), "a lista CONFIRA conta o que o modelo tarjou e o que não")
        cm = os.path.join(saida_r, "NÃO ENVIAR À IA (nomes reais)", "rigoroso_anon — MAPA (nomes reais).json")
        mr = json.load(open(cm, encoding="utf-8")) if os.path.exists(cm) else {"tokens": {}}
        confere(any(v.get("valor") == "Gertrudes" for v in mr["tokens"].values()), "os nomes achados pelo modelo entram no mapa (a reidentificação os devolve)")


def secao_casos(dados):
    print("7. vários documentos do mesmo caso (PDF, .docx e .txt) e um de outro caso: um mapa só, o nome do arquivo sem nomes")
    pasta_c = os.path.join(BASE, "caso")
    shutil.rmtree(pasta_c, ignore_errors=True)
    arqs = gerar_pdfs.gerar_caso(pasta_c)
    saida = os.path.join(pasta_c, "saida")
    rel = rodar_app({"arquivos": arqs, "saida": saida, "txt": True, "extracao": os.path.join(SAIDA, "extracao-caso"),
                     "relatorio": os.path.join(SAIDA, "relatorio-caso.json")}, "http://127.0.0.1:9/", dados)
    por = {it["nome"]: it for it in rel["itens"]}
    N = gerar_pdfs.CASO_NOMES
    for it in rel["itens"]:
        print(f"  {it['nome']}: {it['etapa']} · caso {(it.get('caso') or {}).get('chave')} ({(it.get('caso') or {}).get('n')}, {(it.get('caso') or {}).get('motivo')})"
              + (f" · ERRO {it['erro']}" if it.get("erro") else ""))
    confere(all(it["etapa"] == "pronto" for it in rel["itens"]) and len(rel["itens"]) == 4, "os 4 arquivos (2 PDFs, 1 .docx, 1 .txt) ficaram prontos")
    ca = [(por.get(N[k]) or {}).get("caso") or {} for k in ("a", "relatorio", "termo", "outro")]
    confere(ca[0].get("n") == 3 and ca[0].get("chave") == ca[1].get("chave") == ca[2].get("chave"), "os 3 documentos do caso foram juntados")
    confere(ca[3].get("n") == 1 and ca[3].get("chave") != ca[0].get("chave"), "o PDF do outro caso ficou sozinho")
    mapas = [json.load(open(os.path.join(saida, "NÃO ENVIAR À IA (nomes reais)", os.path.basename(por[N[k]]["saida"]["pdf"])[:-4] + " — MAPA (nomes reais).json"), encoding="utf-8"))
             for k in ("a", "relatorio", "termo")]
    confere(mapas[0]["tokens"] == mapas[1]["tokens"] == mapas[2]["tokens"], "um mapa só para o caso (o mesmo nos 3)")
    tok = [t for t, v in mapas[0]["tokens"].items() if v.get("valor") == "CLODOALDO TESTEFULANO SALAMANDRA"]
    tok = tok[0] if tok else "?"
    nomes_saida = {k: os.path.basename(por[N[k]]["saida"]["pdf"]) for k in N}
    confere(nomes_saida["relatorio"] == "relatorio - não juntar - " + tok + "_anon.pdf", f"o nome do arquivo sai sem o nome da pessoa: {nomes_saida['relatorio']}")
    confere(nomes_saida["a"] == N["a"][:-4] + "_anon.pdf" and nomes_saida["termo"] == "termo da companheira_anon.pdf", "os nomes sem pessoa ficam, com _anon no fim (e a saída do .txt é .pdf)")
    reais = ["CLODOALDO", "SALAMANDRA", "ROSIMEIRE", "TESTEBELTRANA", "GENOVEVO", "TESTECICRANO", "529.982", "111.444"]
    for k in ("a", "relatorio", "termo", "outro"):
        t = texto_pdf(por[N[k]]["saida"]["pdf"])
        achou = [x for x in reais if x in t.upper()]
        confere(not achou and (k == "outro" or tok in t), f"{nomes_saida[k]}: sem nomes reais" + (" e com o mesmo token da pessoa" if k != "outro" else "") + (f" — ACHEI {achou}" if achou else ""))
    confere(not any(x in " ".join(os.listdir(saida)).upper() for x in reais), "nenhum nome real nos nomes dos arquivos de saída")
    t_rel = texto_pdf(por[N["relatorio"]]["saida"]["pdf"])
    confere("DOCUMENTOS DESTE CASO (3" in t_rel, "o cabeçalho lista os documentos do caso")
    # Separar (outra rodada, noutra pasta): o termo sai do caso, e os dois casos são refeitos
    saida2 = os.path.join(pasta_c, "saida-separar")
    rel2 = rodar_app({"arquivos": arqs, "saida": saida2, "txt": False, "extracao": os.path.join(SAIDA, "extracao-caso2"),
                      "relatorio": os.path.join(SAIDA, "relatorio-caso2.json"), "separar": N["termo"]}, "http://127.0.0.1:9/", dados)
    dep = {it["nome"]: it for it in rel2.get("depoisDeSeparar") or []}
    ct, ca2 = (dep.get(N["termo"]) or {}).get("caso") or {}, (dep.get(N["a"]) or {}).get("caso") or {}
    confere(ct.get("n") == 1 and ca2.get("n") == 2 and all(i.get("etapa") == "pronto" for i in dep.values()),
            f"Separar tira o documento do caso e refaz os dois (termo: {ct.get('n')}, custódia: {ca2.get('n')})")


def main():
    if not os.path.exists(EXE):
        raise SystemExit("falta o executável: npx tauri build --debug --no-bundle")
    if os.environ.get("SKIA_E2E_SO") == "casos":   # só a seção 7
        os.makedirs(SAIDA, exist_ok=True)
        secao_casos(os.path.join(SAIDA, "dados-casos"))
        print("\nRESULTADO: " + ("tudo certo" if not falhas else f"{len(falhas)} falha(s)"))
        sys.exit(1 if falhas else 0)
    if os.environ.get("SKIA_E2E_SO") == "rigoroso":   # só a seção 6 (para iterar no modo rigoroso)
        os.makedirs(SAIDA, exist_ok=True)
        secao_rigorosa(os.path.join(SAIDA, "dados-rigoroso"))
        print("\nRESULTADO: " + ("tudo certo" if not falhas else f"{len(falhas)} falha(s)"))
        sys.exit(1 if falhas else 0)
    shutil.rmtree(SAIDA, ignore_errors=True)
    shutil.rmtree(PDFS, ignore_errors=True)
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
    plano = {"arquivos": [os.path.join(PDFS, n) for n in nomes], "saida": None, "txt": True,
             "extracao": os.path.join(SAIDA, "extracao"), "relatorio": os.path.join(SAIDA, "relatorio1.json"), "atualizar": True,
             "minutaNoCard": os.path.join(SAIDA, "minuta-card")}
    os.makedirs(plano["minutaNoCard"], exist_ok=True)
    rel = rodar_app(plano, url, dados)
    for it in rel["itens"]:
        print(f"  {it['nome']}: {it['etapa']} · {it.get('paginas')} pág. · OCR {it.get('e2e', {}).get('ocr', {}).get('paginas')} · "
              f"{(it.get('stats') or {}).get('pessoas')} pessoa(s) · {it.get('segundos')} s" + (f" · ERRO {it['erro']}" if it.get("erro") else ""))
        for a in it.get("avisos") or []:
            print("    aviso: " + a)
    confere(all(it["etapa"] == "pronto" for it in rel["itens"]) and len(rel["itens"]) == 3, "os 3 arquivos ficaram prontos")
    mc = rel.get("minutaNoCard") or {}
    confere(mc.get("bloco"), "o card pronto tem o bloco 'Minuta da IA' (Reidentificar minuta…)")
    confere(mc.get("etapa") == "pronto" and mc.get("n") == mc.get("tokens") and mc.get("tokens", 0) >= 1 and not mc.get("sobras"),
            f"a minuta da IA reidentificada no card, com o mapa do caso: {mc.get('n')} de {mc.get('tokens')} token(s), sobras {mc.get('sobras')} {mc.get('erro') or ''}")
    confere(bool(mc.get("texto")) and "«" not in mc["texto"] and os.path.basename(mc.get("saida") or "") == "minuta da IA (reidentificado).txt",
            "a cópia reidentificada sai ao lado da minuta, sem tokens")

    print("2. a saída")
    pasta = os.path.join(PDFS, "Incognitus — anonimizados")   # a saída padrão: ao lado dos originais
    priv = os.path.join(pasta, "NÃO ENVIAR À IA (nomes reais)")   # o mapa e a lista CONFIRA, à parte dos PDFs
    confere(os.path.isdir(pasta), "a pasta padrão de saída é 'Incognitus — anonimizados', ao lado dos originais")
    saidas = sorted(f for f in os.listdir(pasta) if f.lower().endswith(".pdf"))
    confere(saidas == sorted(A(n) + ".pdf" for n in nomes), f"nomes de saída = os de entrada com _anon no fim: {saidas}")
    proibidos = list(gerar_pdfs.PESSOAS.values()) + list(gerar_pdfs.IDENT.values()) + \
        ["PIRAMBOIA", "TESTEFULANO", "TESTEFULANA", "ROSIMEIRE", "ANTONIETA", "GERALDO", "GUIMARAES", "123456789", "998765432"] + \
        [gerar_pdfs.ASSINANTE, "HELENA FICTICIA", gerar_pdfs.USUARIO_PJE]   # o rodapé do PJe: quem assina e quem gerou o PDF
    for n in nomes:
        p = os.path.join(pasta, A(n) + ".pdf")
        doc = pymupdf.open(p)
        orig = pymupdf.open(os.path.join(PDFS, n))
        t = texto_pdf(p)
        txt = open(os.path.join(pasta, A(n) + ".txt"), encoding="utf-8").read()
        achados = [x for x in proibidos if norm(x) in norm(t) or norm(x) in norm(txt) or norm(x).replace(".", "").replace("-", "") in norm(t).replace(".", "").replace("-", "")]
        confere(not achados, f"{n}: nenhum dado de pessoa fictícia em claro" + (f" — ACHEI {achados}" if achados else ""))
        if n == "texto.pdf":
            confere("gerado pelo usuário [omitido]" in t and "Assinado eletronicamente por: [omitido] - 15/03/2026" in t and "Num. 1234567 - Pág. 1" in t,
                    f"{n}: o rodapé do PJe sai sem quem assina e sem quem gerou o PDF (data e Num. ficam)")
        confere(doc.page_count == orig.page_count + 1, f"{n}: {doc.page_count} folhas = {orig.page_count} páginas dos autos + 1 do cabeçalho")
        confere("AUTOS ANONIMIZADOS — GRAPHE v" + rel["motor"] in doc[0].get_text(), f"{n}: cabeçalho de autos anonimizados com a versão do motor ({rel['motor']})")
        tamanhos = {round(s["size"], 1) for pg in doc for b in pg.get_text("dict")["blocks"] for l in b.get("lines", []) for s in l["spans"]}
        fontes = {s["font"] for pg in doc for b in pg.get_text("dict")["blocks"] for l in b.get("lines", []) for s in l["spans"]}
        confere(min(tamanhos) >= 11 and all("Mono" in f for f in fontes), f"{n}: fonte monoespaçada de 11 pt ({fontes}, {tamanhos})")
        for pagina in range(1, orig.page_count + 1):
            if f"PÁGINA {pagina} =====" not in doc[pagina].get_text():
                confere(False, f"{n}: a folha {pagina + 1} é a página {pagina} dos autos")
                break
        mapa = os.path.join(priv, A(n) + " — MAPA (nomes reais).json")
        confere(os.path.exists(mapa) and "TESTEFULAN" in open(mapa, encoding="utf-8").read().upper(), f"{n}: mapa gravado na pasta NÃO ENVIAR À IA (sempre)")
        conf = os.path.join(priv, A(n) + " — CONFIRA (nomes reais).txt")
        confere(os.path.exists(conf) and "CONFIRA ANTES DE USAR" in open(conf, encoding="utf-8").read(), f"{n}: lista CONFIRA gravada junto do mapa")
        confere(not [f for f in os.listdir(pasta) if "MAPA" in f or "CONFIRA" in f], f"{n}: nada de nomes reais solto ao lado dos PDFs")
        s, m = originais[n]
        confere(sha(os.path.join(PDFS, n)) == s and os.path.getmtime(os.path.join(PDFS, n)) == m, f"{n}: original intacto (sha256 e data)")
    tmp = os.path.join(dados, "cache", "tmp")
    confere(not os.path.exists(tmp) or not os.listdir(tmp), "nenhum arquivo temporário sobrou")

    print("3. a atualização das regras")
    a = rel.get("atualizacao") or {}
    if os.path.exists(CHAVE):
        confere(a.get("trocou") is True, f"motor republicado e assinado: baixado, testado e instalado ({a})")
        confere(novo["data"] in rel.get("motorFinal", ""), f"o motor em uso passou a ser o novo ({rel.get('motorFinal')})")
    else:
        print("  (sem a chave privada aqui: a instalação de um motor assinado não é testada, só a recusa)")
        confere(a.get("trocou") is False and "assinatura" in (a.get("erro") or ""), f"assinatura inválida recusada ({a.get('erro')})")
    publicar_motor_teste(pub, adulterar=True)
    # uma "versão nova" do app servida aqui, como a release do GitHub: o app tem de achá-la e baixar o instalador certo
    instalador = "Incognitus-Windows.msi" if os.name == "nt" else "Incognitus-macOS.dmg"
    for n in ("Incognitus-Windows.msi", "Incognitus-macOS.dmg"):
        open(os.path.join(pub, n), "wb").write(b"instalador ficticio " + n.encode())
    json.dump({"tag_name": "v99.0.0", "body": "notas da versao ficticia", "html_url": url + "pagina", "draft": False, "prerelease": False,
               "assets": [{"name": n, "browser_download_url": url + n, "size": 30} for n in ("Incognitus-Windows.msi", "Incognitus-macOS.dmg")]},
              open(os.path.join(pub, "release.json"), "w", encoding="utf-8"))
    mm = minutas(os.path.join(priv, "texto_anon — MAPA (nomes reais).json"))
    plano2 = {"arquivos": [], "relatorio": os.path.join(SAIDA, "relatorio2.json"), "atualizar": True, "extracao": os.path.join(SAIDA, "extracao"),
              "reidentificar": {"mapa": os.path.join(priv, "texto_anon — MAPA (nomes reais).json"), "texto": mm["texto"], "arquivos": [mm["txt"], mm["docx"]]},
              "atualizacaoApp": True}
    shutil.rmtree(os.path.join(dados, "dados", "motor"), ignore_errors=True)   # volta ao embutido
    rel2 = rodar_app(plano2, url, dados)
    a2 = rel2.get("atualizacao") or {}
    confere(a2.get("trocou") is False and "assinatura" in (a2.get("erro") or ""), f"manifesto adulterado recusado ({a2.get('erro')})")
    srv.shutdown()

    print("4. a reidentificação, com o mapa que o app gravou")
    conferir_reid(rel2, mm)
    ap = rel2.get("app") or {}
    nova, baixado = ap.get("nova") or {}, ap.get("baixado") or ""
    confere(nova.get("versao") == "99.0.0" and nova.get("url", "").endswith(instalador),
            f"versão nova do app achada, com o instalador deste sistema ({nova.get('versao')} · {os.path.basename(nova.get('url', ''))} {ap.get('erro') or ''})")
    confere(os.path.isfile(baixado) and open(baixado, "rb").read() == b"instalador ficticio " + instalador.encode()
            and os.path.dirname(baixado) == os.path.join(SAIDA, "downloads"), f"o instalador baixou inteiro, na pasta de teste ({os.path.basename(baixado)})")

    print("5. a extração do Incognitus × o serviço do Graphé (main.py aqui, com as mesmas ferramentas)")
    if not os.path.exists(MAIN_PY):
        print("  (pulei: sem o main.py do Graphé em " + MAIN_PY + ")")
    else:
        proc, base = servico_local()
        try:
            for it in rel["itens"]:
                fr = it["e2e"]["fracas"]
                js = extrair_servico(base, it["caminho"], fr)
                sk = open(os.path.join(plano["extracao"], it["nome"] + ".extracao.txt"), encoding="utf-8").read()
                igual = js["text"] == sk
                info = f"OCR do serviço {js.get('ocr', {}).get('paginas') if fr else '—'} × Incognitus {it['e2e']['ocr'].get('paginas')}"
                confere(igual, f"{it['nome']}: texto idêntico ao do serviço ({len(sk)} caracteres; {info})")
                if not igual:
                    open(os.path.join(SAIDA, it["nome"] + ".servico.txt"), "w", encoding="utf-8").write(js["text"])
        finally:
            proc.kill()

    secao_rigorosa(dados)
    secao_casos(dados)

    print()
    print("RESULTADO: " + ("tudo certo" if not falhas else f"{len(falhas)} falha(s)"))
    sys.exit(1 if falhas else 0)


if __name__ == "__main__":
    if incognitus_aberto():
        sys.exit("e2e: feche o Incognitus instalado antes do teste (ele divide a identidade com o app de teste e o atrapalha)")
    main()
