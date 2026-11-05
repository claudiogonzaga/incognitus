#!/usr/bin/env bash
# Σκιά · Skiá — publica uma versão nova do motor de anonimização no repositório PÚBLICO skia-motor, de onde o app a baixa
# ("Atualizar regras" e a checagem ao abrir).
#   1. extrai o motor do graphe.js atual (scripts/extrair_motor.mjs: trava + suíte de 63 casos, tem de dar "0 falhas");
#   2. confere que o motor novo passa também nos casos embutidos no app do último commit (senão os apps já instalados
#      o recusariam);
#   3. assina o manifesto com a chave privada (_segredo/motor_ed25519.pem: fora do git, nunca impressa);
#   4. publica no skia-motor: commit + tag + release com motor.js, manifesto.json, manifesto.json.sig e casos.json.
# Sem mudança no motor, não publica nada.
# Uso: scripts/publicar_motor.sh [--sem-publicar]       (SKIA_MOTOR_REPO troca o repositório; padrão abaixo)
set -euo pipefail

RAIZ="$(cd "$(dirname "$0")/.." && pwd)"
REPO="${SKIA_MOTOR_REPO:-claudiogonzaga/skia-motor}"
COPIA="$RAIZ/.ferramentas/skia-motor"
cd "$RAIZ"

echo "── 1. extrair o motor do graphe.js (trava + suíte)"
node scripts/extrair_motor.mjs

echo "── 2. os casos embutidos no app do último commit"
if git cat-file -e HEAD:src/motor/casos.json 2>/dev/null; then
  CASOS_APP="$(mktemp -d "$RAIZ/.ferramentas/casos-XXXX")/casos.json"
  git show HEAD:src/motor/casos.json > "$CASOS_APP"
  node scripts/regressao_motor.cjs motor/motor.js --casos "$CASOS_APP" | grep -E '^SKIÁ|^PARIDADE|^RESULTADO'
  rm -rf "$(dirname "$CASOS_APP")"
fi

echo "── 3. assinar"
node scripts/assinar_motor.mjs motor
cp motor/manifesto.json.sig src/motor/manifesto.json.sig

VERSAO="$(node -e "console.log(require('./motor/manifesto.json').versao)")"
SHA="$(node -e "console.log(require('./motor/manifesto.json').sha256)")"
if [ "${1:-}" = "--sem-publicar" ]; then echo "pronto (sem publicar): motor $VERSAO, sha256 ${SHA:0:12}…"; exit 0; fi

echo "── 4. publicar em $REPO"
if ! gh repo view "$REPO" >/dev/null 2>&1; then
  echo "PAROU: o repositório $REPO não existe. Ele é PÚBLICO (o código das regras fica visível a qualquer pessoa):"
  echo "       crie-o só com a confirmação do Promotor — gh repo create $REPO --public — e rode de novo."
  exit 1
fi
if [ -d "$COPIA/.git" ]; then git -C "$COPIA" pull -q --ff-only; else gh repo clone "$REPO" "$COPIA" -- -q; fi
if [ -f "$COPIA/manifesto.json" ] && [ "$(node -e "console.log(require('$COPIA/manifesto.json').sha256)")" = "$SHA" ]; then
  echo "nada a publicar: o motor $VERSAO (${SHA:0:12}…) já é o publicado"; exit 0
fi
cp motor/motor.js motor/manifesto.json motor/manifesto.json.sig motor/casos.json motor/chave_publica.txt "$COPIA/"
cp scripts/LEIA-ME-skia-motor.md "$COPIA/README.md"
TAG="v$VERSAO"; N=2
while git -C "$COPIA" rev-parse -q --verify "refs/tags/$TAG" >/dev/null || gh release view "$TAG" --repo "$REPO" >/dev/null 2>&1; do
  TAG="v$VERSAO-r$N"; N=$((N + 1))
done
git -C "$COPIA" add -A
git -C "$COPIA" commit -q -m "motor $VERSAO ($TAG) — sha256 ${SHA:0:12}…"
git -C "$COPIA" tag "$TAG"
git -C "$COPIA" push -q origin HEAD --tags
gh release create "$TAG" --repo "$REPO" --title "motor $VERSAO ($TAG)" --latest \
  --notes "Motor de anonimização $VERSAO do Graphé para o Skiá. sha256 do motor.js: $SHA. Manifesto assinado (Ed25519)." \
  "$COPIA/motor.js" "$COPIA/manifesto.json" "$COPIA/manifesto.json.sig" "$COPIA/casos.json"
echo "publicado: $REPO $TAG"
