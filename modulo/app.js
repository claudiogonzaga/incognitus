// Incognitus — a janela: a fila de PDFs e o fluxo de cada um, igual ao lote automático do Graphé, e a reidentificação.
//   extrair (pdftotext -layout) → páginas fracas ao OCR (2 de cada vez) → anonimizar (motor, no worker) → gravar
// Um PDF por vez. A pasta temporária de cada PDF é apagada ao fim dele, com ou sem erro.
'use strict';
const T = window.__TAURI__;
const invoke = T.core.invoke;
const $ = (id) => document.getElementById(id);
const avisar = (texto) => T.dialog.message(texto, { title: 'Incognitus', kind: 'warning' }).catch(() => {});

const registrar = (m) => invoke('registrar', { msg: String(m) }).catch(() => {});
window.addEventListener('error', (e) => registrar('erro: ' + e.message + ' @' + e.filename + ':' + e.lineno));
window.addEventListener('unhandledrejection', (e) => registrar('promessa: ' + ((e.reason && e.reason.message) || e.reason)));

const estado = {
  fila: [], rodando: false, parar: false, proximoId: 1,
  ajustes: { saida: 'ao_lado', pastaSaida: '', txt: false, contatos: null, checarAoAbrir: true },
  instPadrao: '', motor: null, worker: null, casos: null, e2e: null,
  aba: 'anon', reid: { mapa: null, texto: null, itens: [] },
};
const ETAPAS = { fila: 'na fila', extraindo: 'extraindo', ocr: 'OCR', anonimizando: 'anonimizando', gravando: 'gravando o PDF', pronto: 'pronto', erro: 'erro' };

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
    case 'fila': return 0;
    case 'extraindo': return 8;
    case 'ocr': return 10 + 60 * (i.ocr ? i.ocr.feitas / Math.max(1, i.ocr.total) : 0);
    case 'anonimizando': return i.ocr ? 75 : 50;
    case 'gravando': return 92;
    default: return 100;
  }
}

