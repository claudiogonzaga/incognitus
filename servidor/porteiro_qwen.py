#!/usr/bin/env python3
"""O porteiro do Qwen do servidor (Incognitus, 26/11/2026, pedido do usuário: "usar o Qwen deste Mac, sem expor nada
sem senha"). Fica na frente do Ollama do Mac servidor e só deixa passar quem traz o segredo:

  - o túnel da Cloudflare (qwen.consciencia.eco.br) chega em 127.0.0.1:11436. Quem fala com ele é só a função do
    Pages (functions/incognitus/webapp/_middleware.js, no repositório do portal), que confere a sessão de convidado
    e põe o cabeçalho X-Qwen-Segredo. O navegador nunca vê o segredo.
  - opcional, a rede local (QWEN_REDE=0.0.0.0:11435): exige "Authorization: Bearer <senha>". Desligada por padrão.

Só passam três caminhos: POST /api/chat, GET /api/tags e GET /api/version. Os de administração do Ollama (pull,
delete, create, push, copy…) não passam. O pedido é refeito só com os campos conhecidos, o modelo tem de estar
instalado, e o contexto e a saída têm teto.

Enquanto o Qwen lê os autos (num M1, mais de 100 s), a resposta já começou e leva uma linha em branco a cada 10 s,
para que o túnel e a função não derrubem uma conexão parada. Se quem pediu desiste, a conexão com o Ollama fecha e
ele para de gerar.

Nada do conteúdo vai ao log, só a hora, o caminho, o código, os tamanhos e a duração.

Arquivos em ~/.qwen-servidor: segredo (o do túnel), senha (a da rede, se houver).
Só a biblioteca padrão do Python 3.9.
"""
import hmac
import http.client
import json
import os
import queue
import socket
import sys
import threading
import time
import urllib.parse
from http.server import BaseHTTPRequestHandler, ThreadingHTTPServer
from pathlib import Path

BASE = Path(os.environ.get("QWEN_BASE") or (Path.home() / ".qwen-servidor"))
OLLAMA = urllib.parse.urlsplit(os.environ.get("QWEN_OLLAMA") or "http://127.0.0.1:11434")
TUNEL = os.environ.get("QWEN_TUNEL", "127.0.0.1:11436")
REDE = os.environ.get("QWEN_REDE", "")            # ex.: 0.0.0.0:11435; vazio: não escuta na rede local
MAX_CORPO = 8 * 1024 * 1024                         # 8 MB de pedido (os autos já vêm cortados pelo app)
MAX_CTX = int(os.environ.get("QWEN_MAX_CTX", "65536"))
MAX_SAIDA = 16384
BATIDA = 10                                         # s entre as linhas em branco, enquanto o Qwen pensa
MAX_FILA = int(os.environ.get("QWEN_MAX_FILA", "4"))  # pedidos de chat ao mesmo tempo (o Ollama atende um por vez)
OPCOES = {"num_ctx", "num_predict", "temperature", "top_p", "top_k", "min_p", "presence_penalty", "repeat_penalty",
          "repeat_last_n", "seed", "stop", "num_keep"}
PAPEIS = {"system", "user", "assistant", "tool"}

fila = threading.BoundedSemaphore(MAX_FILA)
_modelos = {"lista": set(), "quando": 0.0}
_trava = threading.Lock()


def registrar(*partes):
    print(time.strftime("%Y-%m-%d %H:%M:%S"), *partes, flush=True)


def ler_segredo(nome):
    try:
        s = (BASE / nome).read_text().strip()
    except FileNotFoundError:
        return ""
    return s if len(s) >= 24 else ""


def ollama(metodo, caminho, corpo=None, timeout=30):
    c = http.client.HTTPConnection(OLLAMA.hostname, OLLAMA.port or 80, timeout=timeout)
    try:
        c.request(metodo, caminho, body=corpo, headers={"Content-Type": "application/json"} if corpo else {})
        r = c.getresponse()
        return r.status, r.read()
    finally:
        c.close()


def modelos_instalados():
    with _trava:
        if time.time() - _modelos["quando"] > 60:
            try:
                st, b = ollama("GET", "/api/tags")
                if st == 200:
                    ms = json.loads(b).get("models", [])
                    _modelos["lista"] = {m.get("name") for m in ms} | {m.get("model") for m in ms}
                    _modelos["quando"] = time.time()
            except (OSError, ValueError):
                pass
        return set(_modelos["lista"])


def num(v, lo, hi):
    try:
        v = int(v)
    except (TypeError, ValueError):
        return None
    return max(lo, min(hi, v))


