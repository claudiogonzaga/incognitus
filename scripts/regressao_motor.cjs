#!/usr/bin/env node
// Σκιά · Skiá — a suíte de regressão do Graphé sobre o motor.js.
//   1. o testes/anon/regressao.js do Graphé, SEM alteração, com o motor.js no lugar do graphe.js: o carregar.js é
//      trocado por um contexto vm LIMPO (só o JavaScript, nenhum stub do Apps Script — dependência que faltar quebra
//      aqui) e o pipeline.js é o do próprio Graphé;
//   2. os mesmos casos (casos.json) pela API do Skiá — SkiaMotor.anonimizar, o caminho do app —, conferidos pelo
//      src/casos.js, num motor recém-carregado;
//   3. paridade: caso a caso, o corpo da API do Skiá é idêntico ao do pipeline.js do Graphé.
// Sem a pasta do Graphé (no CI, por exemplo), só o passo 2 roda.
// Uso: node scripts/regressao_motor.cjs motor/motor.js [--casos motor/casos.json] [--graphe ../Graphe] [--verbose]
const fs = require('fs'), path = require('path'), vm = require('vm');
const SKIA = path.resolve(__dirname, '..');
const arg = (k, d) => { const i = process.argv.indexOf(k); return i > 0 ? process.argv[i + 1] : d; };
const MOTOR = path.resolve(process.argv[2] && !process.argv[2].startsWith('--') ? process.argv[2] : path.join(SKIA, 'motor/motor.js'));
const CASOS = path.resolve(arg('--casos', path.join(path.dirname(MOTOR), 'casos.json')));
const GRAPHE = path.resolve(arg('--graphe', process.env.GRAPHE_DIR || path.join(SKIA, '../Graphe')));
const VERB = process.argv.includes('--verbose');
const fonteMotor = fs.readFileSync(MOTOR, 'utf8');
const SkiaCasos = require(path.join(SKIA, 'src/casos.js'));

function motorLimpo() {
  const ctx = vm.createContext({});
  vm.runInContext(fonteMotor, ctx, { filename: 'motor.js' });
  return ctx;
}

let falhou = false;
const REG = path.join(GRAPHE, 'testes/anon/regressao.js'), PIP = path.join(GRAPHE, 'testes/anon/pipeline.js');
const temGraphe = fs.existsSync(REG) && fs.existsSync(PIP);

// 1 ─ o regressao.js do Graphé
if (temGraphe) {
  const ctx = motorLimpo();
  const linhas = [];
  const req = (p) => /carregar\.js$/.test(p) ? (() => ctx) : /pipeline\.js$/.test(p) ? require(PIP) : require(p);
  const proc = { argv: [process.argv[0], REG, MOTOR].concat(VERB ? ['--verbose'] : []), exitCode: 0 };
  const cons = { log: (...a) => linhas.push(a.join(' ')), error: (...a) => linhas.push(a.join(' ')), warn: (...a) => linhas.push(a.join(' ')) };
  new Function('require', 'module', 'exports', '__dirname', '__filename', 'process', 'console', fs.readFileSync(REG, 'utf8'))(
    req, { exports: {} }, {}, path.dirname(REG), REG, proc, cons);
  console.log('── testes/anon/regressao.js do Graphé, sobre o motor.js (contexto limpo)');
  console.log(linhas.join('\n').trim());
  if (proc.exitCode || !/RESULTADO: \d+ ok, 0 falhas/.test(linhas.join('\n'))) falhou = true;
} else console.log('── (sem a pasta do Graphé em ' + GRAPHE + ': só os casos embutidos)');

// 2 ─ a API do Skiá
const pacote = JSON.parse(fs.readFileSync(CASOS, 'utf8'));
const ctx2 = motorLimpo();
const res = SkiaCasos.rodar(ctx2.SkiaMotor, pacote);
console.log('── os mesmos casos pela API do Skiá (SkiaMotor.anonimizar)');
res.falhas.forEach((f) => console.log('✗ ' + f.id + ' [' + f.c + '] ' + f.erros.join('; ')));
console.log('SKIÁ: ' + res.ok + ' ok, ' + res.falhas.length + ' falhas, de ' + res.total + '  (motor ' + res.versao + ', api ' + res.api + ')');
if (res.falhas.length || res.total < 1) falhou = true;

// 3 ─ paridade com o pipeline.js do Graphé
if (temGraphe) {
  const P = require(PIP);
  let iguais = 0;
  const difs = [];
  for (const k of pacote.casos) {
    const texto = pacote.capa + k.t + '\f', opReq = k.req || pacote.reqPadrao;
    let a = null, b = null;
    try { a = P.rodar(ctx2, texto, { req: opReq, equipe: [] }).corpo; } catch (e) { a = 'ERRO ' + e.message; }
    try { b = ctx2.SkiaMotor.anonimizar(texto, { req: opReq, incluirMapa: true }).corpo; } catch (e) { b = 'ERRO ' + e.message; }
    if (a === b) iguais++; else difs.push(k.id);
  }
  console.log('PARIDADE: ' + iguais + '/' + pacote.casos.length + ' corpos iguais ao do pipeline.js do Graphé' + (difs.length ? ' — diferem: ' + difs.join(', ') : ''));
  if (difs.length) falhou = true;
}
process.exitCode = falhou ? 1 : 0;
