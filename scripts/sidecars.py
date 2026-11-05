#!/usr/bin/env python3
"""Σκιά · Skiá — monta os sidecars do Tauri (pdftotext, pdftoppm, tesseract), as bibliotecas deles, o por.traineddata
e as licenças.

Versões: as do serviço do Graphé. O cloudrun-pdftotext é construído de python:3.11-slim (Debian 13): poppler 25.03,
tesseract 5.5.0 e o tesseract-ocr-por do Debian, que é o tessdata_fast 4.1.0 (o MESMO arquivo, sha256 conferido em
05/11/2026). Aqui, do conda-forge: poppler 25.02.0 (o mais próximo publicado) e tesseract 5.5.0.

Uso:
    python3 scripts/sidecars.py mac       # no Mac: arm64 + x86_64 → universal (src-tauri/binaries, recursos/mac/lib)
    python3 scripts/sidecars.py windows   # no Windows x64 (precisa de: pip install pefile)
Saídas (fora do git): src-tauri/binaries/, src-tauri/recursos/{mac,win}/lib/, recursos/tessdata/, recursos/LICENCAS-terceiros/.
"""
import glob
import hashlib
import json
import os
import platform
import re
import shutil
import subprocess
import sys
import tarfile
import urllib.request

POPPLER = "25.02.0"
TESSERACT = "5.5.0"
MICROMAMBA = "2.10.0"
POR_URL = "https://github.com/tesseract-ocr/tessdata_fast/raw/4.1.0/por.traineddata"
POR_SHA256 = "c4932b937207a9514b7514d518b931a99938c02a28a5a5a553f8599ed58b7deb"
FERRAMENTAS = ("pdftotext", "pdftoppm", "tesseract")

RAIZ = os.path.dirname(os.path.dirname(os.path.abspath(__file__)))
TAURI = os.path.join(RAIZ, "src-tauri")
CACHE = os.path.join(RAIZ, ".ferramentas")
BIN = os.path.join(TAURI, "binaries")
REC = os.path.join(TAURI, "recursos")
LIC = os.path.join(REC, "LICENCAS-terceiros")


def rodar(cmd, **kw):
    r = subprocess.run(cmd, capture_output=True, text=True, **kw)
    if r.returncode:
        raise SystemExit("falhou: " + " ".join(cmd) + "\n" + (r.stderr or r.stdout)[-2000:])
    return r.stdout


def baixar(url, destino, sha256=None):
    if not os.path.exists(destino):
        os.makedirs(os.path.dirname(destino), exist_ok=True)
        with urllib.request.urlopen(url, timeout=300) as r, open(destino + ".parcial", "wb") as f:
            shutil.copyfileobj(r, f)
        os.replace(destino + ".parcial", destino)
    if sha256:
        h = hashlib.sha256(open(destino, "rb").read()).hexdigest()
        if h != sha256:
            os.remove(destino)
            raise SystemExit(f"sha256 errado em {destino}: {h}")
    return destino


def micromamba():
    plat = {("Darwin", "arm64"): "osx-arm64", ("Darwin", "x86_64"): "osx-64",
            ("Windows", "AMD64"): "win-64", ("Linux", "x86_64"): "linux-64"}[(platform.system(), platform.machine())]
    exe = os.path.join(CACHE, "Library", "bin", "micromamba.exe") if plat == "win-64" else os.path.join(CACHE, "bin", "micromamba")
    if not os.path.exists(exe):
        arq = baixar(f"https://micro.mamba.pm/api/micromamba/{plat}/{MICROMAMBA}", os.path.join(CACHE, f"micromamba-{plat}.tar.bz2"))
        with tarfile.open(arq) as t:
            membro = "Library/bin/micromamba.exe" if plat == "win-64" else "bin/micromamba"
            t.extract(membro, CACHE)
        os.chmod(exe, 0o755)
    return exe


