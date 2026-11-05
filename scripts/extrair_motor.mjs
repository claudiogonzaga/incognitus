#!/usr/bin/env node
// Σκιά · Skiá — monta o motor de anonimização (motor/motor.js) a partir do graphe.js do Graphé.
//
// O que entra: SÓ o que o lote automático da anonimização usa — as funções que o testes/anon/pipeline.js do Graphé
// chama (preparar → classificador → concluir), as que a API do Skiá chama (cabeçalho do anonConcluir, páginas para o
// OCR) e, pelo NOME, tudo de que elas dependem: função que chama função, constante que usa constante, e os comandos
// soltos que enchem essas constantes. Nada é tirado por número de linha. O que no Graphé vem da planilha e do Apps
// Script (VARIAVEIS_GLOBAIS, equipe, propriedades, Logger, Utilities) fica de fora: é o motor/fonte/ambiente.js.
// Os comentários saem (o motor vai para um repositório público e eles citam casos).
//
// Depois de montar, nesta ordem, e qualquer falha PARA sem gravar nada:
//   1. sintaxe;
//   2. a TRAVA (scripts/trava.mjs): nenhum ID do Drive, URL de WebApp, ID de planilha, token, e-mail, prompt ou nome
//      de pessoa;
//   3. a SUÍTE (scripts/regressao_motor.cjs): o regressao.js do Graphé, sem alteração, com o motor.js num contexto
//      limpo (nada do Apps Script), e os mesmos casos pela API do Skiá — tem de dar "0 falhas" nas duas.
//
// Uso: node scripts/extrair_motor.mjs [--fonte caminho/graphe.js] [--sem-copiar]
//   GRAPHE_DIR (padrão ../Graphe) ou GRAPHE_FONTE trocam a origem. --sem-copiar não atualiza o motor embutido no app.
import fs from 'node:fs';
import path from 'node:path';
import crypto from 'node:crypto';
import { createRequire } from 'node:module';
import { fileURLToPath } from 'node:url';
import { execFileSync } from 'node:child_process';
import { travar } from './trava.mjs';

const require = createRequire(import.meta.url);
const acorn = require('acorn');

const SKIA = path.resolve(path.dirname(fileURLToPath(import.meta.url)), '..');
const arg = (k) => { const i = process.argv.indexOf(k); return i > 0 ? process.argv[i + 1] : null; };
const GRAPHE = process.env.GRAPHE_DIR || path.resolve(SKIA, '../Graphe');
const FONTE = arg('--fonte') || process.env.GRAPHE_FONTE || path.join(GRAPHE, 'graphe.js');
const PIPELINE = path.join(GRAPHE, 'testes/anon/pipeline.js');
const REGRESSAO = path.join(GRAPHE, 'testes/anon/regressao.js');
const AMBIENTE = path.join(SKIA, 'motor/fonte/ambiente.js');
const API = path.join(SKIA, 'motor/fonte/api.js');
const SAIDA = path.join(SKIA, 'motor');
const EMBUTIDO = path.join(SKIA, 'src/motor');

// O que o Skiá pede pelo nome além do que o pipeline.js e a API chamam (o pedido de 05/11/2026: os pesos do
// classificador, o limiar e a versão). Se algum não existir no graphe.js, o build para.
const OBRIGATORIOS = ['ANON_VERSAO', 'ANON_BAYES_PESOS', 'ANON_BAYES_LIMIAR', 'ANON_INST_PADRAO', 'ANON_BOILERPLATE'];

function parar(msg) { console.error('PAROU: ' + msg); process.exit(1); }
const sha256 = (s) => crypto.createHash('sha256').update(s).digest('hex');
const ler = (f) => { try { return fs.readFileSync(f, 'utf8'); } catch (e) { parar('não consegui ler ' + f + ' (' + e.code + ')'); } };