function desenhar() {
  const f = $('fila');
  f.innerHTML = estado.fila.map((i) => {
    const trab = ['extraindo', 'ocr', 'anonimizando', 'gravando'].includes(i.etapa);
    let chip = ETAPAS[i.etapa];
    if (i.etapa === 'ocr' && i.ocr) chip = 'OCR ' + i.ocr.feitas + '/' + i.ocr.total;
    const det = [];
    if (i.paginas) det.push('<b>' + i.paginas + '</b> página' + (i.paginas > 1 ? 's' : ''));
    if (i.fracas != null) det.push(i.fracas ? i.fracas + ' fraca(s) → OCR' : 'nenhuma página para OCR');
    if (i.stats) det.push('<b>' + i.stats.pessoas + '</b> pessoa(s) · ' + i.stats.tokens + ' token(s)' +
      (i.stats.naoConfirmados ? ' · ' + i.stats.naoConfirmados + ' candidato(s) a nome em claro — quase sempre termos que não são nomes; confira' : ''));
    if (i.segundos) det.push(i.segundos + ' s');
    if (i.motor) det.push('motor ' + i.motor);
    return '<li class="item' + (trab ? ' ativo' : '') + (i.etapa === 'erro' ? ' erro' : '') + '">' +
      '<div class="cab"><span class="nome">' + esc(i.nome) + '</span><span class="chip ' + (i.etapa === 'pronto' ? 'pronto' : i.etapa === 'erro' ? 'erro' : trab ? 'trabalhando' : '') + '">' + esc(chip) + '</span></div>' +
      '<div class="barra"><i style="width:' + progresso(i) + '%"></i></div>' +
      (det.length ? '<div class="det">' + det.join(' · ') + '</div>' : '') +
      (i.avisos.length ? '<div class="det">⚠ ' + esc(i.avisos.join(' · ')) + '</div>' : '') +
      (i.erro ? '<div class="msg-erro">' + esc(i.erro) + '</div>' : '') +
      (i.saida ? '<div class="acoes"><button class="ghost" data-revelar="' + esc(i.saida.pdf) + '">Mostrar na pasta</button>' +
        (i.saida.mapa ? '<button class="ghost" data-mapa="' + esc(i.saida.mapa) + '">Reidentificar com este mapa</button>' : '') + '</div>' : '') +
      '</li>';
  }).join('');
  $('vazio').hidden = estado.fila.length > 0;
  const prontos = estado.fila.filter((i) => i.etapa === 'pronto').length, erros = estado.fila.filter((i) => i.etapa === 'erro').length;
  const pendentes = estado.fila.filter((i) => i.etapa === 'fila').length;
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

async function processar(i) {
  const t0 = Date.now();
  if (estado.e2e) registrar('processando ' + i.nome);
  const w = estado.worker;
  let prep = null;
  i.avisos = []; i.erro = null; i.saida = null; i.stats = null; i.ocr = null; i.fracas = null;
  try {
    i.etapa = 'extraindo'; desenhar();
    prep = await invoke('preparar_arquivo', { caminho: i.caminho });
    const lido = await w.pedir('ler', { saida: await invoke('extrair_texto', { id: prep.id }) });
    i.paginas = lido.paginas; i.fracas = lido.fracas.length; desenhar();
    const lidos = {};
    if (lido.fracas.length) {
      i.etapa = 'ocr'; i.ocr = { feitas: 0, total: lido.fracas.length }; desenhar();
      await emParalelo(lido.fracas, 2, async (n) => {
        try { lidos[n] = await invoke('ocr_pagina', { id: prep.id, pagina: n }); }
        catch (e) { lidos[n] = null; i.avisos.push('OCR da página ' + n + ' falhou (' + e + ')'); }
        i.ocr.feitas++; desenhar();
      });
    }
    const oc = await w.pedir('ocr', { texto: lido.texto, fracas: lido.fracas, lidos });
    if (estado.e2e) {
      await invoke('e2e_gravar', { nome: i.nome + '.pdftotext.txt', conteudo: lido.texto });
      await invoke('e2e_gravar', { nome: i.nome + '.extracao.txt', conteudo: oc.texto });
      i.e2e = { fracas: lido.fracas, ocr: oc.meta.ocr };
    }
    i.etapa = 'anonimizando'; desenhar();
    const comTexto = $('op_txt').checked;
    const r = await w.pedir('anonimizar', { texto: oc.texto, meta: oc.meta, comTexto });
    i.motor = r.versao; i.stats = r.estatisticas;
    i.etapa = 'gravando'; desenhar();
    const destino = estado.ajustes.saida === 'outra' ? estado.ajustes.pastaSaida : null;
    i.saida = await invoke('gravar_saida', { pedido: {
      origem: i.caminho, destino, versao_motor: r.versao,
      cabecalho: r.cabecalho.normalize('NFC'), paginas: r.paginas.map((p) => p.normalize('NFC')),
      texto: comTexto ? r.texto : null, mapa: r.mapaJson,
    } });
    if (i.saida.sem_glifo) i.avisos.push(i.saida.sem_glifo + ' caractere(s) sem desenho na fonte viraram "?" no PDF' + (comTexto ? ' (o .txt tem o original)' : ''));
    i.etapa = 'pronto';
  } catch (e) {
    i.etapa = 'erro'; i.erro = String((e && e.message) || e);
    if (estado.e2e) registrar('erro em ' + i.nome + ': ' + i.erro);
  } finally {
    if (prep) await invoke('limpar_arquivo', { id: prep.id }).catch(() => {});
    i.segundos = Math.round((Date.now() - t0) / 1000);
    desenhar();
  }
}

async function rodar() {
  if (estado.rodando) return;
  if (estado.ajustes.saida === 'outra' && !estado.ajustes.pastaSaida) { avisar('Escolha a pasta de saída (ou marque "ao lado de cada original").'); return; }
  estado.rodando = true; estado.parar = false;
  $('bt_parar').textContent = 'Parar depois deste arquivo';
  desenhar();
  try {
    let i;
    while (!estado.parar && (i = estado.fila.find((x) => x.etapa === 'fila'))) await processar(i);
  } finally {
    estado.rodando = false; desenhar();
    if (estado.aoParar) { const f = estado.aoParar; estado.aoParar = null; f(); }
  }
}

// ── ajustes ───────────────────────────────────────────────────────────────────────────────────────────────────────
async function lerAjustes() {
  const a = await invoke('ajustes_ler');
  Object.assign(estado.ajustes, a || {});
  $('op_txt').checked = !!estado.ajustes.txt;
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
    const r = await T.dialog.open({ multiple: true, directory: false, filters: [{ name: 'PDF', extensions: ['pdf', 'PDF'] }] });
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
  $('bt_rodar').onclick = rodar;
  $('bt_parar').onclick = () => { estado.parar = true; $('bt_parar').textContent = 'Parando depois deste…'; };
  $('bt_limpar').onclick = () => { estado.fila = estado.fila.filter((i) => i.etapa !== 'pronto'); desenhar(); };
  $('fila').onclick = (ev) => {
    const b = ev.target.closest('[data-revelar]');
    if (b) invoke('revelar', { caminho: b.dataset.revelar });
    const m = ev.target.closest('[data-mapa]');
    if (m) { mostrarAba('reid'); escolherMapa(m.dataset.mapa); }
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
    await adicionar(p.arquivos || []);
    await rodar();
    rel.itens = estado.fila.map((i) => ({ nome: i.nome, caminho: i.caminho, etapa: i.etapa, erro: i.erro, saida: i.saida, stats: i.stats,
      avisos: i.avisos, paginas: i.paginas, e2e: i.e2e, motor: i.motor, segundos: i.segundos }));
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
