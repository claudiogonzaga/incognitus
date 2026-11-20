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
import re
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


def anthropic_de_mentira():
    """Uma Anthropic de mentira para o 🧠: guarda cada pedido e responde em streaming (SSE), como a de verdade. A análise
    e a minuta citam os tokens que vieram nos autos (para a reidentificação ter o que devolver). Com a ferramenta
    ler_modelo no pedido (o Livro de Modelos), o primeiro pedido da análise devolve a consulta do modelo M1."""
    import re as _re
    pedidos = []

    class H(http.server.BaseHTTPRequestHandler):
        def log_message(self, *a):
            pass

        def do_POST(self):
            n = int(self.headers.get("content-length") or 0)
            corpo = self.rfile.read(n).decode("utf-8")
            pedidos.append({"cabecalhos": {k.lower(): v for k, v in self.headers.items()}, "corpo": json.loads(corpo)})
            p = pedidos[-1]["corpo"]
            msgs = p["messages"]
            autos = msgs[0]["content"][0]["text"]
            toks = [t for t in dict.fromkeys(_re.findall(r"«[A-Za-zÀ-ÿ][A-Za-zÀ-ÿ0-9_]{2,39}»", autos)) if t != "«PAPEL_nn»"][:3]
            pedidos_do_usuario = " ".join(b.get("text", "") for mm in msgs if mm["role"] == "user" and isinstance(mm["content"], list)
                                          for b in mm["content"] if b.get("type") == "text")
            inicio = {"type": "message_start", "message": {"id": "msg_x", "type": "message", "role": "assistant", "model": p["model"], "content": [],
                                                           "stop_reason": None, "usage": {"input_tokens": 1000, "output_tokens": 1}}}
            nomes_ferr = [f.get("name") for f in p.get("tools") or []]
            consulta = None
            if "buscar_modelos" in nomes_ferr and len(msgs) == 1:
                consulta = ("buscar_modelos", {"consulta": "denúncia por tráfico de drogas"})
            elif "buscar_modelos" in nomes_ferr and len(msgs) == 3:
                ult = msgs[-1]["content"][0].get("content") if isinstance(msgs[-1]["content"], list) else ""
                ach = _re.findall(r"^(P\d+) \|", str(ult), _re.M)
                consulta = ("ler_modelo", {"id": ach[0] if ach else "P0"})
            if consulta:
                evs = [inicio,
                       {"type": "content_block_start", "index": 0, "content_block": {"type": "tool_use", "id": "toolu_busca_" + str(len(msgs)), "name": consulta[0], "input": {}}},
                       {"type": "content_block_delta", "index": 0, "delta": {"type": "input_json_delta", "partial_json": json.dumps(consulta[1], ensure_ascii=False)}},
                       {"type": "content_block_stop", "index": 0},
                       {"type": "message_delta", "delta": {"stop_reason": "tool_use", "stop_sequence": None}, "usage": {"output_tokens": 30}},
                       {"type": "message_stop"}]
            elif p.get("tools") and len(msgs) == 1:
                evs = [inicio,
                       {"type": "content_block_start", "index": 0, "content_block": {"type": "tool_use", "id": "toolu_prova_1", "name": "ler_modelo", "input": {}}},
                       {"type": "content_block_delta", "index": 0, "delta": {"type": "input_json_delta", "partial_json": '{"id": "M1"}'}},
                       {"type": "content_block_stop", "index": 0},
                       {"type": "message_delta", "delta": {"stop_reason": "tool_use", "stop_sequence": None}, "usage": {"output_tokens": 30}},
                       {"type": "message_stop"}]
            else:
                parada = "end_turn"
                if "redija a minuta" not in pedidos_do_usuario:
                    texto = ("ANÁLISE (de mentira). Os autos tratam de " + ", ".join(toks) + ".\n\nPRÓXIMO ATO: Denúncia\nMINUTA: SEGUIR")
                    if "PROVA DA TESOURA" in str(p.get("system") or ""):
                        texto = TESOURA(toks)
                    if "PROVA DO CORTE" in str(p.get("system") or ""):
                        # a resposta cortada no limite, no meio de uma palavra; a continuação traz o resto
                        corte = texto.index("Denúncia") + 3
                        if "Continue EXATAMENTE" in pedidos_do_usuario:
                            texto = texto[corte:]
                        else:
                            texto, parada = texto[:corte], "max_tokens"
                else:
                    texto = "EXCELENTÍSSIMO SENHOR JUIZ (minuta de mentira)\n\nO Ministério Público denuncia " + " e ".join(toks) + ".\n"
                evs = [inicio,
                       {"type": "content_block_start", "index": 0, "content_block": {"type": "text", "text": ""}},
                       {"type": "content_block_delta", "index": 0, "delta": {"type": "text_delta", "text": texto}},
                       {"type": "content_block_stop", "index": 0},
                       {"type": "message_delta", "delta": {"stop_reason": parada, "stop_sequence": None}, "usage": {"output_tokens": 200}},
                       {"type": "message_stop"}]
            sse = "".join(f"event: {ev['type']}\ndata: {json.dumps(ev, ensure_ascii=False)}\n\n" for ev in evs).encode("utf-8")
            self.send_response(200)
            self.send_header("content-type", "text/event-stream")
            self.send_header("content-length", str(len(sse)))
            self.end_headers()
            self.wfile.write(sse)

    porta = porta_livre()
    srv = http.server.ThreadingHTTPServer(("127.0.0.1", porta), H)
    threading.Thread(target=srv.serve_forever, daemon=True).start()
    return srv, f"http://127.0.0.1:{porta}/v1/messages", pedidos