/** Percorre a árvore sem recursão (o graphe.js tem expressões fundas demais para a pilha). */
function percorrer(raiz, fn) {
  const pilha = [[raiz, null]];
  while (pilha.length) {
    const [x, pai] = pilha.pop();
    fn(x, pai);
    for (const k in x) {
      if (k === 'loc' || k === 'start' || k === 'end' || k === 'range') continue;
      const v = x[k];
      if (Array.isArray(v)) { for (const e of v) if (e && typeof e.type === 'string') pilha.push([e, x]); }
      else if (v && typeof v.type === 'string') pilha.push([v, x]);
    }
  }
}

/** Nomes usados (lidos ou escritos) num nó, fora as chaves de objeto e as propriedades com ponto. */
function nomesUsados(no) {
  const usados = new Set();
  percorrer(no, (x, pai) => {
    if (x.type !== 'Identifier') return;
    if (pai && pai.type === 'MemberExpression' && pai.property === x && !pai.computed) return;
    if (pai && pai.type === 'Property' && pai.key === x && !pai.computed && !pai.shorthand) return;
    if (pai && (pai.type === 'MethodDefinition' || pai.type === 'PropertyDefinition') && pai.key === x && !pai.computed) return;
    usados.add(x.name);
  });
  return usados;
}

/** Nomes que um comando solto (sem declaração) altera: alvo de atribuição ou objeto de chamada de método. */
function nomesAlterados(no) {
  const alt = new Set();
  const raizDe = (e) => { while (e && e.type === 'MemberExpression') e = e.object; return e && e.type === 'Identifier' ? e.name : null; };
  percorrer(no, (x) => {
    if (x.type === 'AssignmentExpression' || x.type === 'UpdateExpression') { const r = raizDe(x.left || x.argument); if (r) alt.add(r); }
    else if (x.type === 'CallExpression' && x.callee.type === 'MemberExpression') { const r = raizDe(x.callee); if (r) alt.add(r); }
  });
  return alt;
}

/** Os comandos de topo de um arquivo, com o que cada um declara, usa e altera. */
function comandos(fonte, arquivo) {
  const comentarios = [];
  let ast;
  try { ast = acorn.parse(fonte, { ecmaVersion: 'latest', sourceType: 'script', onComment: comentarios }); }
  catch (e) { parar('não consegui ler ' + arquivo + ': ' + e.message); }
  const lista = ast.body.map((n, i) => {
    const nomes = [];
    if (n.type === 'FunctionDeclaration') nomes.push(n.id.name);
    else if (n.type === 'VariableDeclaration') for (const d of n.declarations) if (d.id.type === 'Identifier') nomes.push(d.id.name);
    return { i, n, nomes, usa: nomesUsados(n), altera: nomes.length ? new Set() : nomesAlterados(n) };
  });
  return { lista, comentarios };
}

/** O texto de um trecho sem os comentários (bloco de várias linhas vira quebra de linha, por causa do ASI). */
function semComentarios(fonte, ini, fim, comentarios) {
  let out = '', pos = ini;
  for (const c of comentarios) {
    if (c.end <= ini || c.start >= fim) continue;
    out += fonte.slice(pos, c.start);
    if (c.type === 'Block') out += /\n/.test(fonte.slice(c.start, c.end)) ? '\n' : ' ';
    pos = c.end;
  }
  out += fonte.slice(pos, fim);
  return out.split('\n').map((l) => l.replace(/[ \t]+$/, '')).filter((l, k, a) => l !== '' || (k > 0 && a[k - 1] !== '')).join('\n');
}

// ── 1. Ler as fontes ──────────────────────────────────────────────────────────────────────────────────────────────
const fonteGraphe = ler(FONTE);
const fontePipeline = ler(PIPELINE);
const fonteAmbiente = ler(AMBIENTE);
const fonteApi = ler(API);
const G = comandos(fonteGraphe, FONTE);
const declarados = new Map();
for (const c of G.lista) for (const nm of c.nomes) { if (!declarados.has(nm)) declarados.set(nm, []); declarados.get(nm).push(c.i); }

