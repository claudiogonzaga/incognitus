#!/usr/bin/env python3
"""Incognitus — confere os sidecars SEM o app: chama pdftotext, pdftoppm e tesseract como o
src-tauri/src/ferramentas.rs chama (os mesmos argumentos e as mesmas variáveis de ambiente), sobre os PDFs fictícios,
e mostra o código de saída e o stderr de cada um. Roda no CI antes do build (falha rápido) e serve de diagnóstico.
Uso: python3 testes/diag_sidecars.py   (depois de scripts/sidecars.py)
"""
import glob
import os
import platform
import shutil
import subprocess
import sys
import tempfile

sys.stdout.reconfigure(encoding="utf-8", errors="replace", line_buffering=True)
RAIZ = os.path.dirname(os.path.dirname(os.path.abspath(__file__)))
sys.path.insert(0, os.path.dirname(os.path.abspath(__file__)))
import gerar_pdfs  # noqa: E402

TRIPLO = {("Darwin", "arm64"): "aarch64-apple-darwin", ("Darwin", "x86_64"): "x86_64-apple-darwin",
          ("Windows", "AMD64"): "x86_64-pc-windows-msvc"}[(platform.system(), platform.machine())]
WIN = os.name == "nt"
REC = os.path.join(RAIZ, "src-tauri", "recursos")
falhou = False


def main():
    global falhou
    trab = tempfile.mkdtemp(prefix="incognitus-diag-")
    # o layout do app em modo de desenvolvimento: ferramentas e lib/ lado a lado (no Mac, o rpath @executable_path/lib)
    pasta = os.path.join(trab, "app")
    os.makedirs(pasta)
    for f in ("pdftotext", "pdftoppm", "tesseract"):
        ext = ".exe" if WIN else ""
        shutil.copy2(os.path.join(RAIZ, "src-tauri", "binaries", f"{f}-{TRIPLO}{ext}"), os.path.join(pasta, f + ext))
    lib = os.path.join(pasta, "lib")
    shutil.copytree(os.path.join(REC, "win" if WIN else "mac", "lib"), lib)
    tessdata = os.path.join(REC, "tessdata")
    tmp = os.path.join(trab, "tmp")
    os.makedirs(tmp)
    conf = os.path.join(trab, "fonts.conf")
    dirs = "<dir>WINDOWSFONTDIR</dir>" if WIN else "<dir>/System/Library/Fonts</dir><dir>/Library/Fonts</dir>"
    open(conf, "w", encoding="utf-8").write(f'<?xml version="1.0"?>\n<!DOCTYPE fontconfig SYSTEM "urn:fontconfig:fonts.dtd">\n'
                          f"<fontconfig>{dirs}<cachedir>{os.path.join(trab, 'fc')}</cachedir></fontconfig>\n")
    env = dict(os.environ, FONTCONFIG_FILE=conf, OMP_THREAD_LIMIT="1", TMPDIR=tmp, TMP=tmp, TEMP=tmp)
    if WIN:
        env["PATH"] = lib + ";" + env.get("PATH", "")
    pdfs = os.path.join(trab, "pdfs")
    gerar_pdfs.PASTA = pdfs
    gerar_pdfs.main()

    def roda(nome, args, timeout=180):
        global falhou
        exe = os.path.join(pasta, nome + (".exe" if WIN else ""))
        r = subprocess.run([exe] + args, env=env, capture_output=True, timeout=timeout,
                           creationflags=(0x08000000 | 0x00004000) if WIN else 0)
        err = r.stderr.decode("utf-8", "replace").strip()
        print(f"  {nome} {' '.join(args[:3])}… → código {r.returncode}" + (f" · stderr: {err[:400]}" if err else ""))
        if r.returncode:
            falhou = True
        return r

    print("versões")
    roda("pdftotext", ["-v"])
    roda("pdftoppm", ["-v"])
    roda("tesseract", ["--version"])
    print("pdftotext -layout (texto.pdf)")
    r = roda("pdftotext", ["-layout", os.path.join(pdfs, "texto.pdf"), "-"])
    print(f"  {len(r.stdout)} bytes, {r.stdout.count(b'\f')} páginas")
    print("OCR da página 1 do escaneado.pdf (pdftoppm + tesseract)")
    saida = os.path.join(trab, "ocr")
    os.makedirs(saida)
    roda("pdftoppm", ["-r", "200", "-gray", "-png", "-f", "1", "-l", "1", os.path.join(pdfs, "escaneado.pdf"), os.path.join(saida, "p")])
    imgs = sorted(glob.glob(os.path.join(saida, "*.png")))
    print(f"  imagens: {[os.path.basename(i) for i in imgs]}")
    if not imgs:
        falhou = True
    else:
        r = roda("tesseract", [imgs[0], "stdout", "-l", "por", "--tessdata-dir", tessdata])
        t = r.stdout.decode("utf-8", "replace")
        letras = sum(c.isalpha() for c in t)
        print(f"  {letras} letras lidas: {t.strip()[:160]!r}")
        if letras < 50:
            falhou = True
    shutil.rmtree(trab, ignore_errors=True)
    print("RESULTADO: " + ("FALHOU" if falhou else "sidecars ok"))
    sys.exit(1 if falhou else 0)


if __name__ == "__main__":
    main()