CHAVE_DE_PROVA = "-".join(["sk", "ant", "prova", "e2e", "0000000000000000"])


def TESOURA(toks):
    """A resposta do MPA mestre: a análise, a linha da tesoura, a peça e, no fim, as duas linhas de controle."""
    return ("ANÁLISE (de mentira). Copie só a peça, que começa depois da linha da tesoura. Os autos tratam de " + ", ".join(toks) + ".\n\n"
            "**✂️ -------------------------- INÍCIO DA MINUTA -------------------------- ✂️**\n\n"
            "EXCELENTÍSSIMO SENHOR JUIZ (minuta de mentira, na mesma resposta)\n\nO Ministério Público denuncia " + " e ".join(toks) + ".\n\n"
            "PRÓXIMO ATO: Denúncia\nMINUTA: SEGUIR")


def openai_de_mentira():
    """Uma OpenAI de mentira (chat completions em streaming): devolve a análise com a peça depois da tesoura."""
    import re as _re
    pedidos = []

    class H(http.server.BaseHTTPRequestHandler):
        def log_message(self, *a):
            pass

        def do_POST(self):
            n = int(self.headers.get("content-length") or 0)
            corpo = json.loads(self.rfile.read(n).decode("utf-8"))
            pedidos.append({"caminho": self.path, "cabecalhos": {k.lower(): v for k, v in self.headers.items()}, "corpo": corpo})
            autos = " ".join(m.get("content") or "" for m in corpo["messages"] if m["role"] == "user")
            toks = [x for x in dict.fromkeys(_re.findall(r"«[A-Za-zÀ-ÿ][A-Za-zÀ-ÿ0-9_]{2,39}»", autos)) if x != "«PAPEL_nn»"][:3]
            texto = TESOURA(toks)
            evs = [{"model": corpo["model"], "choices": [{"index": 0, "delta": {"role": "assistant", "content": texto[:40]}}]},
                   {"choices": [{"index": 0, "delta": {"content": texto[40:]}}]},
                   {"choices": [{"index": 0, "delta": {}, "finish_reason": "stop"}]},
                   {"choices": [], "usage": {"prompt_tokens": 900, "completion_tokens": 120}}]
            sse = ("".join("data: " + json.dumps(e, ensure_ascii=False) + "\n\n" for e in evs) + "data: [DONE]\n\n").encode("utf-8")
            self.send_response(200)
            self.send_header("content-type", "text/event-stream")
            self.send_header("content-length", str(len(sse)))
            self.end_headers()
            self.wfile.write(sse)

    porta = porta_livre()
    srv = http.server.ThreadingHTTPServer(("127.0.0.1", porta), H)
    threading.Thread(target=srv.serve_forever, daemon=True).start()
    return srv, f"http://127.0.0.1:{porta}/v1", pedidos


def drive_de_mentira():
    """Um Google Drive de mentira para "Abrir no Google Docs": cria o documento no POST e diz que ele existe no GET."""
    pedidos = []

    class H(http.server.BaseHTTPRequestHandler):
        def log_message(self, *a):
            pass

        def _resp(self, corpo):
            b = json.dumps(corpo).encode("utf-8")
            self.send_response(200)
            self.send_header("content-type", "application/json")
            self.send_header("content-length", str(len(b)))
            self.end_headers()
            self.wfile.write(b)

        def do_POST(self):
            n = int(self.headers.get("content-length") or 0)
            pedidos.append({"metodo": "POST", "caminho": self.path, "cabecalhos": {k.lower(): v for k, v in self.headers.items()}, "corpo": self.rfile.read(n)})
            self._resp({"id": "docDeMentira00001"})

        def do_GET(self):
            pedidos.append({"metodo": "GET", "caminho": self.path, "cabecalhos": {k.lower(): v for k, v in self.headers.items()}, "corpo": b""})
            self._resp({"id": "docDeMentira00001", "trashed": False})

    porta = porta_livre()
    srv = http.server.ThreadingHTTPServer(("127.0.0.1", porta), H)
    threading.Thread(target=srv.serve_forever, daemon=True).start()
    return srv, f"http://127.0.0.1:{porta}", pedidos


