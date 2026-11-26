// Monta a versão web do Incognitus: o módulo (../modulo) + a camada do navegador (tauri-web.js) + as bibliotecas.
//   node web/montar.mjs [pasta de saída]      (padrão: web/dist)
// A saída é estática; no portal, fica em consciencia/incognitus/webapp/, atrás do login (functions/incognitus/webapp).
import fs from 'node:fs';
import path from 'node:path';
import { fileURLToPath } from 'node:url';

const aqui = path.dirname(fileURLToPath(import.meta.url));
const raiz = path.join(aqui, '..');
const saida = path.resolve(process.argv[2] || path.join(aqui, 'dist'));
const nm = path.join(aqui, 'node_modules');
const versao = JSON.parse(fs.readFileSync(path.join(raiz, 'package.json'), 'utf8')).version;

const copiar = (de, para) => { fs.mkdirSync(path.dirname(para), { recursive: true }); fs.copyFileSync(de, para); };
function copiarPasta(de, para, pular) {
  for (const e of fs.readdirSync(de, { withFileTypes: true })) {
    if (e.name.startsWith('.') || (pular && pular(e.name))) continue;
    const a = path.join(de, e.name), b = path.join(para, e.name);
    if (e.isDirectory()) copiarPasta(a, b, null); else copiar(a, b);
  }
}

fs.rmSync(saida, { recursive: true, force: true });
fs.mkdirSync(saida, { recursive: true });
// o módulo inteiro, menos o manifesto do Graphé Desktop
copiarPasta(path.join(raiz, 'modulo'), saida, (n) => n === 'modulo.json');
// a camada do navegador
fs.writeFileSync(path.join(saida, 'tauri-web.js'), fs.readFileSync(path.join(aqui, 'tauri-web.js'), 'utf8').replace('__VERSAO__', versao));
copiar(path.join(aqui, 'web.css'), path.join(saida, 'web.css'));
// as bibliotecas
const v = path.join(saida, 'vendor');
copiar(path.join(nm, 'pdfjs-dist/build/pdf.min.mjs'), path.join(v, 'pdfjs/pdf.min.mjs'));
copiar(path.join(nm, 'pdfjs-dist/build/pdf.worker.min.mjs'), path.join(v, 'pdfjs/pdf.worker.min.mjs'));
copiar(path.join(nm, 'pdfjs-dist/LICENSE'), path.join(v, 'pdfjs/LICENSE'));
copiar(path.join(nm, 'tesseract.js/dist/tesseract.min.js'), path.join(v, 'tesseract/tesseract.min.js'));
copiar(path.join(nm, 'tesseract.js/dist/worker.min.js'), path.join(v, 'tesseract/worker.min.js'));
for (const f of ['tesseract-core-lstm.wasm.js', 'tesseract-core-simd-lstm.wasm.js', 'tesseract-core-simd.wasm.js', 'tesseract-core.wasm.js']) {
  copiar(path.join(nm, 'tesseract.js-core', f), path.join(v, 'tesseract', f));
}
copiar(path.join(nm, 'tesseract.js/LICENSE.md'), path.join(v, 'tesseract/LICENSE.md'));
copiar(path.join(raiz, 'src-tauri/recursos/tessdata/por.traineddata'), path.join(v, 'tesseract/lang/por.traineddata'));
copiar(path.join(nm, 'jszip/dist/jszip.min.js'), path.join(v, 'jszip.min.js'));
copiar(path.join(nm, 'pdf-lib/dist/pdf-lib.min.js'), path.join(v, 'pdf-lib.min.js'));
copiar(path.join(nm, '@pdf-lib/fontkit/dist/fontkit.umd.min.js'), path.join(v, 'fontkit.umd.min.js'));
copiar(path.join(raiz, 'plugin/recursos/fontes/DejaVuSansMono.ttf'), path.join(v, 'fontes/DejaVuSansMono.ttf'));
copiar(path.join(raiz, 'plugin/recursos/fontes/LICENSE_DEJAVU'), path.join(v, 'fontes/LICENSE_DEJAVU'));

// a página: a folha da web e, antes de tudo, as bibliotecas e a camada do navegador
const pag = path.join(saida, 'index.html');
let h = fs.readFileSync(pag, 'utf8');
const antes = '<script src="vendor/quill/quill.js"></script>';
if (!h.includes(antes)) throw new Error('index.html mudou: não achei ' + antes);
h = h.replace(antes, ['vendor/jszip.min.js', 'vendor/pdf-lib.min.js', 'vendor/fontkit.umd.min.js', 'vendor/tesseract/tesseract.min.js', 'tauri-web.js']
  .map((s) => '<script src="' + s + '"></script>').join('\n') + '\n' + antes);
h = h.replace('<link rel="stylesheet" href="estilo.css">', '<link rel="stylesheet" href="estilo.css">\n<link rel="stylesheet" href="web.css">');
h = h.replace('<title>Incognitus</title>', '<title>Incognitus web</title>\n<meta name="robots" content="noindex, nofollow">');
fs.writeFileSync(pag, h);

let n = 0, bytes = 0, maior = ['', 0];
(function contar(d) { for (const e of fs.readdirSync(d, { withFileTypes: true })) { const p = path.join(d, e.name); if (e.isDirectory()) contar(p); else { const t = fs.statSync(p).size; n++; bytes += t; if (t > maior[1]) maior = [p, t]; } } })(saida);
if (maior[1] > 25 * 1024 * 1024) throw new Error('arquivo grande demais para o Cloudflare Pages: ' + maior[0]);
console.log('Incognitus web ' + versao + ': ' + n + ' arquivos, ' + (bytes / 1e6).toFixed(1) + ' MB em ' + saida);