def ambiente(plataforma):
    """O ambiente conda com as versões fixadas (criado uma vez; o cache fica em .ferramentas/)."""
    env = os.path.join(CACHE, "env-" + plataforma)
    pronto = os.path.join(env, ".skia-pronto")
    if not os.path.exists(pronto):
        os.environ["MAMBA_ROOT_PREFIX"] = os.path.join(CACHE, "mamba")
        rodar([micromamba(), "create", "-y", "-q", "-p", env, "--platform", plataforma, "-c", "conda-forge",
               "--override-channels", f"poppler={POPPLER}", f"tesseract={TESSERACT}"])
        open(pronto, "w").write(f"poppler={POPPLER} tesseract={TESSERACT}\n")
    return env


def pacotes_do_ambiente(env):
    """{arquivo relativo: metadados do pacote conda que o instalou}."""
    dono = {}
    for j in glob.glob(os.path.join(env, "conda-meta", "*.json")):
        m = json.load(open(j, encoding="utf-8"))
        for f in m.get("files", []):
            dono[f.replace("\\", "/")] = m
    return dono


# ── macOS ──────────────────────────────────────────────────────────────────────────────────────────────────────────
def dependencias_mac(arq):
    linhas = rodar(["otool", "-L", arq]).splitlines()[1:]
    return [l.strip().split(" (")[0] for l in linhas if l.strip()]


def rpaths_mac(arq):
    out, linhas = [], rodar(["otool", "-l", arq]).splitlines()
    for i, l in enumerate(linhas):
        if l.strip() == "cmd LC_RPATH":
            out.append(linhas[i + 2].strip().split("path ", 1)[1].rsplit(" (offset", 1)[0])
    return out


def fecho_mac(env):
    """Os executáveis e as dylibs que eles carregam (só as do ambiente; as do sistema ficam)."""
    vistos, fila = {}, [os.path.join(env, "bin", b) for b in FERRAMENTAS]
    while fila:
        f = fila.pop()
        nome = os.path.basename(f)
        if nome in vistos:
            continue
        vistos[nome] = f
        for d in dependencias_mac(f):
            if d.startswith("/usr/lib/") or d.startswith("/System/"):
                continue
            base = os.path.basename(d)
            q = os.path.join(env, "lib", base)
            if not os.path.exists(q):
                raise SystemExit(f"{nome} precisa de {d}, que não está em {env}/lib")
            fila.append(q)
    return vistos


def preparar_mac(env, palco):
    """Copia o fecho para o palco e acerta os caminhos: executável procura as libs em ../Resources/lib (no .app) e em
    lib/ (no tauri dev); lib procura as irmãs na própria pasta."""
    os.makedirs(palco, exist_ok=True)
    arquivos = fecho_mac(env)
    for nome, orig in arquivos.items():
        dest = os.path.join(palco, nome)
        shutil.copyfile(os.path.realpath(orig), dest)
        os.chmod(dest, 0o755)
        for rp in rpaths_mac(dest):
            rodar(["install_name_tool", "-delete_rpath", rp, dest])
        for d in dependencias_mac(dest):
            if not (d.startswith("/usr/lib/") or d.startswith("/System/") or d.startswith("@rpath/")):
                rodar(["install_name_tool", "-change", d, "@rpath/" + os.path.basename(d), dest])
        if nome in FERRAMENTAS:
            rodar(["install_name_tool", "-add_rpath", "@executable_path/../Resources/lib", dest])
            rodar(["install_name_tool", "-add_rpath", "@executable_path/lib", dest])
        else:
            rodar(["install_name_tool", "-id", "@rpath/" + nome, dest])
            rodar(["install_name_tool", "-add_rpath", "@loader_path", dest])
    return arquivos


def assinar_mac(arq):
    rodar(["codesign", "--force", "--sign", "-", arq])