def rodar_app(plano, url_motor, dados, extra_env=None):
    caminho = os.path.join(SAIDA, "plano-" + uuid.uuid4().hex[:6] + ".json")
    json.dump(plano, open(caminho, "w", encoding="utf-8"), ensure_ascii=False)
    env = dict(os.environ, SKIA_E2E=caminho, SKIA_DADOS=dados, SKIA_MOTOR_URL=url_motor,
               SKIA_APP_URL=url_motor + "release.json", SKIA_DOWNLOADS=os.path.join(SAIDA, "downloads"), **(extra_env or {}))
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
    # o 🧠 roda neste caso, com uma Anthropic de mentira e um prompt em arquivo (fictício)
    srv_ia, url_ia, pedidos = anthropic_de_mentira()
    prompt = os.path.join(pasta_c, "prompt de prova.md")
    open(prompt, "w", encoding="utf-8").write("# Prompt de prova\nVocê é o assessor jurídico fictício do teste de ponta a ponta.\n")
    # o Guia (arquivo), o RPP (escrito no app) e o Livro de Modelos (arquivo com os próprios modelos, um por título)
    guia = os.path.join(pasta_c, "guia de prova.md")
    open(guia, "w", encoding="utf-8").write("# Guia de Formatação de prova\nFonte fictícia, corpo 12, margens fictícias da casa.\n")
    livro = os.path.join(pasta_c, "livro de prova.md")
    open(livro, "w", encoding="utf-8").write(
        "# Denúncia — modelo fictício de prova\n" + "EXCELENTÍSSIMO SENHOR JUIZ. O Ministério Público oferece DENÚNCIA contra [NOME]. " * 3 + "\n\n"
        "# Arquivamento — modelo fictício de prova\n" + "Promoção de arquivamento fictícia, pelos fundamentos de prova. " * 3 + "\n\n"
        "# Curto demais\nnão vira modelo\n")
    srv_drive, url_drive, pedidos_drive = drive_de_mentira()
    rel = rodar_app({"arquivos": arqs, "saida": saida, "txt": True, "extracao": os.path.join(SAIDA, "extracao-caso"),
                     "relatorio": os.path.join(SAIDA, "relatorio-caso.json"),
                     "mpa": {"prompt": prompt, "vazar": os.path.join(pasta_c, "prompt que vaza.md"), "visor": True, "corte": True, "aprender": True, "conversa": True, "gdocs": True,
                             "livro": {"guia": guia, "livro": livro}}},
                    "http://127.0.0.1:9/", dados, {"SKIA_CLAUDE_URL": url_ia, "SKIA_CHAVE_DE_PROVA": CHAVE_DE_PROVA, "SKIA_DRIVE_URL": url_drive})
    srv_ia.shutdown()
    srv_drive.shutdown()
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
    # o 🧠 Melhor Próximo Ato e o 👁
    mp = rel.get("mpa") or {}
    print(f"  🧠: {mp.get('etapa')} · {mp.get('proximo')} · {len(pedidos)} pedido(s) à Anthropic de mentira" + (f" · ERRO {mp.get('erro')}" if mp.get("erro") else ""))
    confere(mp.get("botao") and (mp.get("olho") or 0) >= 4, f"o card tem o 🧠 e os 👁 (anonimizado e original): {mp.get('olho')} botão(ões) 👁")
    confere(mp.get("visor") == "ok", f"o 👁 abre o PDF anonimizado numa janela do app ({mp.get('visor')})")
    confere(mp.get("etapa") == "pronto" and mp.get("proximo") == "Denúncia" and not mp.get("suspensa"), "o 🧠 fez a análise e a minuta (MINUTA: SEGUIR)")
    ped = pedidos[:2]   # o 🧠 só com o prompt; depois vêm os do Livro (3) e os do repasse do porteiro (2)
    confere(len(ped) == 2 and len(ped[1]["corpo"]["messages"]) == 3 and ped[1]["corpo"]["messages"][1]["role"] == "assistant",
            f"duas chamadas, como no Graphé: a análise e, na mesma conversa, a minuta ({len(ped)} pedido(s))")
    tudo = json.dumps([p["corpo"] for p in ped], ensure_ascii=False).upper()
    confere(bool(ped) and all(p["corpo"].get("system", "").startswith("# Prompt de prova") for p in ped), "o prompt do arquivo vai como as instruções (system)")
    vaz = [x for x in reais if x in tudo]
    confere(bool(ped) and tok.upper() in tudo and not vaz, "à Anthropic vão só os autos anonimizados: o token, nenhum nome real" + (f" — ACHEI {vaz}" if vaz else ""))
    confere(bool(ped) and all(p["cabecalhos"].get("x-api-key") == CHAVE_DE_PROVA and p["cabecalhos"].get("anthropic-version") for p in ped),
            "a chave vai no cabeçalho (posta pelo Rust), com a versão da API")
    ta, tr = mp.get("textoAnon") or "", mp.get("textoReal") or ""
    confere((mp.get("anonimo") or "").endswith("_anon.md") and os.path.dirname(mp.get("anonimo") or "") == saida and tok in ta
            and not [x for x in reais if x in ta.upper()], "a resposta anonimizada (_anon.md) fica ao lado do PDF, só com tokens")
    sobrou = [t for t in mapas[0]["tokens"] if t in tr]
    voltou = [v["valor"] for v in mapas[0]["tokens"].values() if v.get("valor") and v["valor"] in tr]
    confere("NÃO ENVIAR À IA" in (mp.get("real") or "") and not sobrou and voltou and "INÍCIO DA MINUTA" in tr,
            f"a análise e a minuta com os nomes reais ficam na subpasta privada, sem tokens ({len(voltou)} dado(s) de volta)" + (f" — sobrou {sobrou}" if sobrou else ""))
    # o 🧠 com o Guia, o RPP e o Livro de Modelos
    lv, pl = mp.get("livro") or {}, pedidos[2:5]
    print(f"  🧠 com o Livro: {lv.get('etapa')} · modelos {lv.get('modelos')} · anexos {lv.get('anexos')} · {len(pl)} pedido(s)"
          + (f" · ERRO {lv.get('erro')}" if lv.get("erro") else "") + (f" · avisos {lv.get('avisos')}" if lv.get("avisos") else ""))
    confere(lv.get("etapa") == "pronto" and lv.get("catalogo") == 2 and lv.get("modelos") == ["M1 — Denúncia — modelo fictício de prova"],
            f"o Livro em arquivo vira o catálogo (2 modelos; o curto fica de fora) e a IA lê o M1 pela ferramenta ({lv.get('modelos')})")
    s0 = (pl[0]["corpo"].get("system") or "") if pl else ""
    confere(len(pl) == 3 and [t["name"] for t in pl[0]["corpo"].get("tools", [])] == ["ler_modelo"]
            and s0.startswith("# Prompt de prova") and "GUIA DE FORMATAÇÃO" in s0 and "Fonte fictícia, corpo 12" in s0
            and "RPP — REGRAS" in s0 and "M1 | Denúncia — modelo fictício de prova" in s0 and "CONTEXTO DESTA EXECUÇÃO" in s0,
            "o system leva o prompt, o Guia, o RPP e o catálogo do Livro, e o pedido leva a ferramenta ler_modelo")
    res = [b for b in (pl[1]["corpo"]["messages"][-1]["content"] if len(pl) > 1 else []) if b.get("type") == "tool_result"]
    confere(len(res) == 1 and res[0]["tool_use_id"] == "toolu_prova_1" and "TEXTO DO MODELO" in res[0]["content"] and "oferece DENÚNCIA" in res[0]["content"],
            "a consulta volta à IA com o texto do modelo (tool_result), na mesma etapa")
    tudo_l = json.dumps([x["corpo"] for x in pl], ensure_ascii=False)
    confere(bool(lv.get("pessoa")) and lv["pessoa"] not in tudo_l and lv.get("token") in s0,
            "o nome do caso que o RPP cita vai como o token, não em claro" + (f" ({lv.get('token')})" if lv.get("token") else ""))
    confere(len(pl) == 3 and len(pl[2]["corpo"]["messages"]) == 5 and "redija a minuta" in json.dumps(pl[2]["corpo"]["messages"][-1], ensure_ascii=False),
            "a minuta segue a mesma conversa, com a consulta e a análise")
    # o repasse do porteiro: o nome que ficou em claro nos autos sai trocado pelo token, sem pergunta
    rp, pr = mp.get("repasse") or {}, pedidos[5:7]
    tudo_r = json.dumps([x["corpo"] for x in pr], ensure_ascii=False)
    print(f"  repasse do porteiro: {rp.get('etapa')} · {rp.get('tarjados')} trecho(s) · perguntou {rp.get('perguntou')} · {len(pr)} pedido(s)"
          + (f" · ERRO {rp.get('erro')}" if rp.get("erro") else ""))
    confere(rp.get("etapa") == "pronto" and rp.get("tarjados") == 1 and not rp.get("perguntou") and len(pr) == 2
            and any("tarjou 1 trecho" in a for a in (rp.get("avisos") or [])),
            f"o porteiro troca sozinho pelo token o nome que ficou em claro nos autos, e o 🧠 segue sem pergunta ({rp.get('avisos')})")
    confere(bool(rp.get("pessoa")) and rp["pessoa"] not in tudo_r and rp["pessoa"].split(" ")[-1] not in tudo_r
            and ("não pegou: " + (rp.get("token") or "?") + ".") in tudo_r,
            "nos dois pedidos, o trecho vai com o token no lugar do nome (que estava quebrado em linhas)")
    rc = mp.get("repasseCaso") or {}
    confere(rc.get("total") == 2 and rc.get("porDoc") == [2, 0] and rc.get("pagina") and rc.get("txt"),
            f"na anonimização, o repasse tarja a página e o .txt antes de gravar ({rc})")
    vz = mp.get("vazar") or {}
    confere(vz.get("achou") and (vz.get("perguntou") or [0])[0] >= 1 and vz.get("etapa") == "erro" and "cancelado" in (vz.get("erro") or "")
            and len(pedidos) == 14,   # 10 até a resposta cortada, mais os 4 da conversa (análise, minuta, complemento e pergunta)
            f"sem o repasse, o porteiro pergunta antes de mandar o nome em claro, e com Cancelar nada sai ({vz.get('perguntou')} · {vz.get('erro')})")
    # o aprendizado
    ap = mp.get("aprender") or {}
    confere(ap.get("ensinados") == 1 and ap.get("claros") == 1 and ap.get("pagina") and ap.get("mapa") and ap.get("volta") == "Cite Zebedeu.",
            f"o aprendizado: o dado ensinado vira «DADO_01» (e volta na reidentificação); o que não é pessoal fica em claro e sai do mapa ({ap})")
    confere(ap.get("porteiro") and ap.get("total") == 1 and ap.get("botao"),
            "o que o porteiro tarja entra sozinho na tabela do aprendizado; o card tem o Ensinar… e o topo, o Aprendizado e o Fale com o desenvolvedor")
    # a resposta cortada no limite de tamanho: o app pede a continuação e cola os pedaços
    ct, pc = mp.get("corte") or {}, pedidos[7:10]
    print(f"  resposta cortada: {ct.get('etapa')} · {ct.get('proximo')} · {len(pc)} pedido(s)" + (f" · ERRO {ct.get('erro')}" if ct.get("erro") else ""))
    confere(ct.get("etapa") == "pronto" and ct.get("proximo") == "Denúncia" and ct.get("minuta") and "PRÓXIMO ATO: Denúncia" in (ct.get("analise") or ""),
            "a análise cortada no limite continua de onde parou, e os pedaços se colam sem emenda (PRÓXIMO ATO: Denúncia)")
    confere(len(pc) == 3 and all(x["corpo"].get("max_tokens") == 128000 for x in pc) and len(pc[1]["corpo"]["messages"]) == 3
            and "Continue EXATAMENTE" in json.dumps(pc[1]["corpo"]["messages"][-1], ensure_ascii=False) and len(pc[2]["corpo"]["messages"]) == 5,
            "o teto da resposta é de 128 mil tokens; a continuação e a minuta seguem na mesma conversa")
    # a janela do 🧠: quem usa responde na mesma conversa (complementa a minuta e pergunta), e a minuta vira .docx
    cv, pv = mp.get("conversa") or {}, pedidos[-2:]
    ult = lambda x: json.dumps(x["corpo"]["messages"][-1], ensure_ascii=False)
    print(f"  conversa: {cv.get('etapa')} · mensagens {cv.get('antes')} → {cv.get('depois')} → {cv.get('fim')} · janela {cv.get('janela')} itens · editor {cv.get('editor')} caracteres"
          + (f" · ERRO {cv.get('erro') or cv.get('erroDocx') or cv.get('janelaErro')}" if cv.get("erro") or cv.get("erroDocx") or cv.get("janelaErro") else ""))
    confere(cv.get("etapa") == "pronto" and cv.get("antes") == 4 and cv.get("depois") == 6 and cv.get("fim") == 8 and cv.get("minuta"),
            "a resposta de quem usa segue a mesma conversa: a análise, a minuta, o complemento e a pergunta")
    confere(len(pv) == 2 and "Reescreva a peça inteira" in ult(pv[0]) and bool(cv.get("token")) and cv["token"] in ult(pv[0])
            and cv.get("pessoa") not in json.dumps(pv[0]["corpo"], ensure_ascii=False) and len(pv[0]["corpo"]["messages"]) == 5,
            "o complemento vai com o nome do caso trocado pelo token" + (f" ({cv.get('token')})" if cv.get("token") else ""))
    confere(len(pv) == 2 and "Quem pediu a análise pergunta" in ult(pv[1]) and "Qual é o prazo?" in ult(pv[1]) and len(pv[1]["corpo"]["messages"]) == 7,
            "a pergunta também, sem mexer na análise nem na minuta")
    dx = cv.get("docx") or ""
    ok_docx = False
    if dx and os.path.isfile(dx):
        with zipfile.ZipFile(dx) as z:
            xml = z.read("word/document.xml").decode("utf-8")
            ok_docx = "minuta de mentira" in xml and z.namelist()[0] == "[Content_Types].xml" and "«" not in xml
    confere(ok_docx and "NÃO ENVIAR" in dx and dx.endswith(" — minuta.docx") and cv.get("paragrafos", 0) >= 2,
            f"a minuta sai em .docx, com os nomes reais, na subpasta que não vai à IA ({os.path.basename(dx)})")
    # a minuta no Google Docs: o .docx sobe uma vez só; o segundo clique abre o que já está lá
    gd = cv.get("gdocs") or {}
    sobe = [x for x in pedidos_drive if x["metodo"] == "POST"]
    confere(not gd.get("erro") and (gd.get("um") or {}).get("feito") == "criado" and (gd.get("dois") or {}).get("feito") == "aberto" and gd.get("botao")
            and (gd.get("um") or {}).get("link") == "https://docs.google.com/document/d/docDeMentira00001/edit" and len(sobe) == 1
            and sobe[0]["caminho"].startswith("/upload/drive/v3/files?uploadType=multipart") and b"PK\x03\x04" in sobe[0]["corpo"]
            and b"application/vnd.google-apps.document" in sobe[0]["corpo"] and "minuta" in sobe[0]["corpo"][:400].decode("utf-8", "replace"),
            f"Abrir no Google Docs: o .docx sobe uma vez, vira documento do Google, e o segundo clique só abre o de lá ({gd.get('erro') or len(pedidos_drive)})")
    its = cv.get("itens") or []
    confere(cv.get("quill") and not cv.get("janelaErro") and (cv.get("janela") or 0) >= 8 and cv.get("painel") and (cv.get("editor") or 0) > 20 and cv.get("editado")
            and cv.get("botao") == "Complementar a minuta ➤" and its.count("fala:eu") == 2 and its.count("fala:ia") == 4 and "passo:" in its,
            "a janela do 🧠 desenha os passos e as bolhas, o painel do aprendizado abre, e o editor carrega a minuta e guarda a edição")
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


