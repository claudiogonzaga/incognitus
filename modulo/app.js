// Incognitus — a janela: a fila de PDFs e o fluxo de cada um, igual ao lote automático do Graphé, e a reidentificação.
//   extrair (pdftotext -layout) → páginas fracas ao OCR (2 de cada vez) → anonimizar (motor, no worker) → gravar
// Um PDF por vez. A pasta temporária de cada PDF é apagada ao fim dele, com ou sem erro.
'use strict';
const T = window.__TAURI__;
// Os comandos do módulo são do plugin `anonimizacao` (plugin/): valem no app avulso e hospedado no Graphé Desktop. Os da
// atualização do APP (app_checar, app_baixar, app_instalar) são só do app avulso e vão sem o prefixo.
const PLUGIN = 'plugin:anonimizacao|';
const invoke = (cmd, args) => T.core.invoke(/^app_/.test(cmd) ? cmd : PLUGIN + cmd, args);
const $ = (id) => document.getElementById(id);
const avisar = (texto) => T.dialog.message(texto, { title: 'Incognitus', kind: 'warning' }).catch(() => {});

const registrar = (m) => invoke('registrar', { msg: String(m) }).catch(() => {});
window.addEventListener('error', (e) => registrar('erro: ' + e.message + ' @' + e.filename + ':' + e.lineno));
window.addEventListener('unhandledrejection', (e) => registrar('promessa: ' + ((e.reason && e.reason.message) || e.reason)));

const estado = {
  fila: [], rodando: false, parar: false, proximoId: 1,
  ajustes: { saida: 'ao_lado', pastaSaida: '', txt: false, rigoroso: false, juntar: true, contatos: null, checarAoAbrir: true },
  motorCaso: false,
  instPadrao: '', motor: null, worker: null, casos: null, e2e: null,
  aba: 'anon', reid: { mapa: null, texto: null, itens: [] },
};
const ETAPAS = { fila: 'na fila', extraindo: 'extraindo', ocr: 'OCR', anonimizando: 'anonimizando', rigoroso: 'leitura rigorosa', gravando: 'gravando o PDF', pronto: 'pronto', erro: 'erro', lido: 'lido · esperando os outros', refazer: 'a refazer (caso mudou)' };

/** A promessa, ou erro depois de `ms` (um motor que não responde não pode travar a janela nem a atualização). */
function comPrazo(p, ms, msg) {
  let t;
  return Promise.race([p, new Promise((_, falha) => { t = setTimeout(() => falha(new Error(msg)), ms); })]).finally(() => clearTimeout(t));
}

// ── o worker do motor ─────────────────────────────────────────────────────────────────────────────────────────────
const textos = {};
async function texto(arq) {
  if (textos[arq]) return textos[arq];
  registrar('fetch ' + arq + ' (' + location.href + ')');
  const r = await fetch(arq);
  registrar('fetch ' + arq + ' → ' + r.status + ' ' + r.headers.get('content-type'));
  return (textos[arq] = await r.text());
}

/** Um worker com o motor `js` (+ casos.js + worker.js). Devolve { pedir(tipo, dados), fim() }. */
async function criarWorker(js) {
  registrar('worker: montando (' + js.length + ' chars)');
  const url = URL.createObjectURL(new Blob([js, '\n;\n', await texto('casos.js'), '\n;\n', await texto('worker.js')], { type: 'text/javascript' }));
  const w = new Worker(url);
  const espera = new Map();
  let n = 0;
  w.onmessage = (ev) => {
    const m = ev.data, p = espera.get(m.id);
    if (!p) return;
    espera.delete(m.id);
    m.ok ? p.ok(m.r) : p.falha(new Error(m.erro));
  };
  w.onerror = (ev) => {
    ev.preventDefault();
    for (const p of espera.values()) p.falha(new Error('o motor falhou: ' + (ev.message || 'erro no worker')));
    espera.clear();
  };
  const pedir = (tipo, dados) => new Promise((ok, falha) => {
    const id = ++n;
    espera.set(id, { ok, falha });
    w.postMessage(Object.assign({ id, tipo }, dados || {}));
  });
  registrar('worker: criado');
  try { await comPrazo(pedir('versao'), 30000, 'o motor não respondeu ao carregar'); }
  catch (e) { w.terminate(); throw e; }
  finally { URL.revokeObjectURL(url); }
  registrar('worker: respondeu');
  return { pedir, fim: () => w.terminate() };
}

function cfgMotor() {
  return estado.ajustes.contatos == null ? {} : { contatosInstitucionais: estado.ajustes.contatos };
}

/** Carrega o motor em uso (o baixado e conferido, ou o embutido) num worker novo. */
async function carregarMotor() {
  const ativo = await invoke('motor_ativo');
  registrar('motor_ativo: ' + ativo.origem);
  const js = ativo.js || await texto('motor/motor.js');
  const novo = await criarWorker(js);
  estado.motorCaso = !!(await novo.pedir('versao')).caso;
  const padrao = await novo.pedir('configurar', { cfg: {} });
  estado.instPadrao = padrao.contatosInstitucionais;
  await novo.pedir('configurar', { cfg: cfgMotor() });
  if (estado.worker) estado.worker.fim();
  estado.worker = novo;
  estado.motor = ativo;
  mostrarMotor();
}

function dataBr(iso) {
  const d = new Date(iso);
  return isNaN(d) ? iso : d.toLocaleString('pt-BR', { dateStyle: 'short', timeStyle: 'short' });
}

function mostrarMotor() {
  const m = estado.motor.manifesto;
  const origem = estado.motor.origem === 'baixado' ? 'atualizado' : 'embutido';
  $('bt_motor').textContent = 'motor ' + m.versao + ' · ' + origem;
  $('rodape_motor').textContent = 'Motor de anonimização ' + m.versao + ' (' + origem + ', ' + dataBr(m.data) + ')';
  $('motor_info').innerHTML = 'Em uso: <b>' + esc(m.versao) + '</b> — ' + (estado.motor.origem === 'baixado' ? 'baixado do repositório das regras' : 'o que veio com o app') +
    '<br>Publicado em ' + esc(dataBr(m.data)) + ' · sha256 <code>' + esc(m.sha256.slice(0, 16)) + '…</code>' +
    (estado.motor.anterior ? '<br>Anterior guardado: ' + esc(estado.motor.anterior.versao) + ' (' + esc(dataBr(estado.motor.anterior.data)) + ')' : '');
  $('bt_voltar').disabled = !(estado.motor.anterior || estado.motor.origem === 'baixado');
}

// ── atualização das regras ────────────────────────────────────────────────────────────────────────────────────────
/** Baixa (se houver), confere assinatura e sha256 (no Rust), roda os casos embutidos num worker à parte e só então troca. */
async function atualizarRegras(manual) {
  const msg = (t) => { $('motor_msg').textContent = t; };
  try {
    msg('Procurando regras novas…');
    const novo = await invoke('motor_checar', { manual });
    if (!novo) { msg(manual ? 'As regras em uso já são as mais novas.' : ''); return { trocou: false }; }
    msg('Baixando as regras ' + novo.versao + '…');
    const b = await invoke('motor_baixar');
    msg('Assinatura e sha256 conferidos. Rodando os ' + estado.casos.casos.length + ' casos de teste…');
    const teste = await criarWorker(b.js);
    let res;
    try {
      const v = await teste.pedir('versao');
      if (v.api !== 2) throw new Error('o motor novo fala outra API (' + v.api + ')');
      res = await comPrazo(teste.pedir('testar', { pacote: estado.casos }), 300000, 'os casos de teste não terminaram em 5 minutos');
    } finally { teste.fim(); }
    if (res.falhas.length || res.ok !== res.total) {
      msg('As regras ' + b.manifesto.versao + ' NÃO foram instaladas: ' + res.falhas.length + ' caso(s) de teste falharam (' +
        res.falhas.slice(0, 3).map((f) => f.id).join(', ') + '). O motor em uso continua o mesmo.');
      return { trocou: false, falhas: res.falhas };
    }
    if (estado.rodando) await new Promise((ok) => { estado.aoParar = ok; });
    await invoke('motor_instalar', { sha256: b.manifesto.sha256 });
    await carregarMotor();
    msg('Regras atualizadas para ' + b.manifesto.versao + ' (' + res.ok + '/' + res.total + ' casos de teste ok).');
    return { trocou: true, versao: b.manifesto.versao };
  } catch (e) {
    msg((manual ? '' : 'Checagem das regras: ') + String(e.message || e));
    return { trocou: false, erro: String(e.message || e) };
  }
}

