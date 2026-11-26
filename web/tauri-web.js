// Incognitus web (26/11/2026, pedido do usuário: "a página vem do servidor e tudo roda no navegador"). O módulo do Incognitus
// (modulo/) é o mesmo do aplicativo; o que lá é do Rust (plugin/src) aqui é feito no navegador, com o mesmo contrato:
// window.__TAURI__.core.invoke(comando, argumentos), os diálogos e o arrastar e soltar.
//
// Nada sai do navegador além do que o aplicativo também manda: o texto ANONIMIZADO e o prompt à IA escolhida (com a chave
// de quem usa; o Qwen do servidor, sem chave, pela rota ia-local do portal), ao clique, e o download dos modelos de
// linguagem do Hugging Face, na primeira vez. Os PDFs, os mapas e as
// minutas ficam no armazenamento deste navegador (IndexedDB), cifrados quando são o mapa (AES-256-GCM, chave não
// exportável guardada no próprio navegador; a chave de recuperação abre os mesmos mapas no aplicativo).
//
//   - os arquivos têm caminhos de mentira ("/Arquivos/2026-11-26 14.03.12/autos.pdf"), guardados no IndexedDB;
//   - o que vai para "/Downloads/…" e o "revelar" de um arquivo viram download;
//   - pdftotext → pdf.js (com o leiaute das linhas); pdftoppm + tesseract → pdf.js + tesseract.js; o PDF de saída →
//     pdf-lib com a DejaVu Sans Mono; o .docx → JSZip.
(function () {
'use strict';
const VERSAO = '__VERSAO__';
const PREFIXO = 'plugin:anonimizacao|';
const BASE = new URL('.', document.currentScript ? document.currentScript.src : location.href).href;
const PASTA_SAIDA = 'Incognitus — anonimizados';
const PASTA_PRIVADA = 'NÃO ENVIAR À IA (nomes reais)';
const URL_FEEDBACK = 'https://docs.google.com/forms/d/e/1FAIpQLSfH2nYhDkq0rWblMwbqRr7H_-TIk0W4xOX2f2k4L9XCNJqSvA/viewform';

// ── o armazenamento: IndexedDB (arquivos e chave-valor) ──────────────────────────────────────────────────────────
const BANCO = 'incognitus-web';
let bancoP = null;
function banco() {
  if (!bancoP) bancoP = new Promise((ok, falha) => {
    const r = indexedDB.open(BANCO, 1);
    r.onupgradeneeded = () => {
      const db = r.result;
      if (!db.objectStoreNames.contains('arquivos')) db.createObjectStore('arquivos', { keyPath: 'caminho' });
      if (!db.objectStoreNames.contains('kv')) db.createObjectStore('kv');
    };
    r.onsuccess = () => ok(r.result);
    r.onerror = () => falha(new Error('o armazenamento do navegador não abriu: ' + (r.error && r.error.message)));
  });
  return bancoP;
}
async function tx(loja, modo, fn) {
  const db = await banco();
  return new Promise((ok, falha) => {
    const t = db.transaction(loja, modo), s = t.objectStore(loja);
    let res;
    Promise.resolve(fn(s)).then((r) => { res = r; });
    t.oncomplete = () => ok(res && res.result !== undefined && res instanceof IDBRequest ? res.result : res);
    t.onerror = () => falha(t.error || new Error('erro no armazenamento'));
    t.onabort = () => falha(t.error || new Error('o armazenamento recusou (cheio?)'));
  });
}
const req = (r) => new Promise((ok, falha) => { r.onsuccess = () => ok(r.result); r.onerror = () => falha(r.error); });
async function kvLer(chave) { const db = await banco(); return req(db.transaction('kv').objectStore('kv').get(chave)); }
async function kvGravar(chave, valor) { await tx('kv', 'readwrite', (s) => { s.put(valor, chave); }); }
async function kvApagar(chave) { await tx('kv', 'readwrite', (s) => { s.delete(chave); }); }

/** Um arquivo do armazenamento: { caminho, nome, pasta, blob, tam, mtime } ou undefined. */
async function arq(caminho) { const db = await banco(); return req(db.transaction('arquivos').objectStore('arquivos').get(normal(caminho))); }
async function gravarArq(caminho, dados, tipo) {
  const c = normal(caminho);
  const blob = dados instanceof Blob ? dados : new Blob([dados], { type: tipo || tipoDe(c) });
  const reg = { caminho: c, nome: nomeDe(c), pasta: paiDe(c), blob, tam: blob.size, mtime: Date.now() };
  await tx('arquivos', 'readwrite', (s) => { s.put(reg); });
  if (c.startsWith('/Downloads/')) baixar(blob, reg.nome);
  return reg;
}
async function todos() { const db = await banco(); return req(db.transaction('arquivos').objectStore('arquivos').getAll()); }
async function existe(caminho) { return !!(await arq(caminho)); }
async function ehPasta(caminho) {
  const p = normal(caminho) + '/';
  return (await todos()).some((a) => a.caminho.startsWith(p));
}

const normal = (c) => '/' + String(c || '').split('/').filter(Boolean).join('/');
const nomeDe = (c) => normal(c).split('/').pop();
const paiDe = (c) => normal(c).split('/').slice(0, -1).join('/') || '/';
const extDe = (c) => { const n = nomeDe(c), k = n.lastIndexOf('.'); return k > 0 ? n.slice(k + 1).toLowerCase() : ''; };
const semExt = (n) => { const k = n.lastIndexOf('.'); return k > 0 ? n.slice(0, k) : n; };
const juntar = (...ps) => normal(ps.join('/'));
function tipoDe(c) {
  return { pdf: 'application/pdf', txt: 'text/plain;charset=utf-8', md: 'text/markdown;charset=utf-8', json: 'application/json',
    docx: 'application/vnd.openxmlformats-officedocument.wordprocessingml.document', zip: 'application/zip' }[extDe(c)] || 'application/octet-stream';
}
const aceito = (c) => ['pdf', 'docx', 'txt'].includes(extDe(c)) && !/^(\.|~\$)/.test(nomeDe(c));

function baixar(blob, nome) {
  const u = URL.createObjectURL(blob), a = document.createElement('a');
  a.href = u; a.download = nome; a.rel = 'noopener';
  document.body.appendChild(a); a.click(); a.remove();
  setTimeout(() => URL.revokeObjectURL(u), 60000);
}

/** O nome livre na pasta: o próprio, se não existe (ou se `ehNosso`); senão "nome (2).ext"… */
async function livre(dir, base, ext, ehNosso) {
  const p = juntar(dir, base + ext);
  const a = await arq(p);
  if (!a || (ehNosso && await ehNosso(a))) return p;
  for (let n = 2; ; n++) {
    const q = juntar(dir, base + ' (' + n + ')' + ext), b = await arq(q);
    if (!b || (ehNosso && await ehNosso(b))) return q;
  }
}
function nomeSeguro(s) {
  let t = String(s || '').replace(/[\/\\:*?"<>|\u0000-\u001f]/g, '-').split(/\s+/).filter(Boolean).join(' ').replace(/^[. ]+|[. ]+$/g, '');
  t = Array.from(t).slice(0, 180).join('');
  return t || 'documento';
}

// ── texto ────────────────────────────────────────────────────────────────────────────────────────────────────────
const CP1252 = [0x20AC, 0x81, 0x201A, 0x0192, 0x201E, 0x2026, 0x2020, 0x2021, 0x02C6, 0x2030, 0x0160, 0x2039, 0x0152, 0x8D, 0x017D, 0x8F,
  0x90, 0x2018, 0x2019, 0x201C, 0x201D, 0x2022, 0x2013, 0x2014, 0x02DC, 0x2122, 0x0161, 0x203A, 0x0153, 0x9D, 0x017E, 0x0178];
function decodificar(buf) {
  let b = new Uint8Array(buf);
  if (b[0] === 0xEF && b[1] === 0xBB && b[2] === 0xBF) b = b.subarray(3);
  try { return new TextDecoder('utf-8', { fatal: true }).decode(b); }
  catch (e) { return Array.from(b, (c) => String.fromCharCode(c >= 0x80 && c < 0xA0 ? CP1252[c - 0x80] : c)).join(''); }
}
async function lerTextoBruto(caminho) {
  const a = await arq(caminho);
  if (!a) throw 'não achei o arquivo ' + nomeDe(caminho) + ' (foi apagado do navegador?)';
  if (a.tam > 64 * 1024 * 1024) throw 'arquivo grande demais';
  return decodificar(await a.blob.arrayBuffer());
}

// ── .docx (JSZip) ────────────────────────────────────────────────────────────────────────────────────────────────
function parteComTexto(nome) {
  const n = nome.toLowerCase();
  if (!n.startsWith('word/') || !n.endsWith('.xml')) return false;
  const f = n.slice(5, -4);
  if (f.includes('/')) return false;
  return ['document', 'footnotes', 'endnotes', 'comments'].includes(f) || /^header\d*$/.test(f) || /^footer\d*$/.test(f);
}
async function docxPartes(caminho) {
  const a = await arq(caminho);
  if (!a) throw 'não achei o arquivo ' + nomeDe(caminho);
  let z;
  try { z = await JSZip.loadAsync(await a.blob.arrayBuffer()); } catch (e) { throw 'não parece um .docx (o arquivo não é um zip)'; }
  const v = [];
  for (const nome of Object.keys(z.files)) if (parteComTexto(nome) && !z.files[nome].dir) v.push({ nome, xml: await z.files[nome].async('string') });
  if (!v.some((x) => x.nome.toLowerCase() === 'word/document.xml')) throw 'não parece um .docx (sem word/document.xml)';
  return v;
}
const desescapar = (s) => s.replace(/&lt;/g, '<').replace(/&gt;/g, '>').replace(/&quot;/g, '"').replace(/&apos;/g, "'").replace(/&amp;/g, '&');
function xmlParaTexto(xml) {
  let out = '', emTexto = false, i = 0;
  while (true) {
    const k = xml.indexOf('<', i);
    if (k < 0) break;
    if (emTexto) out += desescapar(xml.slice(i, k));
    const f = xml.indexOf('>', k);
    if (f < 0) break;
    const tag = xml.slice(k + 1, f), fecha = tag.startsWith('/'), vazia = tag.endsWith('/');
    const nome = tag.replace(/^\//, '').split(/[\s/]/)[0];
    if (nome === 'w:t') emTexto = !fecha && !vazia;
    else if (nome === 'w:p' && fecha) out += '\n';
    else if (nome === 'w:br' || nome === 'w:cr') out += '\n';
    else if (nome === 'w:tab' && !fecha) out += '\t';
    i = f + 1;
  }
  return out;
}
async function docxTexto(caminho) {
  const ps = await docxPartes(caminho);
  return xmlParaTexto(ps.find((x) => x.nome.toLowerCase() === 'word/document.xml').xml);
}
async function docxGravarPartes(origem, destino, partes) {
  const a = await arq(origem);
  const z = await JSZip.loadAsync(await a.blob.arrayBuffer());
  for (const p of partes) z.file(p.nome, p.xml);
  const blob = await z.generateAsync({ type: 'blob', compression: 'DEFLATE', mimeType: tipoDe('x.docx') });
  return gravarArq(destino, blob);
}

const DOCX_TIPOS = '<?xml version="1.0" encoding="UTF-8" standalone="yes"?>\n<Types xmlns="http://schemas.openxmlformats.org/package/2006/content-types"><Default Extension="rels" ContentType="application/vnd.openxmlformats-package.relationships+xml"/><Default Extension="xml" ContentType="application/xml"/><Override PartName="/word/document.xml" ContentType="application/vnd.openxmlformats-officedocument.wordprocessingml.document.main+xml"/><Override PartName="/word/styles.xml" ContentType="application/vnd.openxmlformats-officedocument.wordprocessingml.styles+xml"/></Types>';
const DOCX_RELS = '<?xml version="1.0" encoding="UTF-8" standalone="yes"?>\n<Relationships xmlns="http://schemas.openxmlformats.org/package/2006/relationships"><Relationship Id="rId1" Type="http://schemas.openxmlformats.org/officeDocument/2006/relationships/officeDocument" Target="word/document.xml"/></Relationships>';
const DOCX_RELS_DOC = '<?xml version="1.0" encoding="UTF-8" standalone="yes"?>\n<Relationships xmlns="http://schemas.openxmlformats.org/package/2006/relationships"><Relationship Id="rId1" Type="http://schemas.openxmlformats.org/officeDocument/2006/relationships/styles" Target="styles.xml"/></Relationships>';
const DOCX_ESTILOS = '<?xml version="1.0" encoding="UTF-8" standalone="yes"?>\n<w:styles xmlns:w="http://schemas.openxmlformats.org/wordprocessingml/2006/main"><w:docDefaults><w:rPrDefault><w:rPr><w:rFonts w:ascii="Times New Roman" w:hAnsi="Times New Roman" w:cs="Times New Roman" w:eastAsia="Times New Roman"/><w:sz w:val="24"/><w:szCs w:val="24"/><w:lang w:val="pt-BR"/></w:rPr></w:rPrDefault><w:pPrDefault><w:pPr><w:spacing w:after="120" w:line="360" w:lineRule="auto"/></w:pPr></w:pPrDefault></w:docDefaults><w:style w:type="paragraph" w:default="1" w:styleId="Normal"><w:name w:val="Normal"/><w:qFormat/></w:style></w:styles>';
const escaparXml = (s) => Array.from(String(s)).filter((c) => c === '\t' || c === '\n' || c >= ' ').join('').replace(/&/g, '&amp;').replace(/</g, '&lt;').replace(/>/g, '&gt;');
function trechoXml(t, negrito, tamanho) {
  if (!t.t) return '';
  let pr = '';
  if (t.b || negrito) pr += '<w:b/><w:bCs/>';
  if (t.i) pr += '<w:i/><w:iCs/>';
  if (t.u) pr += '<w:u w:val="single"/>';
  if (tamanho) pr += '<w:sz w:val="' + tamanho + '"/><w:szCs w:val="' + tamanho + '"/>';
  if (t.m) pr += '<w:highlight w:val="yellow"/>';
  let miolo = '';
  String(t.t).replace(/\r\n?/g, '\n').split('\n').forEach((linha, k) => {
    if (k > 0) miolo += '<w:br/>';
    linha.split('\t').forEach((parte, j) => {
      if (j > 0) miolo += '<w:tab/>';
      if (parte) miolo += '<w:t xml:space="preserve">' + escaparXml(parte) + '</w:t>';
    });
  });
  return '<w:r>' + (pr ? '<w:rPr>' + pr + '</w:rPr>' : '') + miolo + '</w:r>';
}
function documentoXml(paragrafos) {
  let corpo = '', numero = 0;
  for (const p of paragrafos || []) {
    const estilo = p.estilo || '';
    numero = estilo === 'ol' ? numero + 1 : 0;
    const jc = { center: 'center', right: 'right', justify: 'both' }[p.alinhamento] || 'left';
    const recuo = Math.min(p.recuo || 0, 8) * 720;
    let ind = '<w:ind w:left="' + recuo + '"/>', negrito = false, tamanho = null, antes = 0;
    if (estilo === 'h1') { negrito = true; tamanho = 28; antes = 240; }
    else if (estilo === 'h2') { negrito = true; tamanho = 26; antes = 200; }
    else if (estilo === 'h3') { negrito = true; antes = 160; }
    else if (estilo === 'li' || estilo === 'ol') ind = '<w:ind w:left="' + (recuo + 720) + '" w:hanging="360"/>';
    else if (estilo === 'quote') { ind = '<w:ind w:left="' + (recuo + 2268) + '"/>'; tamanho = 22; }
    const espaco = estilo === 'quote' ? '<w:spacing w:after="120" w:line="240" w:lineRule="auto"/>' : antes ? '<w:spacing w:before="' + antes + '" w:after="120"/>' : '';
    const titulo = /^h[123]$/.test(estilo) ? '<w:keepNext/>' : '';
    corpo += '<w:p><w:pPr>' + titulo + espaco + ind + '<w:jc w:val="' + jc + '"/></w:pPr>';
    if (estilo === 'li') corpo += trechoXml({ t: '•\t' });
    if (estilo === 'ol') corpo += trechoXml({ t: numero + '.\t' });
    for (const t of p.trechos || []) corpo += trechoXml(t, negrito, tamanho);
    corpo += '</w:p>';
  }
  return '<?xml version="1.0" encoding="UTF-8" standalone="yes"?>\n<w:document xmlns:w="http://schemas.openxmlformats.org/wordprocessingml/2006/main"><w:body>' + corpo +
    '<w:sectPr><w:pgSz w:w="11906" w:h="16838"/><w:pgMar w:top="1701" w:right="1134" w:bottom="1134" w:left="1701" w:header="709" w:footer="709" w:gutter="0"/></w:sectPr></w:body></w:document>';
}
async function montarDocx(paragrafos) {
  const z = new JSZip();
  z.file('[Content_Types].xml', DOCX_TIPOS); z.file('_rels/.rels', DOCX_RELS); z.file('word/document.xml', documentoXml(paragrafos));
  z.file('word/_rels/document.xml.rels', DOCX_RELS_DOC); z.file('word/styles.xml', DOCX_ESTILOS);
  return z.generateAsync({ type: 'blob', compression: 'DEFLATE', mimeType: tipoDe('x.docx') });
}

// ── o PDF: leitura (pdf.js), OCR (tesseract.js) e gravação (pdf-lib) ─────────────────────────────────────────────
let pdfjsP = null;
function pdfjs() {
  if (!pdfjsP) pdfjsP = import(BASE + 'vendor/pdfjs/pdf.min.mjs').then((m) => { m.GlobalWorkerOptions.workerSrc = BASE + 'vendor/pdfjs/pdf.worker.min.mjs'; return m; });
  return pdfjsP;
}
const preparados = new Map();   // id → { bytes, doc }
let contador = 0;
async function docDe(id) {
  const p = preparados.get(id);
  if (!p) throw 'arquivo desconhecido (já limpo?)';
  if (!p.doc) {
    const m = await pdfjs();
    p.doc = await m.getDocument({ data: p.bytes.slice(0), isEvalSupported: false, disableFontFace: true, useSystemFonts: false }).promise;
  }
  return p.doc;
}

/** O texto de uma página, com o leiaute das linhas (como o pdftotext -layout): agrupa por linha, ordena por x e põe os
 *  espaços pela distância (a largura média de um caractere da própria linha). */
function leiaute(itens, largura) {
  const ps = itens.filter((it) => it.str && it.str.trim() !== '' || (it.str === ' ')).map((it) => {
    const [a, b, , d, e, f] = it.transform;
    const tam = Math.hypot(a, b) || Math.abs(d) || 10;
    return { s: it.str, x: e, y: f, w: it.width || 0, h: tam };
  });
  if (!ps.length) return '';
  ps.sort((p, q) => q.y - p.y || p.x - q.x);
  const linhas = [];
  for (const p of ps) {
    const l = linhas.find((x) => Math.abs(x.y - p.y) <= Math.max(2, Math.min(x.h, p.h) * 0.45));
    if (l) { l.ps.push(p); l.h = Math.max(l.h, p.h); } else linhas.push({ y: p.y, h: p.h, ps: [p] });
  }
  linhas.sort((a, b) => b.y - a.y);
  // a largura de um caractere: a mediana das larguras por caractere dos pedaços da página
  const cws = ps.filter((p) => p.w > 0 && p.s.length).map((p) => p.w / p.s.length).sort((a, b) => a - b);
  const cw = Math.max(2, cws.length ? cws[Math.floor(cws.length / 2)] : 5);
  const minX = Math.min(...ps.map((p) => p.x));
  const out = [];
  let yAnt = null, hAnt = 12;
  for (const l of linhas) {
    if (yAnt !== null) {
      // uma linha em branco para cada linha que caberia no vão (como o pdftotext -layout), no máximo duas
      const vazias = Math.min(2, Math.round((yAnt - l.y) / Math.max(8, hAnt * 1.5)) - 1);
      for (let k = 0; k < vazias; k++) out.push('');
    }
    l.ps.sort((a, b) => a.x - b.x);
    let s = '', fim = minX;
    for (const p of l.ps) {
      const col = Math.round((p.x - minX) / cw);
      if (s.length && p.x - fim > cw * 0.25) s += ' ';
      if (col > s.length) s += ' '.repeat(col - s.length);
      s += p.s;
      fim = p.x + p.w;
    }
    out.push(s.replace(/\s+$/, ''));
    yAnt = l.y; hAnt = l.h;
  }
  return out.join('\n');
}
async function extrairTexto(id) {
  const doc = await docDe(id), partes = [];
  for (let n = 1; n <= doc.numPages; n++) {
    const pg = await doc.getPage(n);
    const tc = await pg.getTextContent({ includeMarkedContent: false, disableNormalization: false });
    const t = leiaute(tc.items, pg.view[2]);
    partes.push(t ? t + '\n\f' : '\f');   // como o pdftotext: a página termina em \f; a vazia é só o \f
    pg.cleanup();
  }
  return partes.join('');
}

let ocrP = null;
function ocr() {
  if (!ocrP) ocrP = (async () => {
    const w = await Tesseract.createWorker('por', 1, {
      workerPath: BASE + 'vendor/tesseract/worker.min.js', corePath: BASE + 'vendor/tesseract/', langPath: BASE + 'vendor/tesseract/lang',
      gzip: false, cacheMethod: 'none', logger: () => {},
    });
    return w;
  })().catch((e) => { ocrP = null; throw e; });
  return ocrP;
}
let filaOcr = Promise.resolve();
async function ocrPagina(id, pagina) {
  const doc = await docDe(id);
  const pg = await doc.getPage(pagina);
  const vp = pg.getViewport({ scale: 200 / 72 });
  const c = document.createElement('canvas');
  c.width = Math.ceil(vp.width); c.height = Math.ceil(vp.height);
  const ctx = c.getContext('2d', { willReadFrequently: false });
  ctx.fillStyle = '#fff'; ctx.fillRect(0, 0, c.width, c.height);
  await pg.render({ canvasContext: ctx, viewport: vp }).promise;
  pg.cleanup();
  // um OCR por vez (o worker do tesseract.js é um só): os pedidos entram na fila
  const vez = filaOcr.then(async () => (await (await ocr()).recognize(c)).data.text);
  filaOcr = vez.catch(() => {});
  const t = await vez;
  c.width = c.height = 0;
  return t;
}

let fonteP = null;
const fonte = () => (fonteP || (fonteP = fetch(BASE + 'vendor/fontes/DejaVuSansMono.ttf').then((r) => r.arrayBuffer())));
const TAMANHO = 11, ENTRELINHA = 13.2, MARGEM = 36, A4 = [595.28, 841.89], COLUNAS_CABECALHO = 78, COLUNAS_MAX = 160;
function quebrar(linha, n, naPalavra) {
  const cs = Array.from(linha);
  if (cs.length <= n) return [linha];
  const out = [];
  let ini = 0;
  while (cs.length - ini > n) {
    let fim = ini + n;
    if (naPalavra) for (let k = ini + n - 1; k >= ini + Math.floor(n / 2); k--) if (cs[k] === ' ') { fim = k + 1; break; }
    out.push(cs.slice(ini, fim).join('').replace(/\s+$/, ''));
    ini = fim;
  }
  out.push(cs.slice(ini).join(''));
  return out;
}
function limpar(linha) {
  let s = '';
  for (const c of linha) {
    if (c === '\t') s += ' '.repeat(8 - (Array.from(s).length % 8));
    else if (/[\u0000-\u001f\u007f-\u009f]/.test(c)) s += ' ';
    else s += c;
  }
  return s.replace(/\s+$/, '');
}
function linhasDaPagina(texto) {
  const ls = String(texto).split('\n').map(limpar);
  const ini = ls.length && ls[0].startsWith('===== P') ? 1 : 0;
  const recuos = ls.slice(ini).filter((l) => l).map((l) => l.length - l.replace(/^ +/, '').length);
  const recuo = recuos.length ? Math.min(...recuos) : 0;
  if (recuo > 0) for (let k = ini; k < ls.length; k++) ls[k] = Array.from(ls[k]).slice(recuo).join('');
  return ls.flatMap((l) => quebrar(l, COLUNAS_MAX, false));
}
async function gerarPdf(cabecalho, paginas, produtor) {
  const { PDFDocument } = PDFLib;
  const pdf = await PDFDocument.create();
  pdf.registerFontkit(fontkit);
  const f = await pdf.embedFont(await fonte(), { subset: true });
  const folhas = [String(cabecalho).split('\n').map(limpar).flatMap((l) => quebrar(l, COLUNAS_CABECALHO, true))].concat(paginas.map(linhasDaPagina));
  const largura = f.widthOfTextAtSize('M', TAMANHO), asc = f.heightAtSize(TAMANHO, { descender: false });
  const temGlifo = (ch) => { try { return f.embedder.font.hasGlyphForCodePoint(ch.codePointAt(0)); } catch (e) { return true; } };
  let semGlifo = 0;
  const vistos = new Map();
  for (const linhas of folhas) {
    const colunas = Math.max(0, ...linhas.map((l) => Array.from(l).length));
    const w = Math.max(A4[0], 2 * MARGEM + colunas * largura), h = Math.max(A4[1], 2 * MARGEM + linhas.length * ENTRELINHA);
    const pg = pdf.addPage([w, h]);
    let y = h - MARGEM - asc;
    for (const l of linhas) {
      if (l) {
        const t = Array.from(l).map((ch) => {
          if (!vistos.has(ch)) { const ok = temGlifo(ch); vistos.set(ch, ok); if (!ok) semGlifo++; }
          return vistos.get(ch) ? ch : '?';
        }).join('');
        pg.drawText(t, { x: MARGEM, y, size: TAMANHO, font: f });
      }
      y -= ENTRELINHA;
    }
  }
  pdf.setProducer(produtor); pdf.setCreator('Incognitus web');
  return { bytes: await pdf.save(), semGlifo };
}

// ── o cofre: o mapa cifrado (AES-256-GCM, o mesmo envelope do aplicativo) ───────────────────────────────────────
const MARCA = 'incognitus_cofre', DADO_AUTENTICADO = new TextEncoder().encode('incognitus-cofre-1');
const AVISO = 'Mapa do Incognitus, cifrado (AES-256-GCM). Abre no Incognitus deste navegador, ou em outro com a chave de recuperação (Configurações, Mapa cifrado).';
const hex = (b) => Array.from(new Uint8Array(b), (x) => x.toString(16).padStart(2, '0')).join('');
const b64 = (b) => { let s = ''; new Uint8Array(b).forEach((x) => { s += String.fromCharCode(x); }); return btoa(s); };
const deB64 = (s) => Uint8Array.from(atob(s), (c) => c.charCodeAt(0));
async function apelido(cru) { return hex(await crypto.subtle.digest('SHA-256', cru)).slice(0, 8); }
const emGrupos = (cru) => hex(cru).toUpperCase().match(/.{4}/g).join('-');
function deGrupos(texto) {
  const limpo = String(texto).replace(/[^0-9a-fA-F]/g, '');
  if (limpo.length !== 64 || /[^0-9a-fA-F\-\s]/.test(texto)) throw 'Isso não parece a chave de recuperação: são 16 grupos de 4 letras e números, separados por traço.';
  return Uint8Array.from(limpo.match(/.{2}/g), (x) => parseInt(x, 16));
}
// A chave fica no IndexedDB como CryptoKey. A cópia em bytes (para a chave de recuperação) também, porque o navegador não
// tem chaveiro do sistema; quem tem acesso a este navegador, no seu usuário, tem acesso ao mapa, como no aplicativo.
async function chaves(criar) {
  let c = await kvLer('cofre');
  if (!c && criar) {
    const cru = crypto.getRandomValues(new Uint8Array(32));
    c = { cru, antigas: [] };
    await kvGravar('cofre', c);
  }
  return c || null;
}
const chaveCripto = (cru) => crypto.subtle.importKey('raw', cru, 'AES-GCM', false, ['encrypt', 'decrypt']);
const ehCofre = (t) => { const s = String(t || '').replace(/^﻿/, '').trimStart(); return s.startsWith('{') && s.slice(0, 80).includes(MARCA); };
async function selar(claro) {
  const c = await chaves(true), n = crypto.getRandomValues(new Uint8Array(12));
  const dados = await crypto.subtle.encrypt({ name: 'AES-GCM', iv: n, additionalData: DADO_AUTENTICADO }, await chaveCripto(c.cru), new TextEncoder().encode(claro));
  return '{\n "' + MARCA + '": 1,\n "aviso": ' + JSON.stringify(AVISO) + ',\n "chave": "' + await apelido(c.cru) + '",\n "nonce": "' + b64(n) + '",\n "dados": "' + b64(dados) + '"\n}\n';
}
async function abrir(envelope) {
  let e;
  try { e = JSON.parse(envelope); } catch (x) { throw 'mapa cifrado ilegível'; }
  const c = await chaves(false);
  if (!c) throw 'Este mapa está cifrado e este navegador ainda não tem a chave: importe a chave de recuperação nas Configurações (Mapa cifrado).';
  for (const cru of [c.cru].concat(c.antigas || [])) {
    if (e.chave && e.chave !== await apelido(cru)) continue;
    try {
      const claro = await crypto.subtle.decrypt({ name: 'AES-GCM', iv: deB64(e.nonce), additionalData: DADO_AUTENTICADO }, await chaveCripto(cru), deB64(e.dados));
      return new TextDecoder().decode(claro);
    } catch (x) { /* a próxima */ }
  }
  throw 'Este mapa foi cifrado com outra chave (' + (e.chave || '?') + '): importe a chave de recuperação daquele computador nas Configurações (Mapa cifrado).';
}

// As chaves das IAs: cifradas com a chave do cofre (não ficam em claro no armazenamento).
async function lerChaveIa(prov) {
  const s = await kvLer('ia-chave-' + (prov || 'anthropic'));
  if (!s) return null;
  try { return await abrir(s); } catch (e) { return null; }
}

// ── os modelos de linguagem (Hugging Face), guardados no Cache Storage ───────────────────────────────────────────
const MODELOS = {
  ner: { nome: 'anonimato-ner-ptbr', repo: 'inples-tech/anonimato-ner-ptbr', revisao: 'e55b0a26470a8525387ad9172e1dab24c390e80b',
    arquivos: [['model.onnx', 'model.onnx', 109515171], ['tokenizer.json', 'tokenizer.json', 677790]],
    rotulos: ['O', 'B-PESSOA', 'I-PESSOA', 'B-ORG', 'I-ORG', 'B-LOCAL', 'I-LOCAL', 'B-ENDERECO', 'I-ENDERECO'] },
  busca: { nome: 'multilingual-e5-small', repo: 'Xenova/multilingual-e5-small', revisao: '761b726dd34fb83930e26aab4e9ac3899aa1fa78',
    arquivos: [['onnx/model_quantized.onnx', 'model.onnx', 118308185], ['tokenizer.json', 'tokenizer.json', 17082730]], rotulos: [] },
};
const urlHf = (m, a) => 'https://huggingface.co/' + m.repo + '/resolve/' + m.revisao + '/' + a[0];
async function estadoModelo(m) {
  let baixado = false;
  try { const c = await caches.open('incognitus-modelos'); baixado = (await Promise.all(m.arquivos.map((a) => c.match(urlHf(m, a))))).every(Boolean); } catch (e) { baixado = false; }
  return { baixado, nome: m.nome, megas: Math.round(m.arquivos.reduce((s, a) => s + a[2], 0) / 1e6), rotulos: m.rotulos };
}
async function baixarModelo(m) {
  const c = await caches.open('incognitus-modelos');
  for (const a of m.arquivos) {
    const u = urlHf(m, a);
    if (await c.match(u)) continue;
    const r = await fetch(u);
    if (!r.ok) throw 'o download de ' + a[0] + ' respondeu ' + r.status;
    await c.put(u, r);
  }
  return estadoModelo(m);
}
async function lerModelo(m, arquivo) {
  const a = m.arquivos.find((x) => x[1] === arquivo);
  if (!a) throw 'arquivo desconhecido: ' + arquivo;
  const c = await caches.open('incognitus-modelos'), r = await c.match(urlHf(m, a));
  if (!r) throw 'o modelo ainda não foi baixado';
  return r.arrayBuffer();
}

// ── a IA (fetch direto; a Anthropic por streaming, para pedidos longos) ──────────────────────────────────────────
const URLS = { openai: 'https://api.openai.com/v1/chat/completions', deepinfra: 'https://api.deepinfra.com/v1/openai/chat/completions' };
async function lerErro(r) {
  let t = '';
  try { t = await r.text(); } catch (e) { /* nada */ }
  let msg = t;
  try { const j = JSON.parse(t); msg = (j.error && (j.error.message || j.error)) || j.message || t; } catch (e) { /* texto */ }
  return 'A IA respondeu ' + r.status + ': ' + String(msg).slice(0, 400);
}
async function anthropicStream(chave, corpo, cabecalhos) {
  const pedido = Object.assign({}, corpo, { stream: true });
  const r = await fetch('https://api.anthropic.com/v1/messages', {
    method: 'POST',
    headers: Object.assign({ 'content-type': 'application/json', 'x-api-key': chave, 'anthropic-version': '2023-06-01',
      'anthropic-dangerous-direct-browser-access': 'true' }, cabecalhos || {}),
    body: JSON.stringify(pedido),
  });
  if (!r.ok) throw await lerErro(r);
  const leitor = r.body.getReader(), dec = new TextDecoder();
  let buf = '', msg = null;
  const blocos = [], parciais = {};
  const tratar = (ev) => {
    if (ev.type === 'message_start') { msg = ev.message; msg.content = []; }
    else if (ev.type === 'content_block_start') { blocos[ev.index] = Object.assign({}, ev.content_block); if (ev.content_block.type === 'tool_use') parciais[ev.index] = ''; }
    else if (ev.type === 'content_block_delta') {
      const b = blocos[ev.index], d = ev.delta;
      if (d.type === 'text_delta') b.text = (b.text || '') + d.text;
      else if (d.type === 'input_json_delta') parciais[ev.index] += d.partial_json;
      else if (d.type === 'thinking_delta') b.thinking = (b.thinking || '') + d.thinking;
      else if (d.type === 'signature_delta') b.signature = d.signature;
    } else if (ev.type === 'content_block_stop') {
      const b = blocos[ev.index];
      if (b && b.type === 'tool_use') { try { b.input = parciais[ev.index] ? JSON.parse(parciais[ev.index]) : {}; } catch (e) { b.input = {}; } }
    } else if (ev.type === 'message_delta') {
      Object.assign(msg, ev.delta || {});
      msg.usage = Object.assign(msg.usage || {}, ev.usage || {});
    } else if (ev.type === 'error') throw 'A Anthropic devolveu um erro no meio da resposta: ' + ((ev.error && ev.error.message) || 'sem detalhe');
  };
  while (true) {
    const { value, done } = await leitor.read();
    if (done) break;
    buf += dec.decode(value, { stream: true });
    let k;
    while ((k = buf.indexOf('\n\n')) >= 0) {
      const ev = buf.slice(0, k); buf = buf.slice(k + 2);
      const dados = ev.split('\n').filter((l) => l.startsWith('data:')).map((l) => l.slice(5).trim()).join('');
      if (dados) tratar(JSON.parse(dados));
    }
  }
  if (!msg) throw 'A Anthropic fechou a conexão sem resposta.';
  msg.content = blocos.filter(Boolean);
  return msg;
}
// O Qwen do servidor (26/11/2026, pedido do usuário: "usar o Qwen deste Mac"): pela rota da função do portal
// (/incognitus/webapp/ia-local), que confere a sessão de convidado e repassa ao túnel com o segredo. O navegador não vê
// nem o segredo nem o endereço do servidor. Só o pedido já conferido pelo porteiro da janela (o texto anonimizado) vai;
// o mapa nunca. A resposta vem do Ollama em NDJSON, aos pedaços (as linhas em branco são o sinal de vida do porteiro do
// servidor), e volta no formato da OpenAI, como a do aplicativo.
const URL_LOCAL = BASE + 'ia-local/api/';
async function respostaLocal(r) {
  if (r.status === 401 || r.status === 403) throw 'A sessão do portal expirou: recarregue a página e entre de novo.';
  if (!r.ok) throw (await lerErro(r)).replace('A IA', 'O Qwen do servidor');
  if (!/json/.test(r.headers.get('content-type') || '')) throw 'A sessão do portal expirou: recarregue a página e entre de novo.';
  return r;
}
async function iaLocal(corpo, t0) {
  let r;
  try {
    r = await fetch(URL_LOCAL + 'chat', { method: 'POST', credentials: 'same-origin', headers: { 'content-type': 'application/json' }, body: corpo });
  } catch (e) { throw 'O Qwen do servidor não respondeu (' + ((e && e.message) || e) + '). Confira a internet e tente de novo.'; }
  await respostaLocal(r);
  const leitor = r.body.getReader(), dec = new TextDecoder(), chamadas = [];
  let buf = '', texto = '', fim = null, modelo = '', ent = 0, sai = 0, feito = false;
  const linha = (l) => {
    l = l.trim();
    if (!l) return;   // o sinal de vida
    let j;
    try { j = JSON.parse(l); } catch (e) { throw 'O Qwen do servidor mandou um pedaço ilegível da resposta.'; }
    if (j.error) throw 'O Qwen do servidor interrompeu a resposta: ' + String(j.error.message || j.error).slice(0, 300);
    if (j.model) modelo = j.model;
    const m = j.message || {};
    if (m.content) texto += m.content;   // o raciocínio (m.thinking) não entra
    (m.tool_calls || []).forEach((t) => {
      const f = t.function || {};
      chamadas.push({ id: 'call_' + (chamadas.length + 1), type: 'function', function: { name: f.name || '', arguments: typeof f.arguments === 'string' ? f.arguments : JSON.stringify(f.arguments || {}) } });
    });
    if (j.done) { feito = true; fim = j.done_reason === 'length' ? 'length' : chamadas.length ? 'tool_calls' : 'stop'; ent = j.prompt_eval_count || 0; sai = j.eval_count || 0; }
  };
  while (true) {
    const { value, done } = await leitor.read();
    if (done) break;
    buf += dec.decode(value, { stream: true });
    let k;
    while ((k = buf.indexOf('\n')) >= 0) { const l = buf.slice(0, k); buf = buf.slice(k + 1); linha(l); }
  }
  linha(buf + dec.decode());
  if (!feito) throw 'A resposta do Qwen do servidor veio cortada: a conexão terminou antes do fim. Tente de novo.';
  const msg = { role: 'assistant', content: texto || null };
  if (chamadas.length) msg.tool_calls = chamadas;
  return { json: { model: modelo, choices: [{ index: 0, message: msg, finish_reason: fim }], usage: { prompt_tokens: ent, completion_tokens: sai } },
    ms: Date.now() - t0, code: 200, tentativas: 1, quedas: 0 };
}
async function modelosLocal() {
  let r;
  try { r = await fetch(URL_LOCAL + 'tags', { credentials: 'same-origin' }); } catch (e) { throw 'O Qwen do servidor não respondeu (' + ((e && e.message) || e) + ').'; }
  await respostaLocal(r);
  return ((await r.json()).models || []).map((m) => m.name).filter(Boolean).sort();
}

async function iaEnviar({ corpo, cabecalhos, provedor }) {
  if (provedor === 'local') return iaLocal(corpo, Date.now());
  const prov = provedor || 'anthropic', chave = await lerChaveIa(prov), t0 = Date.now();
  if (!chave) throw 'Falta a chave ' + ({ anthropic: 'da Anthropic', openai: 'da OpenAI', gemini: 'do Google (Gemini)', deepinfra: 'da DeepInfra' }[prov] || prov) + '.';
  const pedido = JSON.parse(corpo);
  let json;
  if (prov === 'anthropic') {
    const cab = Object.assign({}, cabecalhos || {});
    delete cab['anthropic-version'];
    json = await anthropicStream(chave, pedido, cab);
  } else if (prov === 'gemini') {
    const modelo = pedido.model; delete pedido.model;
    const r = await fetch('https://generativelanguage.googleapis.com/v1beta/models/' + encodeURIComponent(modelo) + ':generateContent', {
      method: 'POST', headers: { 'content-type': 'application/json', 'x-goog-api-key': chave }, body: JSON.stringify(pedido) });
    if (!r.ok) throw await lerErro(r);
    json = await r.json();
  } else {
    const r = await fetch(URLS[prov], { method: 'POST', headers: { 'content-type': 'application/json', authorization: 'Bearer ' + chave }, body: JSON.stringify(pedido) });
    if (!r.ok) throw await lerErro(r);
    json = await r.json();
  }
  return { json, ms: Date.now() - t0, code: 200, tentativas: 1, quedas: 0 };
}

// ── o Google Docs (26/11/2026, pedido do usuário: "se a pessoa logou com o Google, ter a opção de fazer tudo no Google
// Docs") ───────────────────────────────────────────────────────────────────────────────────────────────────────────
// A permissão drive.file (só os arquivos que o próprio Incognitus criar), pedida no navegador pelo Google Identity
// Services, no clique; o acesso fica só na memória desta aba (cerca de 1 hora). O ID do cliente OAuth vem do portal
// (/incognitus/webapp/google-cliente); sem ele, o Google Docs não aparece. A minuta (com os nomes reais) só sobe no
// clique, depois da confirmação, e vai direto do navegador para o Drive de quem usa.
const ESCOPO_ARQUIVOS = 'https://www.googleapis.com/auth/drive.file';
const google = { cliente: null, lido: false, acesso: null, vale: 0, email: '' };
async function clienteGoogle() {
  if (!google.lido) {
    try { const r = await fetch(BASE + 'google-cliente', { credentials: 'same-origin' }); if (r.ok) google.cliente = (await r.json()).cliente || null; } catch (e) { google.cliente = null; }
    google.lido = true;
  }
  return google.cliente;
}
let gisP = null;
const gis = () => (gisP || (gisP = new Promise((ok, falha) => {
  const sc = document.createElement('script');
  sc.src = 'https://accounts.google.com/gsi/client'; sc.async = true;
  sc.onload = () => ok(window.google.accounts.oauth2);
  sc.onerror = () => { gisP = null; falha('Não consegui carregar o login do Google.'); };
  document.head.appendChild(sc);
})));
async function googleEntrar() {
  const cliente = await clienteGoogle();
  if (!cliente) throw 'O Google Docs ainda não está disponível nesta página.';
  const oauth2 = await gis();
  const t = await new Promise((ok, falha) => {
    const tc = oauth2.initTokenClient({ client_id: cliente, scope: ESCOPO_ARQUIVOS, callback: (r) => (r && r.access_token ? ok(r) : falha('O Google não deu a permissão.')),
      error_callback: (e) => falha(e && e.type === 'popup_closed' ? 'A janela do Google foi fechada antes de dar a permissão.' : e && e.type === 'popup_failed_to_open' ? 'O navegador barrou a janela do Google: libere os pop-ups deste site e tente de novo.' : 'O login do Google falhou.') });
    tc.requestAccessToken({ prompt: '' });
  });
  if (!oauth2.hasGrantedAllScopes(t, ESCOPO_ARQUIVOS)) throw 'O Google não deu a permissão de criar documentos no Drive: marque a caixa dessa permissão.';
  google.acesso = t.access_token; google.vale = Date.now() + (Math.max(60, +t.expires_in || 3600) - 60) * 1000;
  return { email: '' };
}
const acessoVale = () => !!google.acesso && Date.now() < google.vale;
async function googleDocsEnviar({ caminho, existente }) {
  const a = await arq(caminho);
  if (!a || !/\.docx$/i.test(a.nome)) throw 'só mando ao Google Docs um .docx que já foi gravado';
  if (existente && /^[A-Za-z0-9_-]{10,128}$/.test(existente)) {
    const link = 'https://docs.google.com/document/d/' + existente + '/edit';
    window.open(link, '_blank', 'noopener');
    return { id: existente, link, feito: 'aberto' };
  }
  if (!acessoVale()) throw 'O acesso ao Google venceu: clique de novo para dar a permissão.';
  const fronteira = 'incognitus' + Math.random().toString(36).slice(2);
  const meta = JSON.stringify({ name: a.nome.replace(/\.docx$/i, ''), mimeType: 'application/vnd.google-apps.document' });
  const corpo = new Blob(['--' + fronteira + '\r\nContent-Type: application/json; charset=UTF-8\r\n\r\n' + meta + '\r\n--' + fronteira +
    '\r\nContent-Type: application/vnd.openxmlformats-officedocument.wordprocessingml.document\r\n\r\n', a.blob, '\r\n--' + fronteira + '--']);
  let r;
  try {
    r = await fetch('https://www.googleapis.com/upload/drive/v3/files?uploadType=multipart&fields=id', {
      method: 'POST', headers: { authorization: 'Bearer ' + google.acesso, 'content-type': 'multipart/related; boundary=' + fronteira }, body: corpo });
  } catch (e) { throw 'Sem conexão com o Google Drive.'; }
  if (r.status === 401) { google.acesso = null; throw 'O acesso ao Google venceu: clique de novo para dar a permissão.'; }
  if (!r.ok) throw (await lerErro(r)).replace('A IA', 'O Google Drive');
  const id = String((await r.json()).id || '');
  if (!/^[A-Za-z0-9_-]{10,128}$/.test(id)) throw 'O Google Drive não devolveu o documento criado.';
  const link = 'https://docs.google.com/document/d/' + id + '/edit';
  window.open(link, '_blank', 'noopener');
  return { id, link, feito: 'criado' };
}

// ── as janelas de diálogo (no lugar das do sistema) ──────────────────────────────────────────────────────────────
function caixa(texto, op, comCancelar) {
  return new Promise((ok) => {
    const d = document.createElement('dialog');
    d.className = 'web-caixa';
    const titulo = document.createElement('h2'); titulo.textContent = (op && op.title) || 'Incognitus';
    const p = document.createElement('p'); p.textContent = String(texto); p.style.whiteSpace = 'pre-wrap';
    const linha = document.createElement('div'); linha.className = 'linha fim';
    const fim = (v) => { d.close(); d.remove(); ok(v); };
    if (comCancelar) {
      const c = document.createElement('button'); c.type = 'button'; c.className = 'ghost'; c.textContent = (op && op.cancelLabel) || 'Cancelar';
      c.onclick = () => fim(false); linha.appendChild(c);
    }
    const b = document.createElement('button'); b.type = 'button'; b.textContent = (op && op.okLabel) || 'OK';
    b.onclick = () => fim(true); linha.appendChild(b);
    d.append(titulo, p, linha);
    d.addEventListener('cancel', (ev) => { ev.preventDefault(); fim(false); });
    document.body.appendChild(d); d.showModal(); b.focus();
  });
}

/** O seletor de arquivos do navegador. Os escolhidos entram no armazenamento e voltam como caminhos. Abre de forma
 *  síncrona (o clique de quem usa ainda vale): nada de await antes do input.click(). */
function escolher(op) {
  op = op || {};
  const inp = document.createElement('input');
  inp.type = 'file';
  inp.multiple = !!op.multiple || !!op.directory;
  if (op.directory) inp.webkitdirectory = true;
  const exts = (op.filters || []).flatMap((f) => f.extensions || []);
  if (exts.length && !op.directory) inp.accept = Array.from(new Set(exts.map((e) => '.' + e.toLowerCase()))).join(',');
  inp.style.display = 'none';
  document.body.appendChild(inp);
  return new Promise((ok) => {
    let feito = false;
    inp.onchange = async () => {
      feito = true;
      const fs = Array.from(inp.files || []);
      inp.remove();
      if (!fs.length) return ok(null);
      const lote = '/Arquivos/' + carimbo();
      const caminhos = [];
      for (const f of fs) {
        const rel = f.webkitRelativePath || f.name;
        const c = juntar(lote, rel);
        await gravarArq(c, f);
        caminhos.push(c);
      }
      if (op.directory) ok(juntar(lote, (fs[0].webkitRelativePath || '').split('/')[0] || ''));
      else ok(op.multiple ? caminhos : caminhos[0]);
    };
    inp.addEventListener('cancel', () => { if (!feito) { inp.remove(); ok(null); } });
    inp.click();
  });
}
const carimbo = () => { const d = new Date(), z = (n) => String(n).padStart(2, '0'); return d.getFullYear() + '-' + z(d.getMonth() + 1) + '-' + z(d.getDate()) + ' ' + z(d.getHours()) + '.' + z(d.getMinutes()) + '.' + z(d.getSeconds()); };

// O arrastar e soltar: os arquivos (e pastas) soltos entram no armazenamento; o app recebe os caminhos, como no Tauri.
async function lerEntrada(entrada, base, saida) {
  if (entrada.isFile) {
    const f = await new Promise((ok, falha) => entrada.file(ok, falha));
    const c = juntar(base, entrada.fullPath || f.name);
    await gravarArq(c, f); saida.push(c);
  } else if (entrada.isDirectory) {
    const r = entrada.createReader();
    let lote;
    do { lote = await new Promise((ok, falha) => r.readEntries(ok, falha)); for (const e of lote) await lerEntrada(e, base, saida); } while (lote.length);
  }
}
function aoArrastar(cb) {
  const pos = (ev) => ({ x: ev.clientX * devicePixelRatio, y: ev.clientY * devicePixelRatio });
  let dentro = 0;
  document.addEventListener('dragenter', (ev) => { if (!ev.dataTransfer || !Array.from(ev.dataTransfer.types).includes('Files')) return; ev.preventDefault(); if (dentro++ === 0) cb({ payload: { type: 'enter', position: pos(ev), paths: [] } }); });
  document.addEventListener('dragover', (ev) => { if (!ev.dataTransfer || !Array.from(ev.dataTransfer.types).includes('Files')) return; ev.preventDefault(); cb({ payload: { type: 'over', position: pos(ev) } }); });
  document.addEventListener('dragleave', () => { if (--dentro <= 0) { dentro = 0; cb({ payload: { type: 'leave' } }); } });
  document.addEventListener('drop', async (ev) => {
    if (!ev.dataTransfer || !ev.dataTransfer.files.length) return;
    ev.preventDefault(); dentro = 0;
    const p = pos(ev), base = '/Arquivos/' + carimbo(), caminhos = [];
    const entradas = Array.from(ev.dataTransfer.items || []).map((it) => it.webkitGetAsEntry && it.webkitGetAsEntry()).filter(Boolean);
    if (entradas.length) { for (const e of entradas) { const antes = caminhos.length; await lerEntrada(e, base, caminhos); if (e.isDirectory && caminhos.length > antes) { /* a pasta: os arquivos dela */ } } }
    else for (const f of Array.from(ev.dataTransfer.files)) { const c = juntar(base, f.name); await gravarArq(c, f); caminhos.push(c); }
    cb({ payload: { type: 'drop', position: p, paths: caminhos } });
  });
  return Promise.resolve(() => {});
}

/** O visor: o PDF numa janela por cima da página (um pop-up seria barrado). */
async function visor(caminho, rotulo) {
  const a = await arq(caminho);
  if (!a) throw 'não achei ' + nomeDe(caminho);
  const u = URL.createObjectURL(a.blob);
  const d = document.createElement('dialog');
  d.className = 'web-visor';
  d.innerHTML = '<div class="linha"><b></b><span class="cresce"></span><button type="button" class="ghost pequeno" data-baixar>Baixar</button><button type="button" class="pequeno" data-fechar>Fechar</button></div><iframe></iframe>';
  d.querySelector('b').textContent = rotulo || a.nome;
  d.querySelector('iframe').src = u;
  d.querySelector('[data-fechar]').onclick = () => { d.close(); };
  d.querySelector('[data-baixar]').onclick = () => baixar(a.blob, a.nome);
  d.addEventListener('close', () => { d.remove(); URL.revokeObjectURL(u); });
  document.body.appendChild(d); d.showModal();
}

/** "Revelar" no navegador: o arquivo é baixado; uma pasta vira um .zip. */
async function revelar(caminho) {
  const a = await arq(caminho);
  if (a) return baixar(a.blob, a.nome);
  const p = normal(caminho) + '/', z = new JSZip();
  const lista = (await todos()).filter((x) => x.caminho.startsWith(p));
  if (!lista.length) throw 'não achei ' + nomeDe(caminho);
  for (const x of lista) z.file(x.caminho.slice(p.length), x.blob);
  baixar(await z.generateAsync({ type: 'blob' }), nomeDe(caminho) + '.zip');
}

// ── os comandos ──────────────────────────────────────────────────────────────────────────────────────────────────
const naoNaWeb = (o) => { throw 'Na versão web, ' + o + ' ainda não está disponível: use o aplicativo do Incognitus.'; };
const PROVEDORES = ['anthropic', 'openai', 'gemini', 'deepinfra'];
const comandos = {
  info: () => ({ modulo: { id: 'incognitus', nome: 'Incognitus', versao: VERSAO }, hospedado: true, sistema: 'web', dados: 'este navegador' }),
  info_app: () => ({ versao: VERSAO, ferramentas: { Ok: ['pdf.js 4.10', 'tesseract.js 6', 'pdf-lib 1.17'] }, plataforma: 'web' }),
  registrar: ({ msg }) => { if (/erro|promessa/.test(msg)) console.warn('[incognitus]', msg); },
  e2e_plano: () => null,
  e2e_gravar: () => null,
  e2e_fim: () => null,
  acordado: ({ ligar }) => {
    try {
      if (ligar && navigator.wakeLock && !window.__trava) navigator.wakeLock.request('screen').then((t) => { window.__trava = t; t.addEventListener('release', () => { window.__trava = null; }); }).catch(() => {});
      if (!ligar && window.__trava) { window.__trava.release(); window.__trava = null; }
    } catch (e) { /* sem trava de tela */ }
    return true;
  },
  ajustes_ler: async () => (await kvLer('ajustes')) || {},
  ajustes_gravar: async ({ ajustes }) => { await kvGravar('ajustes', ajustes); },
  mesa_ler: async () => (await kvLer('mesa')) || '',
  mesa_gravar: async ({ texto }) => { JSON.parse(texto); await kvGravar('mesa', texto); },
  indice_ler: async () => (await kvLer('indice')) || '',
  indice_gravar: async ({ texto }) => { await kvGravar('indice', texto); },

  motor_ativo: async () => ({ origem: 'embutido', manifesto: await (await fetch(BASE + 'motor/manifesto.json')).json(), js: null, anterior: null }),
  motor_checar: () => null,
  motor_baixar: () => naoNaWeb('a troca do motor'),
  motor_instalar: () => naoNaWeb('a troca do motor'),
  motor_voltar: () => naoNaWeb('a troca do motor'),
  app_checar: () => null,
  app_baixar: () => naoNaWeb('a atualização'),
  app_instalar: () => naoNaWeb('a atualização'),

  preparar_arquivo: async ({ caminho }) => {
    const a = await arq(caminho);
    if (!a) throw 'não consegui ler o arquivo (não está mais no navegador)';
    const bytes = new Uint8Array(await a.blob.arrayBuffer());
    const cabeca = new TextDecoder('latin1').decode(bytes.subarray(0, 1024));
    if (!cabeca.includes('%PDF-')) throw 'não parece um PDF';
    const id = 'a' + (++contador);
    preparados.set(id, { bytes, doc: null });
    return { id, bytes: bytes.length };
  },
  extrair_texto: ({ id }) => extrairTexto(id),
  ocr_pagina: ({ id, pagina }) => ocrPagina(id, pagina),
  limpar_arquivo: ({ id }) => { const p = preparados.get(id); if (p && p.doc) p.doc.destroy(); preparados.delete(id); },
  texto_documento: async ({ caminho }) => {
    const e = extDe(caminho);
    if (e === 'docx') return docxTexto(caminho);
    if (e === 'txt' || e === 'md') return lerTextoBruto(caminho);
    throw 'só .txt e .docx (o PDF passa pela leitura do PDF)';
  },
  listar_pdfs: async ({ pasta }) => (await todos()).filter((a) => a.pasta === normal(pasta) && aceito(a.caminho)).map((a) => a.caminho).sort(),
  expandir_caminhos: async ({ caminhos }) => {
    const v = [], lista = await todos();
    for (const c of caminhos || []) {
      const n = normal(c);
      if (lista.some((a) => a.caminho === n)) { if (aceito(n)) v.push(n); continue; }
      v.push(...lista.filter((a) => a.pasta === n && aceito(a.caminho)).map((a) => a.caminho).sort());
    }
    return v;
  },
  pasta_padrao: ({ caminho }) => juntar(paiDe(caminho), PASTA_SAIDA),

  gravar_saida: async ({ pedido }) => {
    const origem = normal(pedido.origem);
    const dir = pedido.destino && String(pedido.destino).trim() ? normal(pedido.destino) : juntar(paiDe(origem), PASTA_SAIDA);
    const nome = nomeDe(origem);
    let base = pedido.nome_saida ? nomeSeguro(pedido.nome_saida) : semExt(nome);
    if (!base.toLowerCase().endsWith('_anon')) base += '_anon';
    const nosso = async (a) => /Incognitus/.test(String(a.produtor || ''));
    const destinoPdf = await livre(dir, base, '.pdf', nosso);
    if (destinoPdf === origem) throw 'a pasta de saída é a dos originais: o anonimizado teria o mesmo nome e tomaria o lugar do original';
    const produtor = 'Incognitus web ' + VERSAO + ' - motor de anonimizacao ' + pedido.versao_motor;
    const r = await gerarPdf(pedido.cabecalho, pedido.paginas, produtor);
    const reg = await gravarArq(destinoPdf, r.bytes, 'application/pdf');
    await tx('arquivos', 'readwrite', (s) => { s.put(Object.assign(reg, { produtor })); });
    const baseFinal = semExt(nomeDe(destinoPdf));
    let txt = null, mapa = null, confira = null, mapaCifrado = false, avisoCofre = null;
    if (pedido.texto != null) {
      txt = await livre(dir, baseFinal, '.txt', async (a) => (await a.blob.slice(0, 400).text()).includes('AUTOS ANONIMIZADOS'));
      await gravarArq(txt, pedido.texto);
    }
    const privada = juntar(dir, PASTA_PRIVADA);
    if (pedido.mapa != null) {
      mapa = juntar(privada, baseFinal + ' — MAPA (nomes reais).json');
      let conteudo = pedido.mapa;
      if (pedido.cifrar) { try { conteudo = await selar(pedido.mapa); mapaCifrado = true; } catch (e) { avisoCofre = String((e && e.message) || e); } }
      await gravarArq(mapa, conteudo, 'application/json');
    }
    if (pedido.confira != null) { confira = juntar(privada, baseFinal + ' — CONFIRA (nomes reais).txt'); await gravarArq(confira, pedido.confira); }
    return { pdf: destinoPdf, txt, mapa, confira, sem_glifo: r.semGlifo, folhas: pedido.paginas.length + 1, mapa_cifrado: mapaCifrado, aviso_cofre: avisoCofre };
  },
  ler_texto: async ({ caminho }) => { const t = await lerTextoBruto(caminho); return ehCofre(t) ? abrir(t) : t; },
  gravar_texto: async ({ caminho, texto }) => { await gravarArq(caminho, texto); },
  gravar_docx: async ({ caminho, paragrafos }) => {
    if (extDe(caminho) !== 'docx') throw 'a minuta só é gravada em .docx';
    await gravarArq(caminho, await montarDocx(paragrafos));
  },
  docx_partes: ({ caminho }) => docxPartes(caminho),
  gravar_partes: async ({ origem, destino, partes }) => {
    if (extDe(origem) !== 'docx' || extDe(destino) !== 'docx') throw 'o modelo preenchido só é gravado em .docx';
    if (normal(origem) === normal(destino)) throw 'a minuta tomaria o lugar do modelo';
    await docxGravarPartes(origem, destino, partes);
  },
  gravar_reidentificado: async ({ pedido }) => {
    const origem = normal(pedido.origem), ext = extDe(origem) ? '.' + extDe(origem) : '';
    const destino = await livre(paiDe(origem), nomeSeguro(pedido.titulo) + ' (reidentificado)', ext, null);
    if (pedido.partes) await docxGravarPartes(origem, destino, pedido.partes);
    else if (pedido.texto != null) await gravarArq(destino, pedido.texto);
    else throw 'nada para gravar';
    const a = await arq(destino);
    if (a) baixar(a.blob, a.nome);
    return destino;
  },
  abrir_documento: async ({ caminho }) => { const a = await arq(caminho); if (!a) throw 'só abro um .docx que já foi gravado'; baixar(a.blob, a.nome); },
  abrir_modelo: async ({ alvo }) => {
    const a = String(alvo || '').trim();
    if (/^https:\/\//.test(a)) { window.open(a, '_blank', 'noopener'); return; }
    const x = await arq(a);
    if (!x) throw 'não achei o arquivo do modelo (foi apagado do navegador?)';
    baixar(x.blob, x.nome);
  },
  revelar: ({ caminho }) => revelar(caminho),
  visor_abrir: ({ caminho, rotulo }) => visor(caminho, rotulo),
  abrir_feedback: () => { window.open(URL_FEEDBACK, '_blank', 'noopener'); },

  cofre_situacao: async () => { const c = await chaves(false); return { tem_chave: !!c, apelido: c ? await apelido(c.cru) : '' }; },
  cofre_chave_mostrar: async () => emGrupos((await chaves(true)).cru),
  cofre_chave_importar: async ({ chave }) => {
    const cru = deGrupos(chave), c = await chaves(false), ap = await apelido(cru);
    if (!c) { await kvGravar('cofre', { cru, antigas: [] }); return 'Chave importada (' + ap + '): os mapas cifrados com ela abrem neste navegador.'; }
    if (await apelido(c.cru) === ap) return 'Esta já é a chave deste navegador.';
    const antigas = (c.antigas || []).slice();
    if (!(await Promise.all(antigas.map(apelido))).includes(ap)) antigas.push(cru);
    await kvGravar('cofre', { cru: c.cru, antigas });
    return 'Chave importada (' + ap + '): os mapas cifrados com ela também abrem neste navegador; os novos seguem com a chave daqui.';
  },

  ner_estado: () => estadoModelo(MODELOS.ner),
  ner_baixar: () => baixarModelo(MODELOS.ner),
  ner_ler: ({ arquivo }) => lerModelo(MODELOS.ner, arquivo),
  busca_estado: () => estadoModelo(MODELOS.busca),
  busca_baixar: () => baixarModelo(MODELOS.busca),
  busca_ler: ({ arquivo }) => lerModelo(MODELOS.busca, arquivo),

  ia_situacao: async () => {
    const chavesIa = {};
    for (const p of PROVEDORES) { const c = await lerChaveIa(p); chavesIa[p] = { guardada: !!c, fim: c ? c.slice(-4) : '' }; }
    const vale = acessoVale();
    return { chave: chavesIa.anthropic, chaves: chavesIa, google: { configurado: !!(await clienteGoogle()), entrou: vale, email: google.email, drive: false, escrever: vale } };
  },
  ia_chave_guardar: async ({ chave, provedor }) => {
    const c = String(chave || '').trim();
    if (c.length < 12 || /\s/.test(c)) throw 'Essa chave não parece válida.';
    await kvGravar('ia-chave-' + (provedor || 'anthropic'), await selar(c));
    return { guardada: true, fim: c.slice(-4) };
  },
  ia_chave_apagar: async ({ provedor }) => { await kvApagar('ia-chave-' + (provedor || 'anthropic')); },
  ia_modelos: ({ provedor }) => (provedor === 'local' ? modelosLocal()
    : (window.SkiaIas && SkiaIas.PROVEDORES[provedor]) ? SkiaIas.PROVEDORES[provedor].modelos.map((m) => m[0]) : []),
  ia_enviar: (args) => iaEnviar(args),
  ia_google_entrar: ({ escrever }) => (escrever ? googleEntrar() : naoNaWeb('o login do Google para ler o Drive')),
  ia_google_cancelar: () => null,
  ia_google_sair: () => { if (google.acesso && window.google && window.google.accounts) window.google.accounts.oauth2.revoke(google.acesso, () => {}); google.acesso = null; },
  ia_gdocs_enviar: (args) => googleDocsEnviar(args),
  ia_prompt_ler: async ({ fonte }) => {
    const f = String(fonte || '').trim();
    if (/^https?:\/\//.test(f)) throw 'Na versão web, o prompt e os modelos vêm de um arquivo escolhido aqui ou do texto escrito aqui (links do Google Docs e do OneDrive só no aplicativo).';
    const e = extDe(f);
    const texto = e === 'docx' ? await docxTexto(f) : await lerTextoBruto(f);
    return { texto, tipo: 'arquivo', titulo: nomeDe(f), caracteres: texto.length };
  },
  ia_pasta_listar: async ({ pasta }) => {
    const p = normal(pasta) + '/';
    return (await todos()).filter((a) => a.caminho.startsWith(p) && /\.(docx|txt|md)$/i.test(a.nome) && !/^(\.|~\$)/.test(a.nome))
      .map((a) => ({ caminho: a.caminho, nome: a.nome, rel: a.pasta.slice(p.length).replace(/^\/+/, '') || '', tam: a.tam, mtime: a.mtime }))
      .sort((x, y) => x.caminho.localeCompare(y.caminho));
  },
};
comandos.modelos_listar = comandos.ia_pasta_listar;

async function invoke(cmd, args) {
  const nome = String(cmd).replace(PREFIXO, '');
  const fn = comandos[nome];
  if (!fn) throw 'comando desconhecido na versão web: ' + nome;
  try { return await fn(args || {}); }
  catch (e) { throw (e && e.message) ? e.message : e; }
}

// o navegador pode apagar o armazenamento de um site quando falta espaço; o pedido de persistência evita isso
document.documentElement.classList.add('web');
// a barra da web: a versão, onde ficam os arquivos, baixar e apagar tudo, sair
document.addEventListener('DOMContentLoaded', () => {
  const b = document.createElement('div');
  b.className = 'web-barra';
  b.innerHTML = '<span>Incognitus web ' + VERSAO + ' · os arquivos ficam neste navegador</span><button type="button" data-w="zip">Baixar tudo (.zip)</button>' +
    '<button type="button" data-w="apagar">Apagar tudo deste navegador</button><a href="sair">Sair</a>';
  b.addEventListener('click', async (ev) => {
    const w = ev.target.dataset && ev.target.dataset.w;
    if (w === 'zip') {
      const z = new JSZip(), lista = await todos();
      if (!lista.length) { await caixa('Ainda não há arquivos neste navegador.', {}, false); return; }
      for (const x of lista) z.file(x.caminho.replace(/^\/+/, ''), x.blob);
      baixar(await z.generateAsync({ type: 'blob' }), 'Incognitus web ' + carimbo() + '.zip');
    } else if (w === 'apagar') {
      if (!await caixa('Apagar deste navegador todos os autos, os PDFs anonimizados, os mapas, as minutas e a mesa? Não dá para desfazer. ' +
        'A chave do cofre e as chaves das IAs ficam (para apagá-las, limpe os dados do site no navegador).', { okLabel: 'Apagar tudo', cancelLabel: 'Cancelar' }, true)) return;
      await window.IncognitusWeb.apagarTudo();
      location.reload();
    }
  });
  document.body.appendChild(b);
});
if (navigator.storage && navigator.storage.persist) navigator.storage.persist().catch(() => {});

window.__TAURI__ = {
  core: { invoke },
  dialog: {
    message: (texto, op) => caixa(texto, op, false).then(() => undefined),
    ask: (texto, op) => caixa(texto, op, true),
    confirm: (texto, op) => caixa(texto, op, true),
    open: (op) => escolher(op),
    save: async (op) => {
      const nome = (op && op.defaultPath) ? nomeDe(op.defaultPath) : 'arquivo.txt';
      return juntar('/Downloads', carimbo(), nome);
    },
  },
  webview: { getCurrentWebview: () => ({ onDragDropEvent: (cb) => aoArrastar(cb) }) },
};
window.IncognitusWeb = { VERSAO, revelar, baixar, todos, apagarTudo: async () => {
  await tx('arquivos', 'readwrite', (s) => { s.clear(); });
  for (const k of ['mesa', 'indice']) await kvApagar(k);
} };
})();