def secao_tesoura(dados):
    print("8. a minuta numa resposta só (a linha da tesoura), pela Anthropic e por outra IA (a OpenAI de mentira)")
    pasta_t = os.path.join(BASE, "tesoura")
    shutil.rmtree(pasta_t, ignore_errors=True)
    os.makedirs(pasta_t)
    pdf = os.path.join(pasta_t, "outro caso.pdf")
    open(pdf, "wb").write(gerar_pdfs.pdf_texto([gerar_pdfs.CASO_OUTRO.splitlines()]))
    prompt = os.path.join(pasta_t, "prompt da tesoura.md")
    open(prompt, "w", encoding="utf-8").write("# Prompt de prova\nPROVA DA TESOURA: a análise e a peça na mesma resposta.\n")
    for ia in ("anthropic", "openai"):
        if ia == "anthropic":
            srv_ia, url_ia, pedidos = anthropic_de_mentira()
            env = {"SKIA_CLAUDE_URL": url_ia, "SKIA_CHAVE_DE_PROVA": CHAVE_DE_PROVA}
        else:
            srv_ia, url_ia, pedidos = openai_de_mentira()
            env = {"SKIA_OUTRA_IA_URL": url_ia, "SKIA_CHAVE_DE_PROVA": CHAVE_DE_PROVA}
        mpa = {"prompt": prompt}
        if ia == "openai":
            mpa["ia"] = {"provedor": "openai", "modelo": "gpt-5", "esforco": "medium"}
        rel = rodar_app({"arquivos": [pdf], "saida": os.path.join(pasta_t, "saida-" + ia), "txt": False,
                         "extracao": os.path.join(SAIDA, "extracao-tesoura-" + ia), "relatorio": os.path.join(SAIDA, "relatorio-tesoura-" + ia + ".json"),
                         "mpa": mpa}, "http://127.0.0.1:9/", dados, env)
        srv_ia.shutdown()
        mp = rel.get("mpa") or {}
        tr, ta = mp.get("textoReal") or "", mp.get("textoAnon") or ""
        print(f"  {ia}: {mp.get('etapa')} · {mp.get('proximo')} · {len(pedidos)} pedido(s)" + (f" · ERRO {mp.get('erro')}" if mp.get("erro") else ""))
        confere(mp.get("etapa") == "pronto" and mp.get("proximo") == "Denúncia" and len(pedidos) == 1,
                f"{ia}: a análise e a minuta num pedido só ({len(pedidos)} pedido(s); antes eram dois)")
        confere(tr.count("INÍCIO DA MINUTA") == 1 and tr.count("EXCELENTÍSSIMO") == 1 and ta.count("EXCELENTÍSSIMO") == 1
                and tr.index("PRÓXIMO ATO") < tr.index("INÍCIO DA MINUTA") and "na mesma resposta" in tr.split("INÍCIO DA MINUTA")[1],
                f"{ia}: a peça vem uma vez, depois da linha da tesoura; as linhas de controle ficam antes dela")
        if ia == "openai" and pedidos:
            c = pedidos[0]["corpo"]
            confere(pedidos[0]["caminho"].endswith("/chat/completions") and pedidos[0]["cabecalhos"].get("authorization") == "Bearer " + CHAVE_DE_PROVA
                    and c.get("model") == "gpt-5" and c.get("reasoning_effort") == "medium" and c.get("stream") is True
                    and c["messages"][0]["role"] == "system" and c["messages"][0]["content"].startswith("# Prompt de prova")
                    and "x-api-key" not in pedidos[0]["cabecalhos"],
                    "openai: o pedido sai no formato da OpenAI (system, esforço médio, streaming), com a chave no Bearer")
            reais = ["GENOVEVO", "TESTECICRANO", "111.444"]
            vaz = [x for x in reais if x in json.dumps(c, ensure_ascii=False).upper()]
            confere(not vaz, "openai: só os autos anonimizados saem (nenhum nome real)" + (f" — ACHEI {vaz}" if vaz else ""))