def montar_mac():
    envs = {"arm64": ambiente("osx-arm64"), "x86_64": ambiente("osx-64")}
    palcos = {a: os.path.join(CACHE, "palco-mac-" + a) for a in envs}
    for a, p in palcos.items():
        shutil.rmtree(p, ignore_errors=True)
    origens = {a: preparar_mac(envs[a], palcos[a]) for a in envs}
    lib = os.path.join(REC, "mac", "lib")
    shutil.rmtree(lib, ignore_errors=True)
    os.makedirs(lib, exist_ok=True)
    os.makedirs(BIN, exist_ok=True)
    nomes = sorted(set(origens["arm64"]) | set(origens["x86_64"]))
    for nome in nomes:
        partes = [os.path.join(palcos[a], nome) for a in envs if nome in origens[a]]
        uni = os.path.join(CACHE, "palco-mac-uni-" + nome)
        if len(partes) == 2:
            rodar(["lipo", "-create"] + partes + ["-output", uni])
        else:
            shutil.copyfile(partes[0], uni)
        assinar_mac(uni)
        if nome in FERRAMENTAS:
            shutil.copyfile(uni, os.path.join(BIN, f"{nome}-universal-apple-darwin"))
            for a, triplo in (("arm64", "aarch64-apple-darwin"), ("x86_64", "x86_64-apple-darwin")):
                fino = os.path.join(BIN, f"{nome}-{triplo}")
                rodar(["lipo", uni, "-thin", a, "-output", fino])
                assinar_mac(fino)
            for f in glob.glob(os.path.join(BIN, nome + "-*")):
                os.chmod(f, 0o755)
        else:
            shutil.copyfile(uni, os.path.join(lib, nome))
        os.remove(uni)
    licencas([(envs[a], origens[a]) for a in envs])
    print(f"mac: {len(nomes) - len(FERRAMENTAS)} bibliotecas universais em {lib}")


# ── Windows ────────────────────────────────────────────────────────────────────────────────────────────────────────
def montar_windows():
    import pefile  # pip install pefile
    env = ambiente("win-64")
    pastas = [os.path.join(env, "Library", "bin"), env]
    acha = lambda n: next((os.path.join(p, n) for p in pastas if os.path.exists(os.path.join(p, n))), None)
    vistos, fila = {}, [os.path.join(env, "Library", "bin", b + ".exe") for b in FERRAMENTAS]
    while fila:
        f = fila.pop()
        nome = os.path.basename(f).lower()
        if nome in vistos:
            continue
        vistos[nome] = f
        pe = pefile.PE(f, fast_load=True)
        pe.parse_data_directories(directories=[pefile.DIRECTORY_ENTRY["IMAGE_DIRECTORY_ENTRY_IMPORT"],
                                               pefile.DIRECTORY_ENTRY["IMAGE_DIRECTORY_ENTRY_DELAY_IMPORT"]])
        for ent in list(getattr(pe, "DIRECTORY_ENTRY_IMPORT", [])) + list(getattr(pe, "DIRECTORY_ENTRY_DELAY_IMPORT", [])):
            dll = ent.dll.decode("ascii", "replace")
            q = acha(dll)
            if q:  # as que não estão no ambiente são do Windows (kernel32, ucrt, api-ms-win-…)
                fila.append(q)
        pe.close()
    lib = os.path.join(REC, "win", "lib")
    shutil.rmtree(lib, ignore_errors=True)
    os.makedirs(lib, exist_ok=True)
    os.makedirs(BIN, exist_ok=True)
    for nome, f in vistos.items():
        base = os.path.basename(f)
        if base[:-4].lower() in FERRAMENTAS:
            shutil.copyfile(f, os.path.join(BIN, f"{base[:-4].lower()}-x86_64-pc-windows-msvc.exe"))
        else:
            shutil.copyfile(f, os.path.join(lib, base))
    licencas([(env, {os.path.basename(f): f for f in vistos.values()})])
    print(f"windows: {len(vistos) - len(FERRAMENTAS)} DLLs em {lib}")


