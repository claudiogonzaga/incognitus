#!/usr/bin/env node
// Incognitus — a suíte de regressão do Graphé sobre o motor.js.
//   1. o testes/anon/regressao.js do Graphé, SEM alteração, com o motor.js no lugar do graphe.js: o carregar.js é
//      trocado por um contexto vm LIMPO (só o JavaScript, nenhum stub do Apps Script — dependência que faltar quebra
//      aqui) e o pipeline.js é o do próprio Graphé;
//   2. os mesmos casos (casos.json) pela API do Incognitus — SkiaMotor.anonimizar, o caminho do app —, conferidos pelo
//      src/casos.js, num motor recém-carregado;
//   3. paridade: caso a caso, o corpo da API do Incognitus é idêntico ao do pipeline.js do Graphé.
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

// 2 ─ a API do Incognitus
const pacote = JSON.parse(fs.readFileSync(CASOS, 'utf8'));
const ctx2 = motorLimpo();
const res = SkiaCasos.rodar(ctx2.SkiaMotor, pacote);
console.log('── os mesmos casos pela API do Incognitus (SkiaMotor.anonimizar)');
res.falhas.forEach((f) => console.log('✗ ' + f.id + ' [' + f.c + '] ' + f.erros.join('; ')));
console.log('INCOGNITUS: ' + res.ok + ' ok, ' + res.falhas.length + ' falhas, de ' + res.total + '  (motor ' + res.versao + ', api ' + res.api + ')');
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

// 4 ─ os carimbos do PJe na saída (só do Incognitus: o corpo do Graphé fica igual, a saída perde quem assina e quem gerou)
{
  const M = ctx2.SkiaMotor;
  const rod = (quem, data, num) => `\n\nAssinado eletronicamente por: ${quem} - ${data}\nhttps://pje.tjmt.jus.br:443/pje/Processo/ConsultaDocumento/listView.seam?x=26110514321012300000123456789\nNúmero do documento: 26110514321012300000123456789\nNum. ${num} - Pág. 1\nEste documento foi gerado pelo usuário 123.***.***-45 em 06/11/2026 09:12:33\n`;
  const texto = 'PROCESSO Nº 1000001-11.2026.8.11.0001\nAUTOR: JOAO TESTEFULANO PIRAMBOIA\nRÉU: MARIA TESTEBELTRANA DIAS\n\nDECISÃO\nO autor JOAO TESTEFULANO PIRAMBOIA requer a citação da ré. Defiro.' +
    rod('TESTEJUIZ ALBERTO SOUZA', '05/11/2026 14:32:10', 135791357) + '\f' +
    'CERTIDÃO\nCertifico que a ré MARIA TESTEBELTRANA DIAS foi citada.\n\n                                        Assinado eletronicamente por: TESTESERVIDORA\n                                                  ROBERTA NUNES CAMPOS\n' +
    'Documento assinado eletronicamente por Testepromotora Lucia Fictícia, Promotora de Justiça, em 05/11/2026, às 10:00.\n' +
    'Assinado de forma digital por TESTEPERITO OTAVIO FICTICIO:12345678909 Dados: 2026.11.05 10:00:00 -03\'00\'\n\f';
  const r = M.anonimizar(texto, {});
  const casos = [
    ['quem assina some do carimbo', !/TESTEJUIZ|TESTESERVIDORA|Testepromotora|TESTEPERITO|ROBERTA NUNES/.test(r.texto)],
    ['quem gerou o PDF some', /gerado pelo usuário \[omitido\] em 06\/11\/2026 09:12:33/.test(r.texto) && !/123\.\*\*\*|«CPFMASC/.test(r.texto)],
    ['data, número e "Num. - Pág." ficam', /Assinado eletronicamente por: \[omitido\] - 05\/11\/2026 14:32:10/.test(r.texto) && /Num\. 135791357 - Pág\. 1/.test(r.texto)],
    ['carimbo do SEI: o cargo fica', /assinado eletronicamente por \[omitido\], Promotora de Justiça, em 05\/11\/2026/.test(r.texto)],
    ['carimbo ICP-Brasil: o CPF some', !/12345678909/.test(r.texto)],
    ['as páginas do PDF também', r.paginas.join('\n').indexOf('TESTEJUIZ') < 0 && r.paginas.join('\n').indexOf('[omitido]') >= 0],
    ['as partes continuam em token', !/TESTEFULANO|TESTEBELTRANA/.test(r.texto)],
    ['o OCR partiu a linha de quem gerou o PDF', !/020\.\*\*\*/.test(M.anonimizar(texto.replace('Este documento foi gerado pelo usuário 123.***.***-45', 'Este documento foi i\n\ngerado pelo usuário 020.***.***. 55'), {}).texto)],
  ];
  const ruins = casos.filter((c) => !c[1]);
  ruins.forEach((c) => console.log('✗ carimbo: ' + c[0]));
  console.log('CARIMBOS: ' + (casos.length - ruins.length) + ' ok, ' + ruins.length + ' falhas, de ' + casos.length);
  if (ruins.length) falhou = true;
}
process.exitCode = falhou ? 1 : 0;