CONTEXTO_FICTICIO = """PODER JUDICIÁRIO DO ESTADO DE MATO GROSSO
COMARCA DE VALE FICTÍCIO DO NORTE
VARA ÚNICA
Processo nº 1000777-22.2026.8.11.0099
A Promotoria de Justiça de Vale Fictício do Norte/MT oferece denúncia contra
GENOVEVO TESTECICRANO DA SILVA, CPF 111.444.777-35, natural de Serra Inventada-MT,
residente na Fazenda Boa Esperança II, zona rural do Município de Vale Fictício do Norte.
Consta que, no dia 12/03/2026, o denunciado, que é o único tabelião do município,
de 8 mil habitantes, e filho do prefeito, esteve no Supermercado Bom Preço
e depois no Sítio Recanto Feliz.
A vítima é dona do Bar do Tonhão e sócia da empresa Agro Fictícia Comércio de Grãos Ltda.
O Secretário Municipal de Saúde e uma vereadora foram ouvidos.
A Fazenda Pública não se manifestou. A SSP/MT foi oficiada.
O sorriso do denunciado foi notado por todos.
Marcos Antônio Ficticiano, Juiz de Direito, recebeu a denúncia em 15/03/2026.
Carlos Eduardo Inventado, Promotor de Justiça, subscreve a peça.
Ana Paula Imaginária, Delegada de Polícia, presidiu o inquérito.
Advogado: José Roberto Fabulado, OAB/MT 99.999.
"""