# ── comum ──────────────────────────────────────────────────────────────────────────────────────────────────────────
def licencas(ambientes):
    """Os textos de licença de cada pacote conda que deu um arquivo embutido, e o LEIA-ME com versão, licença e onde
    está o código-fonte (o poppler é GPL)."""
    shutil.rmtree(LIC, ignore_errors=True)
    os.makedirs(LIC, exist_ok=True)
    usados = {}
    for env, arquivos in ambientes:
        dono = pacotes_do_ambiente(env)
        for nome, orig in arquivos.items():
            rel = os.path.relpath(os.path.realpath(orig), env).replace("\\", "/")
            m = dono.get(rel) or dono.get(os.path.relpath(orig, env).replace("\\", "/"))
            if not m:
                raise SystemExit(f"não achei o pacote de {rel}")
            usados[(m["name"], m["version"])] = m
    pkgs = os.path.join(CACHE, "mamba", "pkgs")
    linhas = []
    for (nome, versao), m in sorted(usados.items()):
        dist = m.get("fn", "").replace(".conda", "").replace(".tar.bz2", "")
        destino = os.path.join(LIC, f"{nome}-{versao}")
        achou = False
        candidatas = [os.path.join(m.get("extracted_package_dir") or "-", "info", "licenses")] + \
            glob.glob(os.path.join(pkgs, "**", dist, "info", "licenses"), recursive=True)
        # libfreetype6 traz a licença no pacote irmão freetype, da mesma versão
        irmao = re.sub(r"^lib|\d+$", "", nome)
        if irmao != nome:
            candidatas += glob.glob(os.path.join(pkgs, "**", f"{irmao}-{versao}-*", "info", "licenses"), recursive=True)
        for pasta in [c for c in candidatas if os.path.isdir(c)][:1]:
            shutil.copytree(pasta, destino, dirs_exist_ok=True)
            achou = True
        if not achou and m.get("license") == "blessing":   # o SQLite é de domínio público
            os.makedirs(destino, exist_ok=True)
            open(os.path.join(destino, "DOMINIO-PUBLICO.txt"), "w", encoding="utf-8").write(
                "O SQLite é de domínio público: https://sqlite.org/copyright.html\n")
            achou = True
        linhas.append(f"{nome} {versao} — licença: {m.get('license', '?')} — pacote: {m.get('url', '')}"
                      + ("" if achou else " (o pacote não traz o texto da licença: ver o projeto de origem)"))
    por = os.path.join(REC, "tessdata", "por.traineddata")
    os.makedirs(os.path.dirname(por), exist_ok=True)
    baixar(POR_URL, por, POR_SHA256)
    texto = LEIA_ME.format(poppler=POPPLER, tesseract=TESSERACT, lista="\n".join("  · " + l for l in linhas))
    open(os.path.join(LIC, "LEIA-ME.txt"), "w", encoding="utf-8").write(texto)
    print(f"licenças: {len(usados)} pacotes em {LIC}")


LEIA_ME = """Σκιά · Skiá — componentes de terceiros

O Skiá chama três programas livres, que vêm dentro do aplicativo e rodam no seu computador:
pdftotext e pdftoppm (do poppler {poppler}) e tesseract {tesseract}, com o modelo de português
por.traineddata (tessdata_fast 4.1.0, licença Apache-2.0). Eles são executados como processos separados;
o Skiá não os altera.

O POPPLER É SOFTWARE LIVRE SOB A GNU GPL (versão 2 ou, a seu critério, versão 3).
Código-fonte exato desta versão: https://poppler.freedesktop.org/poppler-{poppler}.tar.xz
Receita de compilação usada (conda-forge): https://github.com/conda-forge/poppler-feedstock
O tesseract (Apache-2.0): https://github.com/tesseract-ocr/tesseract/archive/refs/tags/{tesseract}.tar.gz
e https://github.com/conda-forge/tesseract-feedstock
O código-fonte de cada biblioteca abaixo está no endereço do pacote (conda-forge) e no projeto de origem.
Quem recebeu o Skiá pode pedir, a quem o distribuiu, uma cópia do código-fonte correspondente destes componentes,
pelo prazo de três anos, sem custo além do meio físico.

Os textos das licenças estão nas pastas ao lado, uma por pacote.

Pacotes embutidos (conda-forge):
{lista}
"""


if __name__ == "__main__":
    alvo = (sys.argv[1:] or [""])[0]
    if alvo == "mac":
        montar_mac()
    elif alvo == "windows":
        montar_windows()
    else:
        raise SystemExit(__doc__)