def limpar_pedido(p):
    """O pedido refeito só com o que o Ollama precisa. Erro: (código, mensagem)."""
    if not isinstance(p, dict):
        raise ValueError((400, "o pedido não é um objeto JSON"))
    modelo = str(p.get("model") or "")
    if modelo not in modelos_instalados():
        raise ValueError((400, "modelo não instalado neste servidor: " + modelo[:80]))
    msgs = []
    for m in p.get("messages") or []:
        if not isinstance(m, dict) or m.get("role") not in PAPEIS:
            raise ValueError((400, "mensagem inválida"))
        x = {"role": m["role"], "content": str(m.get("content") or "")}
        if isinstance(m.get("tool_calls"), list):
            x["tool_calls"] = m["tool_calls"]
        if isinstance(m.get("tool_name"), str):
            x["tool_name"] = m["tool_name"]
        msgs.append(x)
    if not msgs:
        raise ValueError((400, "o pedido não tem mensagens"))
    corpo = {"model": modelo, "messages": msgs, "stream": p.get("stream", True) is not False}
    if isinstance(p.get("think"), bool) or p.get("think") in ("low", "medium", "high"):
        corpo["think"] = p["think"]
    if p.get("format") == "json" or isinstance(p.get("format"), dict):
        corpo["format"] = p["format"]
    if isinstance(p.get("tools"), list):
        corpo["tools"] = p["tools"]
    op = {}
    for k, v in (p.get("options") or {}).items() if isinstance(p.get("options"), dict) else []:
        if k not in OPCOES:
            continue
        if k == "stop":
            if isinstance(v, list):
                op[k] = [str(s)[:40] for s in v[:8]]
        elif isinstance(v, (int, float)) and not isinstance(v, bool):
            op[k] = v
    if "num_ctx" in op:
        op["num_ctx"] = num(op["num_ctx"], 2048, MAX_CTX)
    np = num(op.get("num_predict"), -1, MAX_SAIDA)
    op["num_predict"] = MAX_SAIDA if np is None or np <= 0 else np
    corpo["options"] = op
    return corpo