def secao_contexto(dados):
    print("10. o contexto (município, comarca, cargo, empresa, fazenda, número do processo), o alerta de contexto e o mapa cifrado")
    pasta_x = os.path.join(BASE, "contexto")
    shutil.rmtree(pasta_x, ignore_errors=True)
    os.makedirs(pasta_x)
    pdf = os.path.join(pasta_x, "caso do contexto.pdf")
    open(pdf, "wb").write(gerar_pdfs.pdf_texto([CONTEXTO_FICTICIO.splitlines()]))
    saida = os.path.join(pasta_x, "saida")
    rel = rodar_app({"arquivos": [pdf], "saida": saida, "txt": True, "contexto": True, "cifrar": True,
                     "extracao": os.path.join(SAIDA, "extracao-contexto"), "relatorio": os.path.join(SAIDA, "relatorio-contexto.json")},
                    "http://127.0.0.1:9/", dados)
    it = rel["itens"][0]
    sd = it.get("saida") or {}
    txt = open(sd["txt"], encoding="utf-8").read() if sd.get("txt") else ""
    print(f"  {it['nome']}: {it['etapa']} · {it.get('avisos')}")
    claros = [x for x in ("Vale Fictício", "VALE FICTÍCIO", "Serra Inventada", "Boa Esperança", "Recanto Feliz", "Bom Preço", "Tonhão", "Agro Fictícia",
                          "tabelião", "prefeito", "vereadora", "Secretário Municipal", "1000777-22", "GENOVEVO") if x in txt]
    marcas = [t for t in ("«MUNICIPIO_01»", "«CARGO_01»", "«FAZENDA_01»", "«EMPRESA_01»", "«PROCESSO_01»") if t in txt]
    confere(it["etapa"] == "pronto" and len(marcas) == 5 and not claros,
            f"o município, a comarca, o cargo, a fazenda, a empresa e o número do processo saem do texto ({len(marcas)} tipos de marcador; em claro: {claros or 'nada'})")
    aut_claros = [x for x in ("Ficticiano", "Inventado", "Imaginária", "Fabulado") if x in txt]
    aut_marcas = [t for t in ("«JUIZ_01»", "«PROMOTOR_01»", "«DELEGADO_01»", "«ADVOGADO_01»") if t in txt]
    confere(len(aut_marcas) == 4 and not aut_claros and "15/03/2026" in txt,
            f"o juiz, o promotor, o delegado e o advogado saem do texto, cada um com o marcador do papel ({aut_marcas}; em claro: {aut_claros or 'nada'})")
    confere("Contexto também trocado por tokens: " in txt and "MUNICIPIO (município ou comarca)" in txt, "o cabeçalho avisa a IA dos tokens de contexto")
    confere("12/03/2026" in txt and "Fazenda Pública" in txt and "SSP/MT" in txt and "sorriso do denunciado" in txt and "MATO GROSSO" in txt,
            "as datas ficam, e o que não é contexto também (Fazenda Pública, SSP/MT, a palavra comum em minúscula, o estado)")
    al = it.get("alertas") or []
    regras = {a.split(":")[0] for a in al}
    confere({"unico", "porte", "parente", "dono"} <= regras and "processo" not in regras,
            f"o alerta de contexto aponta o que sobra: o único, o tamanho do lugar, o parentesco, o dono do negócio ({sorted(regras)})")
    bruto = open(sd["mapa"], encoding="utf-8").read() if sd.get("mapa") else ""
    cf = rel.get("cofre") or {}
    confere(bruto.lstrip().startswith("{") and '"incognitus_cofre": 1' in bruto[:80] and not [x for x in ("GENOVEVO", "Vale Fict", "tokens", "MUNICIPIO") if x in bruto]
            and cf.get("cifrado") and not cf.get("aviso"), "o mapa vai cifrado para o disco: nenhum nome, nenhum marcador no arquivo")
    confere(not cf.get("erro") and "«MUNICIPIO_01»" in (cf.get("tokens") or []) and "«PROCESSO_01»" in (cf.get("tokens") or []),
            f"o app abre o mapa cifrado e acha nele os marcadores do contexto ({cf.get('erro') or len(cf.get('tokens') or [])} marcadores)")