// ── a fila ────────────────────────────────────────────────────────────────────────────────────────────────────────
function esc(s) { return String(s == null ? '' : s).replace(/[&<>"]/g, (c) => ({ '&': '&amp;', '<': '&lt;', '>': '&gt;', '"': '&quot;' }[c])); }
function nomeDe(c) { return c.split(/[\\/]/).pop(); }

async function adicionar(caminhos) {
  const pdfs = await invoke('expandir_caminhos', { caminhos });
  const ja = new Set(estado.fila.filter((i) => i.etapa !== 'pronto' && i.etapa !== 'erro').map((i) => i.caminho));
  for (const c of pdfs) if (!ja.has(c)) { estado.fila.push({ id: estado.proximoId++, caminho: c, nome: nomeDe(c), etapa: 'fila', avisos: [] }); ja.add(c); }
  desenhar();
}

function progresso(i) {
  switch (i.etapa) {
    case 'fila': case 'refazer': return 0;
    case 'lido': return 55;
    case 'extraindo': return 8;
    case 'ocr': return 10 + 60 * (i.ocr ? i.ocr.feitas / Math.max(1, i.ocr.total) : 0);
    case 'anonimizando': return i.ocr ? 75 : 50;
    case 'rigoroso': return 76 + 15 * (i.ner ? i.ner.feitas / Math.max(1, i.ner.total) : 0);
    case 'gravando': return 92;
    default: return 100;
  }
}

const CORES_CASO = ['#8a5a2b', '#3f6b5a', '#6b4a7a', '#2f5d7c', '#8a3b3b', '#5b6b2f'];
function corCaso(chave) { let h = 0; for (const ch of String(chave)) h = (h * 31 + ch.charCodeAt(0)) >>> 0; return CORES_CASO[h % CORES_CASO.length]; }
function desenhar() {
  const f = $('fila');
  f.innerHTML = estado.fila.map((i) => {
    const trab = ['extraindo', 'ocr', 'anonimizando', 'rigoroso', 'gravando'].includes(i.etapa);
    let chip = ETAPAS[i.etapa];
    if (i.etapa === 'ocr' && i.ocr) chip = 'OCR ' + i.ocr.feitas + '/' + i.ocr.total;
    if (i.etapa === 'rigoroso') chip = i.nerMsg ? i.nerMsg : 'leitura rigorosa ' + (i.ner ? i.ner.feitas + '/' + i.ner.total : '');
    const det = [];
    if (i.paginas) det.push('<b>' + i.paginas + '</b> página' + (i.paginas > 1 ? 's' : ''));
    if (i.fracas != null) det.push(i.fracas ? i.fracas + ' fraca(s) → OCR' : 'nenhuma página para OCR');
    if (i.stats) det.push('<b>' + i.stats.pessoas + '</b> pessoa(s) · ' + i.stats.tokens + ' token(s)' +
      (i.stats.naoConfirmados ? ' · ' + i.stats.naoConfirmados + ' candidato(s) a nome em claro — quase sempre termos que não são nomes; confira' : ''));
    if (i.segundos) det.push(i.segundos + ' s');
    if (i.motor) det.push('motor ' + i.motor);
    const c = i.caso, varios = c && c.itens.length > 1;
    const outros = [...new Map(estado.fila.filter((x) => x.caso && x !== i && (!c || x.caso.chave !== c.chave) && x.doc)
      .map((x) => [x.caso.chave, x.caso])).values()];
    const linhaCaso = !c ? '' : '<div class="caso"' + (varios ? ' style="--cor-caso:' + corCaso(c.chave) + '"' : '') + '>' +
      (varios ? '<b>' + esc(c.rotulo) + '</b> · ' + c.itens.length + ' documentos anonimizados juntos, um mapa só' + (c.motivo ? ' (' + esc(c.motivo) + ')' : '')
              : 'documento sozinho (nenhum outro da leva é do mesmo caso)') +
      (i.doc && !estado.rodando ? (varios ? ' <button class="ghost mini" data-separar="' + i.id + '">Separar</button>' : '') +
        (outros.length ? ' <select class="mini" data-juntar="' + i.id + '"><option value="">Juntar a…</option>' +
          outros.map((o) => '<option value="' + esc(o.chave) + '">' + esc(o.rotulo) + ' (' + o.itens.length + ')</option>').join('') + '</select>' : '') : '') +
      '</div>';
    return '<li class="item' + (trab ? ' ativo' : '') + (i.etapa === 'erro' ? ' erro' : '') + (varios ? ' do-caso' : '') + '"' + (varios ? ' style="--cor-caso:' + corCaso(c.chave) + '"' : '') + '>' +
      '<div class="cab"><span class="nome">' + esc(i.nome) + '</span><span class="chip ' + (i.etapa === 'pronto' ? 'pronto' : i.etapa === 'erro' ? 'erro' : trab ? 'trabalhando' : '') + '">' + esc(chip) + '</span></div>' +
      '<div class="barra"><i style="width:' + progresso(i) + '%"></i></div>' +
      (det.length ? '<div class="det">' + det.join(' · ') + '</div>' : '') + linhaCaso +
      (i.saida ? '<div class="det">saiu como <b>' + esc(nomeDe(i.saida.pdf)) + '</b></div>' : '') +
      (i.avisos.length ? '<div class="det">⚠ ' + esc(i.avisos.join(' · ')) + '</div>' : '') +
      (i.erro ? '<div class="msg-erro">' + esc(i.erro) + '</div>' : '') +
      (i.saida ? '<div class="acoes"><button class="ghost" data-revelar="' + esc(i.saida.pdf) + '">Mostrar na pasta</button>' +
        (i.saida.mapa ? '<button class="ghost" data-mapa="' + esc(i.saida.mapa) + '">Reidentificar com este mapa</button>' : '') +
        (i.saida.confira ? '<button class="ghost" data-revelar="' + esc(i.saida.confira) + '">Lista CONFIRA</button>' : '') + '</div>' : '') +
      '</li>';
  }).join('');
  $('vazio').hidden = estado.fila.length > 0;
  const prontos = estado.fila.filter((i) => i.etapa === 'pronto').length, erros = estado.fila.filter((i) => i.etapa === 'erro').length;
  const pendentes = estado.fila.filter((i) => i.etapa === 'fila' || i.etapa === 'refazer').length;
  $('resumo').textContent = estado.fila.length ? prontos + ' de ' + estado.fila.length + ' pronto(s)' + (erros ? ' · ' + erros + ' erro(s)' : '') : '';
  $('bt_rodar').disabled = estado.rodando || !pendentes || !estado.worker;
  $('bt_rodar').textContent = estado.rodando ? 'Anonimizando…' : 'Anonimizar' + (pendentes ? ' (' + pendentes + ')' : '');
  $('bt_parar').hidden = !estado.rodando;
  $('bt_limpar').hidden = !prontos;
}

/** Roda `fn` sobre os itens, `n` de cada vez. */
async function emParalelo(itens, n, fn) {
  let k = 0;
  const vez = async () => { while (k < itens.length) { const x = itens[k++]; await fn(x); } };
  await Promise.all(Array.from({ length: Math.min(n, itens.length) }, vez));
}

// ── o prazo do motor ──────────────────────────────────────────────────────────────────────────────────────────────
// Uma linha patológica de formulário já travou uma expressão regular do motor por mais de um minuto: cada pedido ao
// worker tem prazo proporcional às páginas; estourado, o worker é recriado e o PDF sai com erro, sem travar a fila.
const MSG_PRAZO = 'o motor passou do prazo';
// (14/11/2026: no Mac o WebView roda o motor no JavaScriptCore, várias vezes mais lento que o V8 em autos grandes —
// 112 páginas reais levaram 289 s —, daí 6 s por página)
function prazoMotor(paginas) { return 120000 + 6000 * (paginas || 1); }
async function pedirMotor(tipo, dados, paginas) {
  try { return await comPrazo(estado.worker.pedir(tipo, dados), prazoMotor(paginas), MSG_PRAZO + ' (' + tipo + ', ' + Math.round(prazoMotor(paginas) / 1000) + ' s)'); }
  catch (e) {
    if (String(e.message || e).startsWith(MSG_PRAZO)) { registrar('motor: prazo estourado em ' + tipo + '; recriando'); await carregarMotor().catch(() => {}); }
    throw e;
  }
}

// ── a anonimização rigorosa (opcional) ────────────────────────────────────────────────────────────────────────────
// Depois do motor, um modelo de NER (BERT, inples-tech/anonimato-ner-ptbr) lê as páginas já anonimizadas e aponta o que
// ainda parece nome ou endereço; o motor decide o que entra (sem autoridade, precedente, palavra comum) e refaz o texto.
// O modelo (110 MB) é baixado na primeira vez; vários leitores (Web Workers, WebAssembly) rodam em paralelo.
const ner = { leitores: null, preparo: null };
function criarLeitor(_, k) {
  const w = new Worker('ner_worker.mjs', { type: 'module' });
  const espera = new Map();
  let n = 0;
  w.onmessage = (ev) => {
    const m = ev.data;
    if (m.passo) { if (estado.e2e) registrar('leitor ' + k + ': ' + m.passo); return; }
    const p = espera.get(m.id); if (!p) return; espera.delete(m.id); m.ok ? p.ok(m.r) : p.falha(new Error(m.erro)); };
  w.onerror = (ev) => { ev.preventDefault(); for (const p of espera.values()) p.falha(new Error('o leitor do modelo falhou: ' + (ev.message || 'erro no worker'))); espera.clear(); };
  const pedir = (tipo, dados, transferir) => new Promise((ok, falha) => { const id = ++n; espera.set(id, { ok, falha }); w.postMessage(Object.assign({ id, tipo }, dados || {}), transferir || []); });
  return { pedir, fim: () => w.terminate() };
}
async function baixarModelo(aoAndar) {
  let est = await invoke('ner_estado');
  if (!est.baixado) { if (aoAndar) aoAndar('baixando o modelo (' + est.megas + ' MB, só na primeira vez)…'); est = await invoke('ner_baixar'); }
  return est;
}
async function prepararLeitores(aoAndar) {
  if (ner.leitores) return ner.leitores;
  if (!ner.preparo) {
    ner.preparo = (async () => {
      const est = await baixarModelo(aoAndar);
      if (aoAndar) aoAndar('carregando o modelo…');
      const modelo = await invoke('ner_ler', { arquivo: 'model.onnx' });
      const tok = new TextDecoder().decode(await invoke('ner_ler', { arquivo: 'tokenizer.json' }));
      const n = Math.max(1, Math.min(4, Math.floor((navigator.hardwareConcurrency || 4) / 2)));
      const lista = Array.from({ length: n }, criarLeitor);
      // um de cada vez (menos memória no pico), cada um com a sua cópia dos bytes, transferida (sem clonar 110 MB)
      try {
        for (const [k, l] of lista.entries()) {
          if (estado.e2e) registrar('leitor ' + k + ': iniciando');
          const copia = (modelo instanceof ArrayBuffer ? modelo : modelo.buffer).slice(0);
          await comPrazo(l.pedir('iniciar', { modelo: copia, tokenizer: tok, rotulos: est.rotulos }, [copia]), 180000, 'o modelo não carregou em 3 minutos');
        }
      } catch (e) { lista.forEach((l) => l.fim()); throw e; }
      registrar('rigoroso: ' + n + ' leitor(es) prontos');
      return (ner.leitores = lista);
    })();
  }
  try { return await ner.preparo; } catch (e) { ner.preparo = null; throw e; }
}
/** As páginas já anonimizadas → o que o modelo achou que ainda parece nome ou endereço (para o motor decidir). */
async function lerRigoroso(i, paginas) {
  const leitores = await prepararLeitores((t) => { i.nerMsg = t; desenhar(); });
  i.nerMsg = null; i.ner = { feitas: 0, total: paginas.length }; desenhar();
  const extras = [];
  let k = 0;
  await Promise.all(leitores.map(async (L) => {
    while (k < paginas.length) {
      const idx = k++, pag = paginas[idx];
      if (estado.e2e) registrar('rigoroso: página ' + (idx + 1) + ' (' + pag.length + ' caracteres)');
      // o prazo cresce com a página: cada janela de 510 peças leva uns 6 s num leitor (a página de diagramação larga, com
      // milhares de linhas, tem várias janelas); umas 3 letras por peça
      const janelas = Math.max(1, Math.ceil((pag.replace(/\s+/g, '').length / 3 - 510) / 384) + 1);
      const achados = await comPrazo(L.pedir('reconhecer', { texto: pag }), 120000 + 60000 * janelas, 'a leitura rigorosa travou na página ' + (idx + 1));
      // o que já é do motor (o nome grego de um token, [omitido]) sai; o resto encostado num token é pedaço de nome que
      // o OCR estragou ("«Thalia» SgRGIO") e vai marcado como colado
      const zonas = [...pag.matchAll(/«[^«»\n]{0,80}»|\[omitido\]/g)].map((m) => [m.index, m.index + m[0].length]);
      for (const a of achados) {
        if (a.tipo !== 'PESSOA' && a.tipo !== 'ENDERECO') continue;
        const partes = [];
        let pos = a.ini, tocou = false;
        for (const [zi, zf] of zonas) {
          if (zf < a.ini - 2 || zi > a.fim + 2) continue;
          tocou = true;
          if (zf <= a.ini || zi >= a.fim) continue;
          if (zi > pos) partes.push(pag.slice(pos, zi));
          pos = Math.max(pos, zf);
        }
        if (pos < a.fim) partes.push(pag.slice(pos, a.fim));
        const nome = partes.join(' ').replace(/[«»]/g, ' ').replace(/\s+/g, ' ').replace(/^[\s,;:.\-]+|[\s,;:\-]+$/g, '').trim();
        if (!nome || nome.length < 2 || !/\p{L}{2}/u.test(nome)) continue;
        extras.push({ nome, tipo: a.tipo, pagina: idx + 1, prob: a.prob, colado: tocou, ctx: pag.slice(Math.max(0, a.ini - 60), a.fim + 60) });
      }
      if (estado.e2e) registrar('rigoroso: página ' + (idx + 1) + ' lida, ' + achados.length + ' achado(s)');
      i.ner.feitas++; desenhar();
    }
  }));
  return extras;
}

// ── a fila em três passos (14/11/2026): ler cada arquivo → juntar os do mesmo caso → anonimizar e gravar cada caso ────
// O mesmo caso (os autos, o relatório, o termo…) vai ao motor de uma vez: um mapa só, a mesma pessoa com o mesmo token em
// todos. O que liga dois documentos: o número do processo (no nome do arquivo ou na capa), um documento de pessoa (CPF,
// RG, CNH, telefone, placa, e-mail) ou a mesma pessoa (nome com três palavras ou mais, ou dois nomes em comum). A fila
// mostra o caso e o motivo; Separar e Juntar corrigem à mão (e o caso é refeito).
const ehPdf = (c) => /\.pdf$/i.test(c);

/** Um .txt ou .docx em páginas de 55 linhas (o PDF de saída tem uma folha por página), ou nas quebras de página dele. */
function paginarTexto(t) {
  t = String(t || '').replace(/\r\n?/g, '\n');
  const blocos = t.indexOf('\f') >= 0 ? t.split('\f') : [t];
  const pags = [];
  for (const b of blocos) {
    const ls = b.split('\n');
    for (let k = 0; k < ls.length; k += 55) pags.push(ls.slice(k, k + 55).join('\n'));
  }
  return pags.filter((p, k) => p.trim() || k < pags.length - 1).join('\f') + '\f';
}

/** Passo 1: o texto (pdftotext + OCR, ou o .txt/.docx) e a anonimização só dele (para saber quem está nele). */
async function lerArquivo(i) {
  const t0 = Date.now();
  if (estado.e2e) registrar('lendo ' + i.nome);
  let prep = null;
  i.avisos = []; i.erro = null; i.saida = null; i.stats = null; i.ocr = null; i.fracas = null; i.ner = null; i.nerMsg = null; i.caso = null;
  try {
    if (!estado.motorCaso) throw new Error('o motor em uso é antigo (sem a anonimização por caso): volte ao motor embutido nos Ajustes');
    i.etapa = 'extraindo'; desenhar();
    let oc, npag;
    if (ehPdf(i.caminho)) {
      prep = await invoke('preparar_arquivo', { caminho: i.caminho });
      const lido = await pedirMotor('ler', { saida: await invoke('extrair_texto', { id: prep.id }) }, 50);
      i.paginas = npag = lido.paginas; i.fracas = lido.fracas.length; desenhar();
      const lidos = {};
      if (lido.fracas.length) {
        i.etapa = 'ocr'; i.ocr = { feitas: 0, total: lido.fracas.length }; desenhar();
        await emParalelo(lido.fracas, 2, async (n) => {
          try { lidos[n] = await invoke('ocr_pagina', { id: prep.id, pagina: n }); }
          catch (e) { lidos[n] = null; i.avisos.push('OCR da página ' + n + ' falhou (' + e + ')'); }
          i.ocr.feitas++; desenhar();
        });
      }
      oc = await pedirMotor('ocr', { texto: lido.texto, fracas: lido.fracas, lidos }, lido.paginas);
      if (estado.e2e) {
        await invoke('e2e_gravar', { nome: i.nome + '.pdftotext.txt', conteudo: lido.texto });
        await invoke('e2e_gravar', { nome: i.nome + '.extracao.txt', conteudo: oc.texto });
        i.e2e = { fracas: lido.fracas, ocr: oc.meta.ocr };
      }
    } else {
      const texto = paginarTexto(await invoke('texto_documento', { caminho: i.caminho }));
      npag = (texto.match(/\f/g) || []).length;
      oc = await pedirMotor('ocr', { texto, fracas: [], lidos: {} }, npag);
      i.paginas = npag; i.fracas = null;
    }
    i.doc = { nome: i.nome, texto: oc.texto, meta: oc.meta };
    i.etapa = 'anonimizando'; desenhar();
    i.solo = await pedirMotor('caso', { docs: [i.doc], comTexto: $('op_txt').checked }, npag);
    i.assin = i.solo.assinatura; i.motor = i.solo.versao; i.stats = i.solo.estatisticas;
    i.etapa = 'lido';
    return true;
  } catch (e) {
    i.etapa = 'erro'; i.erro = String((e && e.message) || e);
    if (estado.e2e) registrar('erro em ' + i.nome + ': ' + i.erro);
    return false;
  } finally {
    if (prep) await invoke('limpar_arquivo', { id: prep.id }).catch(() => {});
    i.segundos = Math.round((Date.now() - t0) / 1000);
    desenhar();
  }
}

/** Por que dois documentos são do mesmo caso (ou null). */
function ligacao(a, b) {
  const comum = (x, y) => (x || []).filter((k) => (y || []).indexOf(k) >= 0);
  if (comum(a.cnj, b.cnj).length) return 'mesmo número de processo';
  if (comum(a.ids, b.ids).length) return 'mesmo documento de pessoa (' + comum(a.ids, b.ids)[0].split(':')[0] + ')';
  // (a assinatura só traz nome de três palavras ou mais, ou nome de PARTE: dois nomes curtos em comum juntavam custódias
  // diferentes da mesma região — bateria de 14/11/2026)
  if (comum(a.pessoas, b.pessoas).length) return 'mesma pessoa';
  return null;
}

let proximoCaso = 1;
/** Passo 2: os grupos. A escolha à mão (Separar, Juntar) vale; o resto se liga pela assinatura. */
function agrupar(leva) {
  const pai = new Map(leva.map((i) => [i, i])), motivo = new Map();
  const raiz = (x) => { while (pai.get(x) !== x) x = pai.get(x); return x; };
  const unir = (a, b, m) => { const ra = raiz(a), rb = raiz(b); if (ra !== rb) { pai.set(rb, ra); if (m) motivo.set(ra, motivo.get(ra) || m); } };
  const juntar = estado.ajustes.juntar !== false;
  for (let x = 0; x < leva.length; x++) for (let y = x + 1; y < leva.length; y++) {
    const a = leva[x], b = leva[y];
    if (a.casoManual || b.casoManual) { if (a.casoManual && a.casoManual === b.casoManual && !a.casoManual.startsWith('só:')) unir(a, b, 'juntados à mão'); continue; }
    if (!juntar) continue;
    const m = ligacao(a.assin, b.assin);
    if (m) unir(a, b, m);
  }
  const grupos = new Map();
  for (const i of leva) { const r = raiz(i); if (!grupos.has(r)) grupos.set(r, []); grupos.get(r).push(i); }
  return [...grupos.entries()].map(([r, itens]) => {
    const cnj = itens.flatMap((i) => i.assin.cnj || []);
    const n = proximoCaso++;
    return { itens, motivo: motivo.get(r) || '', rotulo: cnj.length ? 'processo ' + cnj[0] : 'caso ' + n, chave: 'c' + n };
  });
}

/** Passo 3: o caso inteiro no motor (um documento só já veio pronto do passo 1), a leitura rigorosa e a gravação. */
async function gravarCaso(g, nomesUsados) {
  const t0 = Date.now(), comTexto = $('op_txt').checked;
  const its = g.itens, todas = its.reduce((n, i) => n + (i.paginas || 1), 0);
  const marcar = (etapa) => { its.forEach((i) => { i.etapa = etapa; i.caso = g; }); desenhar(); };
  try {
    marcar('anonimizando');
    let r = its.length === 1 && !estado.ajustes.rigoroso ? its[0].solo : await pedirMotor('caso', { docs: its.map((i) => i.doc), comTexto }, todas);
    if (estado.ajustes.rigoroso) {
      marcar('rigoroso');
      const prog = { nerMsg: null, ner: null };
      const espelho = new Proxy(prog, { set: (o, k, v) => { o[k] = v; its.forEach((i) => { i[k] = v; }); return true; } });
      const extras = await lerRigoroso(espelho, r.paginasCaso);
      marcar('anonimizando');
      r = await pedirMotor('caso', { docs: its.map((i) => i.doc), comTexto, extras, rigoroso: true }, todas);
    }
    marcar('gravando');
    const destino = estado.ajustes.saida === 'outra' ? estado.ajustes.pastaSaida : null;
    for (const [k, i] of its.entries()) {
      const d = r.documentos[k];
      let nome = d.nomeSaida, n = 2;
      while (nomesUsados.has(nome.toLowerCase())) nome = d.nomeSaida + ' (' + n++ + ')';
      nomesUsados.add(nome.toLowerCase());
      i.motor = r.versao; i.stats = r.estatisticas;
      if (r.estatisticas.falhas) i.avisos.push(r.estatisticas.falhas + ' etapa(s) do motor falharam: veja a lista CONFIRA');
      if (estado.ajustes.rigoroso) i.avisos.push('leitura rigorosa: ' + r.estatisticas.rigorosoAceitos + ' tarjado(s) a mais, ' + r.estatisticas.rigorosoRecusados + ' para conferir');
      i.saida = await invoke('gravar_saida', { pedido: {
        origem: i.caminho, destino, versao_motor: r.versao, nome_saida: nome,
        cabecalho: r.cabecalho.normalize('NFC'), paginas: d.paginas.map((p) => p.normalize('NFC')),
        texto: comTexto ? d.texto : null, mapa: r.mapaJson, confira: r.confira,
      } });
      if (i.saida.sem_glifo) i.avisos.push(i.saida.sem_glifo + ' caractere(s) sem desenho na fonte viraram "?" no PDF' + (comTexto ? ' (o .txt tem o original)' : ''));
      i.etapa = 'pronto';
    }
  } catch (e) {
    its.forEach((i) => { if (i.etapa !== 'pronto') { i.etapa = 'erro'; i.erro = String((e && e.message) || e); } });
    if (estado.e2e) registrar('erro no caso ' + g.rotulo + ': ' + String((e && e.message) || e));
  } finally {
    const s = Math.round((Date.now() - t0) / 1000);
    its.forEach((i) => { i.segundos = (i.segundos || 0) + s; });
    desenhar();
  }
}

// Com a janela escondida atrás de outras ou minimizada, o WebView suspende os timers e os workers da página e o lote
// para (visto em 14/11/2026: a leitura rigorosa parada, sem CPU, até a janela voltar). No Mac, a janela tem
// "backgroundThrottling": "disabled" (tauri.conf.json, macOS 14+); no Windows, onde isso não existe, uma Web Lock
// pendente enquanto a fila roda segura a página acordada.
function manterAcordado(promessa) {
  try { if (navigator.locks) navigator.locks.request('incognitus-fila', () => promessa.catch(() => {})); } catch (e) { /* sem Web Locks */ }
  // no Mac, com a janela fora da tela, o sistema suspende a página inteira (o backgroundThrottling não basta): a fila
  // declara uma atividade iniciada pelo usuário enquanto roda (plugin: acordado)
  invoke('acordado', { ligar: true }).catch(() => {});
  promessa.finally(() => invoke('acordado', { ligar: false }).catch(() => {}));
  return promessa;
}

function rodar() { return manterAcordado(rodarFila()); }
async function rodarFila() {
  if (estado.rodando) return;
  if (estado.ajustes.saida === 'outra' && !estado.ajustes.pastaSaida) { avisar('Escolha a pasta de saída (ou marque "ao lado de cada original").'); return; }
  estado.rodando = true; estado.parar = false;
  $('bt_parar').textContent = 'Parar depois deste arquivo';
  desenhar();
  try {
    // os que voltam para refazer o caso (Separar/Juntar) já têm o texto
    const leva = estado.fila.filter((x) => x.etapa === 'refazer' && x.doc);
    leva.forEach((x) => { x.etapa = 'lido'; x.avisos = []; x.erro = null; });
    let i;
    while (!estado.parar && (i = estado.fila.find((x) => x.etapa === 'fila'))) if (await lerArquivo(i)) leva.push(i);
    const grupos = agrupar(leva), nomes = new Set();
    if (estado.e2e) registrar('casos: ' + grupos.map((g) => g.itens.length).join(' + '));
    for (const g of grupos) await gravarCaso(g, nomes);
  } finally {
    estado.rodando = false; desenhar();
    if (estado.aoParar) { const f = estado.aoParar; estado.aoParar = null; f(); }
  }
}

/** Separar um documento do caso dele, ou juntá-lo a outro caso: os documentos dos casos mexidos voltam para refazer. */
let proximaMao = 1;
function mexerNoCaso(i, alvo) {
  const doCaso = (c) => estado.fila.filter((x) => x.caso && x.caso.chave === c && x.doc);
  const mexidos = new Set(i.caso ? doCaso(i.caso.chave) : [i]);
  if (alvo === 'só') {
    i.casoManual = 'só:' + i.id;
  } else {
    const chave = 'm:' + proximaMao++;
    for (const x of doCaso(alvo).concat([i])) { x.casoManual = chave; mexidos.add(x); }
  }
  mexidos.forEach((x) => { x.etapa = 'refazer'; });
  desenhar();
}

// ── ajustes ───────────────────────────────────────────────────────────────────────────────────────────────────────
async function lerAjustes() {
  const a = await invoke('ajustes_ler');
  Object.assign(estado.ajustes, a || {});
  $('op_txt').checked = !!estado.ajustes.txt;
  $('op_rigoroso').checked = !!estado.ajustes.rigoroso;
  $('op_juntar').checked = estado.ajustes.juntar !== false;
  document.querySelector('input[name=saida][value=' + (estado.ajustes.saida === 'outra' ? 'outra' : 'ao_lado') + ']').checked = true;
  $('pasta_saida').value = estado.ajustes.pastaSaida || '';
}
async function gravarAjustes() {
  const a = Object.assign({}, estado.ajustes);
  await invoke('ajustes_gravar', { ajustes: a });
}
function abrirAjustes() {
  const c = estado.ajustes.contatos == null ? estado.instPadrao : estado.ajustes.contatos;
  $('inst').value = c.split(/\s*;\s*|\n+/).filter(Boolean).join('\n');
  $('op_checar').checked = estado.ajustes.checarAoAbrir !== false;
  $('motor_msg').textContent = '';
  $('dlg_ajustes').showModal();
}
async function salvarAjustes() {
  const linhas = $('inst').value.split('\n').map((x) => x.trim()).filter(Boolean).join('\n');
  const padrao = estado.instPadrao.split(/\s*;\s*|\n+/).filter(Boolean).join('\n');
  estado.ajustes.contatos = linhas === padrao ? null : linhas;
  estado.ajustes.checarAoAbrir = $('op_checar').checked;
  await gravarAjustes();
  await estado.worker.pedir('configurar', { cfg: cfgMotor() });
}

// ── reidentificação ───────────────────────────────────────────────────────────────────────────────────────────────
// Como o 🔓 do Graphé: as funções do motor (no worker) trocam cada token do mapa pelo nome; aqui, a janela, a leitura e a
// gravação dos arquivos (no Rust) e, no .docx, a troca dentro de cada parágrafo, sem mexer na formatação.
const W_NS = 'http://schemas.openxmlformats.org/wordprocessingml/2006/main';
const XML_NS = 'http://www.w3.org/XML/1998/namespace';

function mostrarAba(qual) {
  estado.aba = qual;
  $('aba_anon').hidden = qual !== 'anon';
  $('aba_reid').hidden = qual !== 'reid';
  $('aba_bt_anon').classList.toggle('ativa', qual === 'anon');
  $('aba_bt_reid').classList.toggle('ativa', qual === 'reid');
}

const tituloDe = (c) => nomeDe(c).replace(/\.[^.]+$/, '');
const extDe = (c) => ((nomeDe(c).match(/\.([^.]+)$/) || [])[1] || '').toLowerCase();

async function escolherMapa(caminho) {
  try {
    const json = await invoke('ler_texto', { caminho });
    const dic = await estado.worker.pedir('dicionario', { mapa: json });
    estado.reid.mapa = { caminho, json, dic };
  } catch (e) {
    estado.reid.mapa = null;
    avisar('Não consegui usar esse arquivo como mapa: ' + ((e && e.message) || e));
  }
  desenharReid();
}

/** O aviso do que sobrou, com as palavras do 🔓 do Graphé. */
function avisoSobras(sobras) {
  return 'Sobraram tokens que não voltaram (fora do mapa, malformados ou não trocados): ' + sobras.join(', ') +
    '. Confira esses pontos antes de usar. (Nome grego SEM « » não é trocado — pode ser o nome comum, como o filósofo; se for a pessoa, ponha os « » ou troque à mão.)';
}

function desenharReid() {
  const m = estado.reid.mapa;
  $('mapa_info').innerHTML = m
    ? '<b>' + esc(nomeDe(m.caminho)) + '</b><br>' + (/\d{7}-\d{2}\.\d{4}/.test(m.dic.idCaso) ? 'processo ' + esc(m.dic.idCaso) + ' · ' : '') +
      m.dic.pessoas + ' pessoa(s) · ' + m.dic.lista.length + ' token(s)'
    : 'É o “… — MAPA (nomes reais).json” que o Incognitus grava ao lado de cada PDF anonimizado (o mapa do Graphé também serve).';
  $('dicionario').hidden = !m;
  $('dic_tabela').innerHTML = m ? m.dic.lista.map((x) => '<tr><td>' + esc(x.token) + '</td><td>' + esc(x.valor) + '</td><td>' + esc(x.tipo) + '</td></tr>').join('') : '';
  $('bt_minutas').disabled = !m;
  $('bt_reid_texto').disabled = !m || !estado.worker;
  const t = estado.reid.texto;
  $('reid_texto_caixa').hidden = !t;
  if (t) {
    $('reid_texto_info').innerHTML = t.sobras.length
      ? '<div class="aviso">' + t.n + ' token(s) substituído(s). ' + esc(avisoSobras(t.sobras)) + '</div>'
      : '<div class="ok">' + t.n + ' token(s) substituído(s). Nenhum token sobrou.</div>';
    $('reid_saida').value = t.texto;
  }
  $('reid_lista').innerHTML = estado.reid.itens.map((i) => {
    const chip = i.etapa === 'pronto' ? 'pronto' : i.etapa === 'erro' ? 'erro' : 'trabalhando';
    return '<li class="item' + (i.etapa === 'erro' ? ' erro' : '') + '">' +
      '<div class="cab"><span class="nome">' + esc(i.nome) + '</span><span class="chip ' + chip + '">' +
      (i.etapa === 'pronto' ? 'reidentificada' : i.etapa === 'erro' ? 'erro' : 'reidentificando') + '</span></div>' +
      (i.saida ? '<div class="det"><b>' + i.n + '</b> token(s) substituído(s) · ' + esc(nomeDe(i.saida)) + '</div>' : '') +
      (i.sobras && i.sobras.length ? '<div class="det">⚠ ' + esc(avisoSobras(i.sobras)) + '</div>' : '') +
      (i.erro ? '<div class="msg-erro">' + esc(i.erro) + '</div>' : '') +
      (i.saida ? '<div class="acoes"><button class="ghost" data-revelar="' + esc(i.saida) + '">Mostrar na pasta</button></div>' : '') +
      '</li>';
  }).join('');
  $('reid_vazio').hidden = !!t || estado.reid.itens.length > 0;
  $('bt_reid_limpar').hidden = !t && !estado.reid.itens.length;
}

async function reidTexto() {
  const m = estado.reid.mapa;
  if (!m) { avisar('Escolha primeiro o mapa.'); return; }
  const texto = $('reid_entrada').value;
  if (!texto.trim()) return;
  try { estado.reid.texto = await estado.worker.pedir('reidentificar', { texto, mapa: m.json }); }
  catch (e) { avisar('Não consegui reidentificar: ' + ((e && e.message) || e)); }
  desenharReid();
}

async function reidTxt(caminho, mapa) {
  const texto = await invoke('ler_texto', { caminho });
  const r = await estado.worker.pedir('reidentificar', { texto, mapa, titulo: tituloDe(caminho) });
  const saida = await invoke('gravar_reidentificado', { pedido: { origem: caminho, titulo: r.titulo, texto: r.texto, partes: null } });
  return { saida, n: r.n + r.nTitulo, sobras: r.sobras };
}

/** Troca, nos nós de texto (w:t) de um parágrafo, os trechos [{ i, f, v }] do texto corrido deles. O valor entra no
 *  nó onde o token começa (com a formatação dele); o resto do token sai dos nós seguintes. Do fim para o começo, para
 *  as posições anteriores continuarem valendo. */
function aplicarTrechos(ts, trechos) {
  const ini = [];
  let acc = 0;
  for (const t of ts) { ini.push(acc); acc += t.textContent.length; }
  const no = (pos) => { let j = 0; while (j + 1 < ts.length && ini[j + 1] <= pos) j++; return j; };
  for (const tr of trechos.slice().sort((x, y) => y.i - x.i)) {
    const a = no(tr.i), b = no(tr.f - 1), ta = ts[a].textContent;
    if (a === b) ts[a].textContent = ta.slice(0, tr.i - ini[a]) + tr.v + ta.slice(tr.f - ini[a]);
    else {
      ts[a].textContent = ta.slice(0, tr.i - ini[a]) + tr.v;
      for (let j = a + 1; j < b; j++) ts[j].textContent = '';
      ts[b].textContent = ts[b].textContent.slice(tr.f - ini[b]);
    }
    for (const j of new Set([a, b])) ts[j].setAttributeNS(XML_NS, 'xml:space', 'preserve');
  }
}

async function reidDocx(caminho, mapa) {
  const partes = (await invoke('docx_partes', { caminho })).map((p) => ({ nome: p.nome, doc: new DOMParser().parseFromString(p.xml, 'application/xml') }));
  const pars = [];
  for (const p of partes) {
    if (p.doc.getElementsByTagName('parsererror').length) throw new Error('não consegui ler ' + p.nome + ' do .docx');
    for (const par of Array.from(p.doc.getElementsByTagNameNS(W_NS, 'p'))) {
      // só os w:t deste parágrafo (uma caixa de texto dentro dele tem os próprios parágrafos)
      const ts = Array.from(par.getElementsByTagNameNS(W_NS, 't')).filter((t) => {
        let x = t.parentNode;
        while (x && !(x.localName === 'p' && x.namespaceURI === W_NS)) x = x.parentNode;
        return x === par;
      });
      if (ts.length) pars.push(ts);
    }
  }
  const textos = pars.map((ts) => ts.map((t) => t.textContent).join(''));
  const trechos = await estado.worker.pedir('trechos', { textos, mapa });
  let n = 0;
  pars.forEach((ts, k) => { if (trechos[k].length) { n += trechos[k].length; aplicarTrechos(ts, trechos[k]); } });
  const tit = await estado.worker.pedir('reidentificar', { texto: '', mapa, titulo: tituloDe(caminho) });
  const final = pars.map((ts) => ts.map((t) => t.textContent).join('')).join('\n');
  const sobras = await estado.worker.pedir('sobras', { texto: final, titulo: tit.titulo, mapa });
  const ser = new XMLSerializer();
  const saida = await invoke('gravar_reidentificado', { pedido: { origem: caminho, titulo: tit.titulo, texto: null,
    partes: partes.map((p) => ({ nome: p.nome, xml: ser.serializeToString(p.doc) })) } });
  return { saida, n: n + tit.nTitulo, sobras };
}

async function reidArquivos(caminhos) {
  const m = estado.reid.mapa;
  if (!m) { avisar('Escolha primeiro o mapa.'); return; }
  for (const c of caminhos) {
    const i = { caminho: c, nome: nomeDe(c), etapa: 'trabalhando' };
    estado.reid.itens.unshift(i);
    desenharReid();
    try {
      const ext = extDe(c);
      if (ext === 'docx') Object.assign(i, await reidDocx(c, m.json));
      else if (ext === 'txt' || ext === 'md') Object.assign(i, await reidTxt(c, m.json));
      else throw new Error('a reidentificação lê .docx, .txt e .md');
      i.etapa = 'pronto';
    } catch (e) {
      i.etapa = 'erro'; i.erro = String((e && e.message) || e);
    }
    desenharReid();
  }
}

/** O que foi arrastado para a aba Reidentificar: o mapa (.json) primeiro, depois as minutas. */
async function soltarReid(caminhos) {
  const mapa = caminhos.find((c) => extDe(c) === 'json');
  if (mapa) await escolherMapa(mapa);
  const minutas = caminhos.filter((c) => ['docx', 'txt', 'md'].includes(extDe(c)));
  if (minutas.length) await reidArquivos(minutas);
  else if (!mapa) avisar('Aqui entram o mapa (.json) e as minutas (.docx, .txt ou .md). Os PDFs vão na aba Anonimizar.');
}

async function copiar() {
  const texto = $('reid_saida').value;
  try { await navigator.clipboard.writeText(texto); }
  catch (e) { $('reid_saida').focus(); $('reid_saida').select(); try { document.execCommand('copy'); } catch (e2) { /* fica selecionado */ } }
  $('bt_copiar').textContent = '✓ Copiado';
  setTimeout(() => { $('bt_copiar').textContent = 'Copiar'; }, 2500);
}

async function salvarTxt() {
  const caminho = await T.dialog.save({ defaultPath: 'minuta reidentificada.txt', filters: [{ name: 'Texto', extensions: ['txt'] }] });
  if (!caminho) return;
  try { await invoke('gravar_texto', { caminho, texto: $('reid_saida').value }); }
  catch (e) { avisar('Não consegui gravar: ' + ((e && e.message) || e)); }
}

// ── versão nova do app ────────────────────────────────────────────────────────────────────────────────────────────
// Como no Comentora: a release mais nova do repositório público; se for mais nova, a faixa no alto oferece atualizar.
async function checarApp(manual) {
  if (estado.hospedado) return;   // hospedado, quem atualiza o app é o Graphé Desktop
  try {
    estado.novaVersao = await invoke('app_checar');
    mostrarNovidade();
    if (manual) $('app_msg').textContent = estado.novaVersao ? 'Há uma versão nova: ' + estado.novaVersao.versao + '.' : 'Esta já é a versão mais nova.';
  } catch (e) {
    if (manual) $('app_msg').textContent = 'Não consegui procurar: ' + ((e && e.message) || e);
  }
}

function mostrarNovidade() {
  const n = estado.novaVersao, el = $('novidade');
  el.hidden = !n || !!estado.novidadeAdiada;
  if (!n) return;
  el.innerHTML = '<span>Versão nova do Incognitus: <b>' + esc(n.versao) + '</b>' + (n.tamanho ? ' (' + Math.round(n.tamanho / 1048576) + ' MB)' : '') + '.</span>' +
    '<button id="bt_app_atualizar" class="pequeno">Atualizar agora</button><button id="bt_app_depois" class="ghost pequeno">Depois</button>' +
    (n.notas ? '<details><summary>O que mudou</summary><div class="notas">' + esc(n.notas) + '</div></details>' : '');
  $('bt_app_atualizar').onclick = atualizarApp;
  $('bt_app_depois').onclick = () => { estado.novidadeAdiada = true; mostrarNovidade(); };
}

async function atualizarApp() {
  const n = estado.novaVersao, b = $('bt_app_atualizar');
  if (estado.rodando) { avisar('Espere a fila terminar antes de atualizar.'); return; }
  b.disabled = true; b.textContent = 'Baixando…';
  try {
    const caminho = await invoke('app_baixar', { url: n.url, versao: n.versao });
    b.textContent = 'Abrindo o instalador…';
    await T.dialog.message(/Mac/.test(navigator.platform)
      ? 'O instalador da versão ' + n.versao + ' vai abrir. Arraste o Incognitus para Aplicativos, confirme "Substituir" e abra o app de novo. O Incognitus fecha agora.'
      : 'O instalador da versão ' + n.versao + ' vai abrir e trocar a versão instalada. O Incognitus fecha agora.', { title: 'Incognitus', kind: 'info' });
    await invoke('app_instalar', { caminho });
  } catch (e) {
    b.disabled = false; b.textContent = 'Atualizar agora';
    avisar('Não consegui atualizar: ' + ((e && e.message) || e) + (n.pagina ? '\n\nDá para baixar pela página da versão: ' + n.pagina : ''));
  }
}

// ── início ────────────────────────────────────────────────────────────────────────────────────────────────────────
function ligar() {
  $('aba_bt_anon').onclick = () => mostrarAba('anon');
  $('aba_bt_reid').onclick = () => mostrarAba('reid');
  $('bt_mapa').onclick = async () => {
    const r = await T.dialog.open({ multiple: false, directory: false, filters: [{ name: 'Mapa', extensions: ['json'] }] });
    if (r) await escolherMapa(r);
  };
  $('bt_minutas').onclick = async () => {
    const r = await T.dialog.open({ multiple: true, directory: false, filters: [{ name: 'Minutas', extensions: ['docx', 'txt', 'md'] }] });
    if (r) await reidArquivos(Array.isArray(r) ? r : [r]);
  };
  $('bt_reid_texto').onclick = reidTexto;
  $('bt_copiar').onclick = copiar;
  $('bt_salvar_txt').onclick = salvarTxt;
  $('bt_reid_limpar').onclick = () => { estado.reid.texto = null; estado.reid.itens = []; desenharReid(); };
  $('reid_lista').onclick = (ev) => { const b = ev.target.closest('[data-revelar]'); if (b) invoke('revelar', { caminho: b.dataset.revelar }); };
  $('bt_pdfs').onclick = async () => {
    const r = await T.dialog.open({ multiple: true, directory: false, filters: [{ name: 'PDF, Word ou texto', extensions: ['pdf', 'PDF', 'docx', 'DOCX', 'txt', 'TXT'] }] });
    if (r) await adicionar(Array.isArray(r) ? r : [r]);
  };
  $('bt_pasta').onclick = async () => {
    const r = await T.dialog.open({ directory: true, multiple: false });
    if (r) await adicionar([r]);
  };
  $('bt_pasta_saida').onclick = async () => {
    const r = await T.dialog.open({ directory: true, multiple: false });
    if (!r) return;
    estado.ajustes.pastaSaida = r; estado.ajustes.saida = 'outra';
    $('pasta_saida').value = r;
    document.querySelector('input[name=saida][value=outra]').checked = true;
    await gravarAjustes();
  };
  document.querySelectorAll('input[name=saida]').forEach((el) => { el.onchange = () => { estado.ajustes.saida = el.value; gravarAjustes(); }; });
  $('op_txt').onchange = () => { estado.ajustes.txt = $('op_txt').checked; gravarAjustes(); };
  $('op_juntar').onchange = () => { estado.ajustes.juntar = $('op_juntar').checked; gravarAjustes(); };
  $('op_rigoroso').onchange = async () => {
    estado.ajustes.rigoroso = $('op_rigoroso').checked; gravarAjustes();
    if (!estado.ajustes.rigoroso) return;
    const msg = (t) => { $('msg_rigoroso').textContent = t; };
    try { await baixarModelo(msg); msg('Modelo pronto, neste computador.'); }
    catch (e) { msg('Não consegui baixar o modelo: ' + (e.message || e)); }
  };
  $('bt_rodar').onclick = rodar;
  $('bt_parar').onclick = () => { estado.parar = true; $('bt_parar').textContent = 'Parando depois deste…'; };
  $('bt_limpar').onclick = () => { estado.fila = estado.fila.filter((i) => i.etapa !== 'pronto'); desenhar(); };
  $('fila').onclick = (ev) => {
    const b = ev.target.closest('[data-revelar]');
    if (b) invoke('revelar', { caminho: b.dataset.revelar });
    const m = ev.target.closest('[data-mapa]');
    if (m) { mostrarAba('reid'); escolherMapa(m.dataset.mapa); }
    const sp = ev.target.closest('[data-separar]');
    if (sp) mexerNoCaso(estado.fila.find((x) => x.id === +sp.dataset.separar), 'só');
  };
  $('fila').onchange = (ev) => {
    const j = ev.target.closest('[data-juntar]');
    if (j && j.value) mexerNoCaso(estado.fila.find((x) => x.id === +j.dataset.juntar), j.value);
  };
  $('bt_ajustes').onclick = abrirAjustes;
  $('bt_motor').onclick = abrirAjustes;
  $('bt_inst_padrao').onclick = () => { $('inst').value = estado.instPadrao.split(/\s*;\s*|\n+/).filter(Boolean).join('\n'); };
  $('bt_ajustes_cancelar').onclick = () => $('dlg_ajustes').close();
  $('dlg_ajustes').querySelector('form').onsubmit = (ev) => { ev.preventDefault(); salvarAjustes().then(() => $('dlg_ajustes').close()); };
  $('bt_atualizar').onclick = async () => { $('bt_atualizar').disabled = true; try { await atualizarRegras(true); } finally { $('bt_atualizar').disabled = false; } };
  $('bt_app_checar').onclick = () => checarApp(true);
  $('bt_voltar').onclick = async () => {
    try { await invoke('motor_voltar'); await carregarMotor(); $('motor_msg').textContent = 'Em uso agora: ' + estado.motor.manifesto.versao + '.'; }
    catch (e) { $('motor_msg').textContent = String(e); }
  };
  T.webview.getCurrentWebview().onDragDropEvent((ev) => {
    const t = ev.payload.type;
    document.body.classList.toggle('arrastando', t === 'enter' || t === 'over');
    if (t === 'drop') (estado.aba === 'reid' ? soltarReid : adicionar)(ev.payload.paths || []);
  });
}

async function iniciar() {
  registrar('início');
  if (/Mac/.test(navigator.platform)) document.documentElement.classList.add('mac');
  ligar();
  desenhar();
  desenharReid();
  // Hospedado no Graphé Desktop, some o que é só do app avulso (a atualização do APP; a das regras vale nos dois modos).
  estado.hospedado = !!(await invoke('info')).hospedado;
  document.documentElement.classList.toggle('hospedado', estado.hospedado);
  await lerAjustes();
  registrar('ajustes lidos');
  estado.casos = JSON.parse(await texto('motor/casos.json'));
  registrar('casos lidos: ' + estado.casos.casos.length);
  await carregarMotor();
  registrar('motor ' + estado.motor.manifesto.versao + ' carregado (' + estado.motor.origem + ')');
  desenhar();
  desenharReid();
  invoke('info_app').then((info) => {
    const f = info.ferramentas;
    $('versao_app').textContent = 'Incognitus ' + info.versao;
    $('ferramentas_info').textContent = 'Incognitus ' + info.versao + ' · ' + (f.Ok ? f.Ok.join(' · ') : 'PROBLEMA nas ferramentas: ' + f.Err);
    if (f.Err) avisar('As ferramentas embutidas (pdftotext, pdftoppm, tesseract) não rodaram: ' + f.Err);
  });
  estado.e2e = await invoke('e2e_plano');
  // no teste, um sinal de vida a cada 30 s: mostra se os timers da janela continuam rodando (página suspensa ou não)
  if (estado.e2e) { const t0 = Date.now(); setInterval(() => registrar('vivo ' + Math.round((Date.now() - t0) / 1000) + ' s · ' + document.visibilityState), 30000); }
  if (estado.e2e) return testeDePontaAPonta(estado.e2e);
  if (estado.ajustes.checarAoAbrir !== false) { atualizarRegras(false); checarApp(false); }
}

/** Modo de teste (SKIA_E2E): enfileira, roda, confere a atualização das regras e grava o relatório. */
async function testeDePontaAPonta(p) {
  const rel = { inicio: new Date().toISOString(), motor: estado.motor.manifesto.versao, itens: [] };
  try {
    estado.ajustes.saida = p.saida ? 'outra' : 'ao_lado';
    estado.ajustes.pastaSaida = p.saida || '';
    $('op_txt').checked = !!p.txt;
    estado.ajustes.rigoroso = !!p.rigoroso;
    $('op_rigoroso').checked = estado.ajustes.rigoroso;
    estado.ajustes.juntar = p.juntar !== false;
    $('op_juntar').checked = estado.ajustes.juntar;
    await adicionar(p.arquivos || []);
    await rodar();
    rel.itens = estado.fila.map((i) => ({ nome: i.nome, caminho: i.caminho, etapa: i.etapa, erro: i.erro, saida: i.saida, stats: i.stats,
      avisos: i.avisos, paginas: i.paginas, e2e: i.e2e, motor: i.motor, segundos: i.segundos,
      caso: i.caso ? { rotulo: i.caso.rotulo, chave: i.caso.chave, n: i.caso.itens.length, motivo: i.caso.motivo } : null }));
    // o teste de Separar: tira o documento pedido do caso dele e refaz
    if (p.separar) {
      const alvo = estado.fila.find((i) => i.nome === p.separar);
      if (alvo) { mexerNoCaso(alvo, 'só'); await rodar(); }
      rel.depoisDeSeparar = estado.fila.map((i) => ({ nome: i.nome, etapa: i.etapa, saida: i.saida, caso: i.caso ? { chave: i.caso.chave, n: i.caso.itens.length } : null }));
    }
    if (p.reidentificar) {
      const q = p.reidentificar;
      mostrarAba('reid');
      await escolherMapa(q.mapa);
      rel.reid = { tokens: estado.reid.mapa ? estado.reid.mapa.dic.lista.length : null };
      if (q.texto) { $('reid_entrada').value = q.texto; await reidTexto(); rel.reid.texto = estado.reid.texto; }
      if (q.arquivos) {
        await reidArquivos(q.arquivos);
        rel.reid.arquivos = estado.reid.itens.map((i) => ({ caminho: i.caminho, etapa: i.etapa, erro: i.erro, saida: i.saida, n: i.n, sobras: i.sobras }));
      }
    }
    if (p.atualizacaoApp) {
      try {
        const n = await invoke('app_checar');
        rel.app = { nova: n };
        if (n) rel.app.baixado = await invoke('app_baixar', { url: n.url, versao: n.versao });
      } catch (e) { rel.app = { erro: String((e && e.message) || e) }; }
    }
    if (p.atualizar) rel.atualizacao = await atualizarRegras(true);
    rel.motorFinal = estado.motor.manifesto.versao + ' (' + estado.motor.origem + ', ' + estado.motor.manifesto.data + ')';
  } catch (e) { rel.erro = String(e); }
  rel.fim = new Date().toISOString();
  await invoke('e2e_fim', { relatorio: JSON.stringify(rel, null, 1) });
}

iniciar().catch((e) => { registrar('o Incognitus não iniciou: ' + (e.message || e)); document.body.insertAdjacentHTML('afterbegin', '<div class="aviso">O Incognitus não iniciou: ' + esc(e.message || e) + '</div>'); });