// ── 2. Fronteira e entradas ───────────────────────────────────────────────────────────────────────────────────────
const amb = comandos(fonteAmbiente, AMBIENTE);
const FRONTEIRA = new Set(amb.lista.flatMap((c) => c.nomes).concat(['Logger', 'Utilities']));
// O pipeline.js chama tudo como G.nome: essas são as entradas do lote.
const pip = comandos(fontePipeline, PIPELINE);
const entradas = new Set();
for (const c of pip.lista) percorrer(c.n, (x) => {
  if (x.type === 'MemberExpression' && !x.computed && x.object.type === 'Identifier' && x.object.name === 'G') entradas.add(x.property.name);
});
const api = comandos(fonteApi, API);
for (const c of api.lista) for (const u of c.usa) if (declarados.has(u)) entradas.add(u);
for (const o of OBRIGATORIOS) { if (!declarados.has(o)) parar('o graphe.js não tem ' + o); entradas.add(o); }
for (const e of entradas) if (!declarados.has(e) && !FRONTEIRA.has(e)) parar('o pipeline.js ou a API usa ' + e + ', que o graphe.js não declara');

// ── 3. Fecho por nome ─────────────────────────────────────────────────────────────────────────────────────────────
const nomesIncluidos = new Set(), cmdIncluidos = new Set(), fila = [...entradas];
const incluir = (nm) => {
  if (FRONTEIRA.has(nm) || !declarados.has(nm) || nomesIncluidos.has(nm)) return;
  nomesIncluidos.add(nm);
  for (const i of declarados.get(nm)) if (!cmdIncluidos.has(i)) { cmdIncluidos.add(i); for (const u of G.lista[i].usa) fila.push(u); }
};
for (let mudou = true; mudou;) {
  mudou = false;
  while (fila.length) incluir(fila.shift());
  for (const c of G.lista) {
    if (cmdIncluidos.has(c.i) || c.nomes.length) continue;
    if ([...c.altera].some((a) => nomesIncluidos.has(a))) { cmdIncluidos.add(c.i); for (const u of c.usa) fila.push(u); mudou = true; }
  }
}
for (const f of FRONTEIRA) if (declarados.has(f) && cmdIncluidos.has(declarados.get(f)[0])) parar('a fronteira ' + f + ' entrou no motor');

// ── 4. Montar o motor.js ──────────────────────────────────────────────────────────────────────────────────────────
const ordem = [...cmdIncluidos].sort((a, b) => a - b);
const extraido = ordem.map((i) => { const n = G.lista[i].n; return semComentarios(fonteGraphe, n.start, n.end, G.comentarios); }).join('\n');
const pipelineSemComent = semComentarios(fontePipeline, 0, fontePipeline.length, pip.comentarios);
const versao = (() => { const m = /^var ANON_VERSAO = '([^']+)'/m.exec(extraido); return m ? m[1] : parar('ANON_VERSAO sem valor literal'); })();
const motor = [
  '// Σκιά · Skiá — motor de anonimização ' + versao + '. Gerado por scripts/extrair_motor.mjs a partir do graphe.js do Graphé',
  '// (as funções do lote automático da anonimização e as dependências delas, pelo nome) e do testes/anon/pipeline.js.',
  '// Não edite à mão: rode de novo o script. O cabeçalho que este motor escreve diz "GRAPHE v' + versao + '": é a versão do motor.',
  fonteAmbiente.trim(),
  '// ── Graphé: anonimização (' + ordem.length + ' declarações) ──',
  extraido,
  '// ── Graphé: testes/anon/pipeline.js (o lote automático) ──',
  'var SKIA_PIPELINE = (function () {',
  '  var module = { exports: {} };',
  pipelineSemComent.trim(),
  '  return module.exports;',
  '})();',
  fonteApi.trim(),
  ''
].join('\n');

// ── 5. Conferências ───────────────────────────────────────────────────────────────────────────────────────────────
try { acorn.parse(motor, { ecmaVersion: 'latest', sourceType: 'script' }); } catch (e) { parar('o motor montado não compila: ' + e.message); }
const achados = travar(motor);
if (achados.length) parar('o motor tem dados que não podem ir a público (nada foi gravado):\n  ' + achados.join('\n  '));
console.log('trava: nenhum ID, URL, token, e-mail, prompt ou nome de pessoa · sintaxe ok');