class Porteiro(BaseHTTPRequestHandler):
    protocol_version = "HTTP/1.1"
    server_version = "porteiro-qwen"
    sys_version = ""
    timeout = 120
    modo = "tunel"
    esperado = ""

    def log_message(self, formato, *args):   # o log é o nosso, sem conteúdo
        pass

    def autorizado(self):
        if self.modo == "tunel":
            dado = self.headers.get("X-Qwen-Segredo", "")
        else:
            a = self.headers.get("Authorization", "")
            dado = a[7:].strip() if a[:7].lower() == "bearer " else ""
        return bool(self.esperado) and hmac.compare_digest(dado.encode(), self.esperado.encode())

    def caminho(self):
        return urllib.parse.urlsplit(self.path).path

    def json(self, code, obj):
        corpo = json.dumps(obj, ensure_ascii=False).encode()
        self.send_response(code)
        self.send_header("Content-Type", "application/json; charset=utf-8")
        self.send_header("Content-Length", str(len(corpo)))
        self.send_header("Cache-Control", "no-store")
        self.send_header("X-Content-Type-Options", "nosniff")
        self.end_headers()
        self.wfile.write(corpo)
        return len(corpo)

    def do_GET(self):
        t0, cam = time.time(), self.caminho()
        if not self.autorizado():
            code, n = 403, self.json(403, {"error": "proibido"})
        elif cam in ("/api/tags", "/api/version"):
            try:
                code, b = ollama("GET", cam)
                n = self.json(code, json.loads(b or b"{}"))
            except (OSError, ValueError) as e:
                code, n = 502, self.json(502, {"error": "o Ollama não respondeu (" + e.__class__.__name__ + ")"})
        else:
            code, n = 404, self.json(404, {"error": "caminho desconhecido"})
        registrar(self.modo, "GET", cam, code, "saiu=" + str(n), "%.1fs" % (time.time() - t0))

    def do_POST(self):
        t0, cam, tam, code, n = time.time(), self.caminho(), 0, "?", 0
        try:
            if not self.autorizado():
                code, n = 403, self.json(403, {"error": "proibido"})
                return
            if cam != "/api/chat":
                code, n = 404, self.json(404, {"error": "caminho desconhecido"})
                return
            try:
                tam = int(self.headers.get("Content-Length") or "-1")
            except ValueError:
                tam = -1
            if tam < 0 or tam > MAX_CORPO:
                self.close_connection = True
                code, n = 413, self.json(413, {"error": "pedido grande demais ou sem tamanho"})
                return
            try:
                corpo = limpar_pedido(json.loads(self.rfile.read(tam)))
            except ValueError as e:
                c, msg = e.args[0] if e.args and isinstance(e.args[0], tuple) else (400, "o pedido não é JSON")
                code, n = c, self.json(c, {"error": msg})
                return
            if not fila.acquire(blocking=False):
                code, n = 429, self.json(429, {"error": "o Qwen do servidor está ocupado com outros pedidos: tente daqui a pouco"})
                return
            try:
                if corpo["stream"]:
                    code, n = self.em_partes(corpo)
                else:
                    try:
                        c, b = ollama("POST", "/api/chat", json.dumps(corpo).encode(), timeout=1800)
                        code, n = c, self.json(c, json.loads(b or b"{}"))
                    except (OSError, ValueError) as e:
                        code, n = 502, self.json(502, {"error": "o Ollama não respondeu (" + e.__class__.__name__ + ")"})
            finally:
                fila.release()
        finally:
            registrar(self.modo, "POST", cam, code, "entrou=" + str(max(tam, 0)), "saiu=" + str(n), "%.1fs" % (time.time() - t0))

    def pedaco(self, b):
        self.wfile.write(b"%x\r\n" % len(b) + b + b"\r\n")
        self.wfile.flush()

    def em_partes(self, corpo):
        """A resposta em NDJSON, aos pedaços, com a linha em branco de sinal de vida."""
        self.send_response(200)
        self.send_header("Content-Type", "application/x-ndjson; charset=utf-8")
        self.send_header("Cache-Control", "no-store, no-transform")
        self.send_header("X-Accel-Buffering", "no")
        self.send_header("Transfer-Encoding", "chunked")
        self.send_header("Connection", "close")
        self.end_headers()
        self.close_connection = True
        q = queue.Queue()
        con = http.client.HTTPConnection(OLLAMA.hostname, OLLAMA.port or 80, timeout=1800)
        sock = {"s": None}

        def trabalhar():
            try:
                con.connect()
                sock["s"] = con.sock
                con.request("POST", "/api/chat", body=json.dumps(corpo).encode(), headers={"Content-Type": "application/json"})
                r = con.getresponse()
                if r.status != 200:
                    txt = r.read(4000).decode("utf-8", "replace")
                    try:
                        msg = json.loads(txt).get("error") or txt
                    except ValueError:
                        msg = txt
                    q.put(("erro", "o Ollama respondeu %d: %s" % (r.status, str(msg)[:300])))
                    return
                while True:
                    linha = r.readline()
                    if not linha:
                        break
                    q.put(("linha", linha))
                q.put(("fim", None))
            except (OSError, http.client.HTTPException) as e:
                q.put(("erro", "o Ollama não respondeu (" + e.__class__.__name__ + ")"))

        threading.Thread(target=trabalhar, daemon=True).start()
        n, code = 0, 200
        try:
            while True:
                try:
                    tipo, dado = q.get(timeout=BATIDA)
                except queue.Empty:
                    self.pedaco(b"\n")
                    continue
                if tipo == "linha":
                    dado = dado if dado.endswith(b"\n") else dado + b"\n"
                    n += len(dado)
                    self.pedaco(dado)
                elif tipo == "erro":
                    code = 502
                    self.pedaco((json.dumps({"error": dado}, ensure_ascii=False) + "\n").encode())
                    break
                else:
                    break
            self.wfile.write(b"0\r\n\r\n")
            self.wfile.flush()
        except (BrokenPipeError, ConnectionResetError, socket.timeout):
            code = 499   # quem pediu desistiu: o Ollama para de gerar quando a conexão fecha
        finally:
            try:
                if sock["s"] is not None:
                    sock["s"].shutdown(socket.SHUT_RDWR)
            except OSError:
                pass
            con.close()
        return code, n


def servir(endereco, modo, esperado):
    host, porta = endereco.rsplit(":", 1)
    classe = type("Porteiro_" + modo, (Porteiro,), {"modo": modo, "esperado": esperado})
    s = ThreadingHTTPServer((host, int(porta)), classe)
    s.daemon_threads = True
    registrar("escutando", modo, endereco, "→ Ollama", OLLAMA.geturl())
    return s


def main():
    segredo = ler_segredo("segredo")
    if not segredo:
        sys.exit("falta o segredo do túnel em " + str(BASE / "segredo") + " (24 caracteres ou mais)")
    tunel = servir(TUNEL, "tunel", segredo)
    if REDE:
        senha = ler_segredo("senha")
        if not senha:
            sys.exit("QWEN_REDE pede a senha da rede em " + str(BASE / "senha") + " (24 caracteres ou mais)")
        threading.Thread(target=servir_rede, args=(senha,), daemon=True).start()
    tunel.serve_forever()


def servir_rede(senha):
    """A rede local sobe ao lado do túnel. Se o endereço ainda não existe (o do Tailscale, logo depois de ligar o Mac),
    tenta de novo a cada 30 s, sem derrubar o túnel."""
    avisou = False
    while True:
        try:
            s = servir(REDE, "rede", senha)
        except OSError as e:
            if not avisou:
                registrar("rede", REDE, "ainda indisponível (" + str(e) + "); tento a cada 30 s")
                avisou = True
            time.sleep(30)
            continue
        s.serve_forever()


if __name__ == "__main__":
    main()