MODELOS_FICTICIOS = {
    "Denúncia - tráfico de drogas.txt": "EXCELENTÍSSIMO SENHOR JUIZ DE DIREITO. O Ministério Público oferece DENÚNCIA contra [NOME], pela prática do "
        "crime do art. 33 da Lei 11.343/2006 (tráfico de drogas): trazia consigo, para venda, porções de entorpecente. Requer o recebimento da denúncia.",
    "Arquivamento de inquérito.txt": "Promoção de arquivamento do inquérito policial: ausência de justa causa, sem indícios de autoria nem prova da "
        "materialidade. Requer o arquivamento, com as comunicações de praxe.",
    os.path.join("Recursos", "Contrarrazões da acusação.md"): "CONTRARRAZÕES AO RECURSO DE APELAÇÃO interposto pela defesa. A sentença condenatória "
        "deve ser mantida: a prova é firme e a dosimetria, adequada. Requer o desprovimento do recurso.",
    os.path.join("Cível", "Ação civil pública - improbidade.txt"): "AÇÃO CIVIL PÚBLICA POR ATO DE IMPROBIDADE ADMINISTRATIVA. O Ministério Público, "
        "com base na Lei 8.429/1992, pede o ressarcimento do dano ao erário e a indisponibilidade de bens.",
}


