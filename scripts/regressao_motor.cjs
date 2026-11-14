#!/usr/bin/env node
// Incognitus — a suíte de regressão do Graphé sobre o motor.js.
//   1. o testes/anon/regressao.js do Graphé, SEM alteração, com o motor.js no lugar do graphe.js: o carregar.js é
//      trocado por um contexto vm LIMPO (só o JavaScript, nenhum stub do Apps Script — dependência que faltar quebra
//      aqui) e o pipeline.js é o do próprio Graphé;
//   2. os mesmos casos (casos.json) pela API do Incognitus — SkiaMotor.anonimizar, o caminho do app —, conferidos pelo
//      modulo/casos.js, num motor recém-carregado;
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
const SkiaCasos = require(path.join(SKIA, 'modulo/casos.js'));

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
    ['assinatura ICP-Brasil espalhada em linhas (nome, letras grandes, CN e CPF)', (function () {
      const icp = 'DESPACHO\nCite-se o réu MARIA TESTEBELTRANA DIAS.\n\n'
        + '                          TESTEFULANA         Assinado de forma digital\n'
        + '                                                                     por TESTEFULANA TESTESOBRENOME\n'
        + '                                                       TESTESOBRENOME   FICTICIO:9876543210\n'
        + '                                                                     Dados: 2026.01.02 10:00:00 -04\'00\'\n'
        + 'TESTEBELTRANO DN: C=BR, O=ICP-Brasil, OU=AC TESTE, CN=TESTEBELTRANO TESTECICRANO:12345678909\n'
        + 'Testeprenome Ficticio da Testa Dados: 2026.01.02 10:00:00 -04\'00\'\n\f';
      const t = M.anonimizar('PROCESSO Nº 1000001-11.2026.8.11.0001\nRÉU: MARIA TESTEBELTRANA DIAS\f' + icp, {}).texto;
      return !/TESTEFULANA|TESTESOBRENOME|9876543210|TESTEBELTRANO|TESTECICRANO|12345678909|Testeprenome/.test(t) && /Dados: 2026\.01\.02/.test(t);
    })()],
    ['carimbo estropiado pelo OCR: nome em maiúsculas antes de " - data hora"', (function () {
      // a servidora fica em claro no corpo (regra 4 do Graphé), mas não no carimbo
      const t = M.anonimizar('PROCESSO Nº 1000001-11.2026.8.11.0001\nRÉU: MARIA TESTEBELTRANA DIAS\f'
        + 'CERTIDÃO\nCertifico que intimei a ré MARIA TESTEBELTRANA DIAS.\nTESTESERVIDORA FICTICIA DE TAL\nGestora Judiciária\n'
        + '26010210000000000000000000001 Assil letroni te : TESTESERVIDORA FICTICIA DE TA - 02/01/2026 10:00:00\n\f', {}).texto;
      const carimbo = t.split('\n').filter((l) => /10:00:00/.test(l)).join('\n');
      return carimbo !== '' && !/TESTESERVIDORA|FICTICIA/.test(carimbo) && /\[omitido\] - 02\/01\/2026 10:00:00/.test(carimbo)
        && !/TESTEBELTRANA/.test(t);
    })()],
    ['ICP-Brasil com o nome intercalado nas duas colunas', (function () {
      const t = M.anonimizar('PROCESSO Nº 1000001-11.2026.8.11.0001\nRÉU: MARIA TESTEBELTRANA DIAS\f'
        + 'PETIÇÃO\nRequer a juntada. Ré MARIA TESTEBELTRANA DIAS.\n                                    Nestes termos, pede deferimento.\n'
        + '                                    Assinado de forma\n'
        + '                     TESTEFULANA    digital por TESTEFULANA\n'
        + '                     TESTESOBRENOME TESTESOBRENOME\n'
        + '                     TESTEFIM       TESTEFIM: 98765\n'
        + '                                    Dados: 2026.01.02 10:00:00 -04\'00\'\n\f', {}).texto;
      return !/TESTEFULANA|TESTESOBRENOME|TESTEFIM|98765|TESTEBELTRANA/.test(t) && /Dados: 2026\.01\.02/.test(t) && /pede deferimento/.test(t);
    })()],
    ['carimbo na diagonal com o nome acima, entre a URL e o número', (function () {
      const t = M.anonimizar('PROCESSO Nº 1000001-11.2026.8.11.0001\nRÉU: MARIA TESTEBELTRANA DIAS\f'
        + 'DESPACHO\nCite-se a ré MARIA TESTEBELTRANA DIAS.\n'
        + '                    26010210000000000000000000002\n'
        + '                    https://pje.tjmt.jus.br:443/pje/Processo/ConsultaDocumento/listView.seam?x=26010210000000000000000000002\n'
        + '                    TESTEPRENOME\n'
        + 'Assinado eletronicamente por: TESTEPRENOME TESTESOBRENOME - 02/01/2026 10:00:00\n\f', {}).texto;
      return !/TESTEPRENOME|TESTESOBRENOME/.test(t) && /Cite-se/.test(t) && /listView/.test(t);
    })()],
    ['"digitalmente por NOME" sem o "assinado" (cortado pelo leiaute)', (function () {
      const t = M.anonimizar('PROCESSO Nº 1000001-11.2026.8.11.0001\nRÉU: MARIA TESTEBELTRANA DIAS\f'
        + 'OFÍCIO\nNatureza(s) vinculada(s) a vítima:\n          47 digitalmente por TESTEASSINANTE\n          8\n'
        + 'Documento assinado digitalmente por meio de certificado.\n\f', {}).texto;
      return !/TESTEASSINANTE/.test(t) && /assinado digitalmente por meio de certificado/.test(t);
    })()],
    ['o OCR partiu a linha de quem gerou o PDF', !/999\.\*\*\*/.test(M.anonimizar(texto.replace('Este documento foi gerado pelo usuário 123.***.***-45', 'Este documento foi i\n\ngerado pelo usuário 999.***.***. 88'), {}).texto)],
  ];
  const ruins = casos.filter((c) => !c[1]);
  ruins.forEach((c) => console.log('✗ carimbo: ' + c[0]));
  console.log('CARIMBOS: ' + (casos.length - ruins.length) + ' ok, ' + ruins.length + ' falhas, de ' + casos.length);
  // 5 ─ a anonimização rigorosa: o filtro do que o modelo aponta (sem o modelo; dados fictícios)
  const tr = 'PROCESSO Nº 1000001-11.2026.8.11.0001\nRÉU: GERVASIO TESTEFULANO DE TAL\f'
    + 'TERMO DE DECLARAÇÕES\nQUE a declarante diz que suas amigas Gertrudes e Leocádia chegaram e tentaram acalmar GERVASIO TESTEFULANO DE TAL.\n'
    + 'QUE a rosa estava murcha e a Rosa não falou nada.\nConforme julgado no HC 123.456/MT, Rel. Min. Fulano Ficticio, Plenário, julgado em 01/01/2020 (Teodoro Precedente versus Estado).\n\f';
  const ex = [{ nome: 'Gertrudes', tipo: 'PESSOA' }, { nome: 'Leocádia', tipo: 'PESSOA' }, { nome: 'Rosa', tipo: 'PESSOA' }, { nome: 'Teodoro Precedente', tipo: 'PESSOA' }];
  const sem = M.anonimizar(tr, {}), com = M.anonimizar(tr, { extras: ex, rigoroso: true, incluirMapa: true });
  const rig = [
    ['sem extras, nada muda', sem.texto.indexOf('Gertrudes') >= 0],
    ['prenome solto apontado vira token (com acento)', !/Gertrudes|Leocádia/.test(com.texto)],
    ['palavra que também aparece em minúscula fica', /a Rosa não falou/.test(com.texto)],
    ['precedente citado fica inteiro', /Teodoro Precedente versus Estado/.test(com.texto)],
    ['a lista CONFIRA conta o que entrou e o que não entrou', /Tarjados a mais: 2/.test(com.confira) && /Apontados pelo modelo e NÃO tarjados \(confira\): 2/.test(com.confira)],
    ['o mapa leva os nomes novos (a reidentificação devolve)', M.reidentificar(com.texto, com.mapa).texto.indexOf('Leocádia') >= 0],
  ].filter((c) => !c[1]);
  rig.forEach((c) => console.log('✗ rigoroso: ' + c[0]));
  console.log('RIGOROSO: ' + (6 - rig.length) + ' ok, ' + rig.length + ' falhas, de 6');
  // 6 ─ vários documentos do mesmo caso: um mapa só, as páginas repartidas, o nome do arquivo sem nomes (dados fictícios)
  const dA = { nome: '1 - 13h - custódia - 1000999-11.2026.8.11.0099.pdf',
    texto: 'PODER JUDICIÁRIO DO ESTADO DE MATO GROSSO\nProcesso nº 1000999-11.2026.8.11.0099\nAUTOR: MINISTÉRIO PÚBLICO DO ESTADO DE MATO GROSSO\n'
      + 'FLAGRANTEADO: CLODOALDO TESTEFULANO SALAMANDRA\nTERMO DE AUDIÊNCIA DE CUSTÓDIA\nPresente o flagranteado CLODOALDO TESTEFULANO SALAMANDRA, '
      + 'brasileiro, inscrito no CPF nº 529.982.247-25, filho de ZULEIDE TESTEFULANA SALAMANDRA.\f'
      + 'DECISÃO\nConverto a prisão de CLODOALDO TESTEFULANO SALAMANDRA em preventiva.\f' };
  const dB = { nome: 'relatorio janus - não juntar - CLODOALDO TESTEFULANO SALAMANDRA.pdf',
    texto: 'RELATÓRIO DE INTELIGÊNCIA\nAlvo: CLODOALDO TESTEFULANO SALAMANDRA, CPF 529.982.247-25.\nO alvo foi visto com ROSIMEIRE TESTEBELTRANA DIAS, '
      + 'sua companheira, na residência do casal.\f' };
  const dC = { nome: '2 - 13h15 - custódia - 1000888-22.2026.8.11.0088.pdf',
    texto: 'PODER JUDICIÁRIO DO ESTADO DE MATO GROSSO\nProcesso nº 1000888-22.2026.8.11.0088\nFLAGRANTEADO: GENOVEVO TESTECICRANO PIRES\n'
      + 'TERMO DE AUDIÊNCIA DE CUSTÓDIA\nPresente o flagranteado GENOVEVO TESTECICRANO PIRES, inscrito no CPF nº 111.444.777-35.\f' };
  const reais = /CLODOALDO|SALAMANDRA|ROSIMEIRE|ZULEIDE|529\.982/;
  const cAB = M.anonimizarCaso([dA, dB], { incluirMapa: true });
  const sA = M.anonimizarCaso([dA], {}).assinatura, sB = M.anonimizarCaso([dB], {}).assinatura, sC = M.anonimizarCaso([dC], {}).assinatura;
  const comum = (x, y) => x.filter((k) => y.indexOf(k) >= 0);
  const tokReu = (Object.keys(cAB.mapa.tokens).filter((t) => cAB.mapa.tokens[t].valor === 'CLODOALDO TESTEFULANO SALAMANDRA')[0]) || '?';
  const um = M.anonimizarCaso([dA], {}), solo = M.anonimizar(dA.texto, {});
  const caso = [
    ['dois documentos, cada um com as suas páginas', cAB.documentos.length === 2 && cAB.documentos[0].paginas.length === 2 && cAB.documentos[1].paginas.length === 1],
    ['as páginas de cada documento começam em 1', /^===== PÁGINA 1 =====/.test(cAB.documentos[1].paginas[0])],
    ['a mesma pessoa leva o mesmo token nos dois', cAB.documentos.every((d) => d.paginas.join('\n').indexOf(tokReu) >= 0)],
    ['nenhum nome real nas páginas nem no cabeçalho', !cAB.documentos.some((d) => reais.test(d.paginas.join('\n')) || reais.test(d.texto)) && !reais.test(cAB.cabecalho)],
    ['o nome do arquivo sai sem o nome da pessoa', cAB.documentos[1].nomeSaida === 'relatorio janus - não juntar - ' + tokReu],
    ['o nome do arquivo sem pessoa fica igual', cAB.documentos[0].nomeSaida === '1 - 13h - custódia - 1000999-11.2026.8.11.0099'],
    ['o cabeçalho lista os documentos do caso', /DOCUMENTOS DESTE CASO \(2/.test(cAB.cabecalho) && cAB.cabecalho.indexOf(tokReu) >= 0],
    ['a assinatura liga A e B (CPF e nome)', comum(sA.ids, sB.ids).length > 0 && comum(sA.pessoas, sB.pessoas).length > 0],
    ['a assinatura não liga A e C', !comum(sA.ids, sC.ids).length && !comum(sA.pessoas, sC.pessoas).length && !comum(sA.cnj, sC.cnj).length],
    ['o número do processo do nome do arquivo entra na assinatura', sA.cnj.indexOf('1000999-11.2026.8.11.0099') >= 0],
    ['um documento só: as mesmas páginas da anonimização de sempre', JSON.stringify(um.documentos[0].paginas) === JSON.stringify(solo.paginas) && um.cabecalho === solo.cabecalho],
    ['o nome do arquivo sem pessoa não vira parte', M.partesDoNomeDeArquivo('2 - 13h15 - custódia - 1000888-22.2026.8.11.0088.pdf').length === 0
      && M.partesDoNomeDeArquivo('Relatório Final - Delegacia de Polícia.pdf').length === 0],
    ['um papel tomado por nome não troca a palavra no nome do arquivo', M.anonimizarCaso([{ nome: 'termo da companheira.txt',
      texto: 'TERMO DE DECLARAÇÕES\nDeclarante: ROSIMEIRE TESTEBELTRANA DIAS, companheira do flagranteado.\nQUE conhece o flagranteado há cinco anos.\n\f' }], {}).documentos[0].nomeSaida === 'termo da companheira'],
    ['a reidentificação devolve os nomes dos dois documentos', M.reidentificar(cAB.documentos[1].texto, cAB.mapa).texto.indexOf('ROSIMEIRE TESTEBELTRANA DIAS') >= 0],
  ].filter((c) => !c[1]);
  caso.forEach((c) => console.log('✗ caso: ' + c[0]));
  console.log('CASO: ' + (14 - caso.length) + ' ok, ' + caso.length + ' falhas, de 14');
  if (ruins.length || rig.length || caso.length) falhou = true;
}
process.exitCode = falhou ? 1 : 0;
