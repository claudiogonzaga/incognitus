#!/usr/bin/env node
// Incognitus — monta do zero os sidecars (pdftotext, pdftoppm, tesseract), as bibliotecas deles, o por.traineddata e as
// licenças, para o sistema em que roda. É o `montar` do modulo/modulo.json (contrato de módulo do Graphé Desktop): um
// comando só, rodado na raiz do repositório, que serve ao Mac e ao Windows. Quem monta é o scripts/sidecars.py.
// No Windows precisa de: pip install pefile.
// Uso: node scripts/montar.mjs
import path from 'node:path';
import { fileURLToPath } from 'node:url';
import { spawnSync } from 'node:child_process';

const RAIZ = path.resolve(path.dirname(fileURLToPath(import.meta.url)), '..');
const alvo = { darwin: 'mac', win32: 'windows' }[process.platform];
if (!alvo) { console.error('montar: só há sidecars para o macOS e o Windows (aqui: ' + process.platform + ')'); process.exit(1); }
const pythons = process.platform === 'win32' ? ['python', 'py'] : ['python3', 'python'];
for (const py of pythons) {
  const r = spawnSync(py, [path.join(RAIZ, 'scripts', 'sidecars.py'), alvo], { cwd: RAIZ, stdio: 'inherit' });
  if (r.error && r.error.code === 'ENOENT') continue;   // este nome de Python não existe aqui: tenta o seguinte
  process.exit(r.status == null ? 1 : r.status);
}
console.error('montar: não achei o Python (' + pythons.join(', ') + ')');
process.exit(1);