// Os casos da suíte (o array `casos` e a CAPA do regressao.js, avaliados pelo nome) → casos.json, embutido no app.
const casos = (() => {
  const reg = comandos(ler(REGRESSAO), REGRESSAO);
  const fonteReg = ler(REGRESSAO);
  const pega = (nome) => {
    for (const c of reg.lista) if (c.n.type === 'VariableDeclaration') for (const d of c.n.declarations) if (d.id.name === nome) return fonteReg.slice(d.init.start, d.init.end);
    parar('o regressao.js não tem ' + nome);
  };
  const vm = require('node:vm');
  const capa = vm.runInNewContext('(' + pega('CAPA') + ')');
  const lista = vm.runInNewContext('(' + pega('casos') + ')');
  return { capa, reqPadrao: 'JOAO TESTEFULANO PIRAMBOIA', casos: JSON.parse(JSON.stringify(lista)) };
})();
const casosJson = JSON.stringify(casos, null, 1) + '\n';

const tmp = fs.mkdtempSync(path.join(SKIA, 'motor/.montando-'));
try {
  fs.writeFileSync(path.join(tmp, 'motor.js'), motor);
  fs.writeFileSync(path.join(tmp, 'casos.json'), casosJson);
  let out;
  try {
    out = execFileSync(process.execPath, [path.join(SKIA, 'scripts/regressao_motor.cjs'), path.join(tmp, 'motor.js'),
      '--casos', path.join(tmp, 'casos.json'), '--graphe', GRAPHE], { encoding: 'utf8', stdio: ['ignore', 'pipe', 'pipe'] });
  } catch (e) { parar('a suíte falhou:\n' + ((e.stdout || '') + (e.stderr || '')).trim()); }
  process.stdout.write(out);
  if (!/RESULTADO: \d+ ok, 0 falhas/.test(out) || !/SKIÁ: \d+ ok, 0 falhas/.test(out)) parar('a suíte não terminou em "0 falhas"');

  // ── 6. Gravar (só agora) ────────────────────────────────────────────────────────────────────────────────────────
  const hash = sha256(motor);
  const manifAntigo = (() => { try { return JSON.parse(fs.readFileSync(path.join(SAIDA, 'manifesto.json'), 'utf8')); } catch (e) { return null; } })();
  const manifesto = (manifAntigo && manifAntigo.sha256 === hash && manifAntigo.casos && manifAntigo.casos.sha256 === sha256(casosJson)) ? manifAntigo : {
    nome: 'skia-motor', versao, api: 1, data: new Date().toISOString().replace(/\.\d+Z$/, 'Z'), sha256: hash,
    bytes: Buffer.byteLength(motor), casos: { total: casos.casos.length, sha256: sha256(casosJson) },
    origem: { graphe_sha256: sha256(fonteGraphe), pipeline_sha256: sha256(fontePipeline) }
  };
  const manifJson = JSON.stringify(manifesto, null, 2) + '\n';
  for (const dir of [SAIDA].concat(process.argv.includes('--sem-copiar') ? [] : [EMBUTIDO])) {
    fs.mkdirSync(dir, { recursive: true });
    fs.writeFileSync(path.join(dir, 'motor.js'), motor);
    fs.writeFileSync(path.join(dir, 'casos.json'), casosJson);
    fs.writeFileSync(path.join(dir, 'manifesto.json'), manifJson);
  }
  console.log('motor ' + versao + ' · ' + ordem.length + ' declarações · ' + Math.round(Buffer.byteLength(motor) / 1024) + ' KB · sha256 ' + hash.slice(0, 12) + '…' +
    (manifesto === manifAntigo ? ' (sem mudança)' : ''));
  console.log('fonte: ' + FONTE);
} finally {
  fs.rmSync(tmp, { recursive: true, force: true });
}