def secao_pasta(dados):
    print("9. a pasta de modelos com a busca por BERT (SKIA_BUSCA_DIR apontando para o modelo já baixado)")
    bd = os.environ.get("SKIA_BUSCA_DIR", "")
    if not (bd and os.path.exists(os.path.join(bd, "model.onnx"))):
        print("  (pulei: sem o modelo em SKIA_BUSCA_DIR)")
        return
    base = os.path.join(BASE, "pasta-modelos")
    shutil.rmtree(base, ignore_errors=True)
    pasta = os.path.join(base, "meus modelos")
    for nome, texto in MODELOS_FICTICIOS.items():
        os.makedirs(os.path.dirname(os.path.join(pasta, nome)), exist_ok=True)
        open(os.path.join(pasta, nome), "w", encoding="utf-8").write(texto + "\n")
    open(os.path.join(pasta, "~$aberto.docx"), "w").write("lixo do Word")
    pdf = os.path.join(base, "outro caso.pdf")
    open(pdf, "wb").write(gerar_pdfs.pdf_texto([gerar_pdfs.CASO_OUTRO.splitlines()]))
    prompt = os.path.join(base, "prompt.md")
    open(prompt, "w", encoding="utf-8").write("# Prompt de prova\nUse os modelos de quem pede.\n")
    srv_ia, url_ia, pedidos = anthropic_de_mentira()
    rel = rodar_app({"arquivos": [pdf], "saida": os.path.join(base, "saida"), "txt": False,
                     "extracao": os.path.join(SAIDA, "extracao-pasta"), "relatorio": os.path.join(SAIDA, "relatorio-pasta.json"),
                     "mpa": {"prompt": prompt, "pasta": pasta}}, "http://127.0.0.1:9/", dados,
                    {"SKIA_CLAUDE_URL": url_ia, "SKIA_CHAVE_DE_PROVA": CHAVE_DE_PROVA, "SKIA_BUSCA_DIR": bd})
    srv_ia.shutdown()
    mp = rel.get("mpa") or {}
    print(f"  🧠 com a pasta: {mp.get('etapa')} · modelos lidos {mp.get('modelos')} · {len(pedidos)} pedido(s)" + (f" · ERRO {mp.get('erro')}" if mp.get("erro") else "") + (f" · avisos {mp.get('avisos')}" if mp.get("avisos") else ""))
    s0 = (pedidos[0]["corpo"].get("system") or "") if pedidos else ""
    confere(len(pedidos) >= 3 and [f["name"] for f in pedidos[0]["corpo"].get("tools", [])] == ["buscar_modelos", "ler_modelo"]
            and "MODELOS DE QUEM PEDE" in s0 and s0.count(" | ") >= 4 and "aberto" not in s0,
            "o system leva os modelos da pasta (sem o ~$ do Word) e o pedido, as ferramentas buscar_modelos e ler_modelo")
    res = [b for b in (pedidos[1]["corpo"]["messages"][-1]["content"] if len(pedidos) > 1 else []) if b.get("type") == "tool_result"]
    prim = (re.findall(r"^P\d+ \| ([^|]+)\|", res[0]["content"], re.M) or [""])[0].strip() if res else ""
    confere(prim.startswith("Denúncia - tráfico"), f"o BERT põe o modelo da denúncia por tráfico no topo da busca ({prim or 'nada'})")
    confere(mp.get("etapa") == "pronto" and any("tráfico" in x for x in (mp.get("modelos") or [])),
            "a IA lê o modelo achado (ler_modelo pelo ID da busca) e o 🧠 termina")
    tudo = json.dumps([p["corpo"] for p in pedidos], ensure_ascii=False)
    confere(base not in tudo and os.path.expanduser("~") not in tudo, "o caminho da pasta no computador não vai à IA (só o nome e a subpasta)")


def main():
    if not os.path.exists(EXE):
        raise SystemExit("falta o executável: npx tauri build --debug --no-bundle")
    if os.environ.get("SKIA_E2E_SO") == "contexto":   # só a seção 10
        os.makedirs(SAIDA, exist_ok=True)
        secao_contexto(os.path.join(SAIDA, "dados-contexto"))
        print("\nRESULTADO: " + ("tudo certo" if not falhas else f"{len(falhas)} falha(s)"))
        sys.exit(1 if falhas else 0)
    if os.environ.get("SKIA_E2E_SO") in ("tesoura", "pasta"):   # só a seção 8 ou a 9
        os.makedirs(SAIDA, exist_ok=True)
        if os.environ.get("SKIA_E2E_SO") == "tesoura":
            secao_tesoura(os.path.join(SAIDA, "dados-tesoura"))
        else:
            secao_pasta(os.path.join(SAIDA, "dados-pasta"))
        print("\nRESULTADO: " + ("tudo certo" if not falhas else f"{len(falhas)} falha(s)"))
        sys.exit(1 if falhas else 0)
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
    confere(mc.get("bloco"), "o card pronto tem o Reidentificar minuta… no painel de botões")
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
    secao_tesoura(dados)
    secao_pasta(dados)
    secao_contexto(dados)

    print()
    print("RESULTADO: " + ("tudo certo" if not falhas else f"{len(falhas)} falha(s)"))
    sys.exit(1 if falhas else 0)


if __name__ == "__main__":
    if incognitus_aberto():
        sys.exit("e2e: feche o Incognitus instalado antes do teste (ele divide a identidade com o app de teste e o atrapalha)")
    main()
