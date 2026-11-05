// Σκιά · Skiá — a janela: a fila de PDFs e o fluxo de cada um, igual ao lote automático do Graphé.
//   extrair (pdftotext -layout) → páginas fracas ao OCR (2 de cada vez) → anonimizar (motor, no worker) → gravar
// Um PDF por vez. A pasta temporária de cada PDF é apagada ao fim dele, com ou sem erro.
'use strict';
const T = window.__TAURI__;
const invoke = T.core.invoke;
const $ = (id) => document.getElementById(id);

const registrar = (m) => invoke('registrar', { msg: String(m) }).catch(() => {});
window.addEventListener('error', (e) => registrar('erro: ' + e.message + ' @' + e.filename + ':' + e.lineno));
window.addEventListener('unhandledrejection', (e) => registrar('promessa: ' + ((e.reason && e.reason.message) || e.reason)));

const estado = {
  fila: [], rodando: false, parar: false, proximoId: 1,
  ajustes: { saida: 'ao_lado', pastaSaida: '', txt: false, contatos: null, checarAoAbrir: true },
  instPadrao: '', motor: null, worker: null, casos: null, e2e: null,
};
const ETAPAS = { fila: 'na fila', extraindo: 'extraindo', ocr: 'OCR', anonimizando: 'anonimizando', gravando: 'gravando o PDF', pronto: 'pronto', erro: 'erro' };

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
  try { await pedir('versao'); } finally { URL.revokeObjectURL(url); }
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
      if (v.api !== 1) throw new Error('o motor novo fala outra API (' + v.api + ')');
      res = await teste.pedir('testar', { pacote: estado.casos });
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
      (i.stats.naoConfirmados ? ' · ' + i.stats.naoConfirmados + ' nome(s) provável(is) em claro (ver cabeçalho)' : ''));
    if (i.segundos) det.push(i.segundos + ' s');
    if (i.motor) det.push('motor ' + i.motor);
    return '<li class="item' + (trab ? ' ativo' : '') + (i.etapa === 'erro' ? ' erro' : '') + '">' +
      '<div class="cab"><span class="nome">' + esc(i.nome) + '</span><span class="chip ' + (i.etapa === 'pronto' ? 'pronto' : i.etapa === 'erro' ? 'erro' : trab ? 'trabalhando' : '') + '">' + esc(chip) + '</span></div>' +
      '<div class="barra"><i style="width:' + progresso(i) + '%"></i></div>' +
      (det.length ? '<div class="det">' + det.join(' · ') + '</div>' : '') +
      (i.avisos.length ? '<div class="det">⚠ ' + esc(i.avisos.join(' · ')) + '</div>' : '') +
      (i.erro ? '<div class="msg-erro">' + esc(i.erro) + '</div>' : '') +
      (i.saida ? '<div class="acoes"><button class="ghost" data-revelar="' + esc(i.saida.pdf) + '">Mostrar na pasta</button></div>' : '') +
      '</li>';
  }).join('');
  $('vazio').hidden = estado.fila.length > 0;
  const prontos = estado.fila.filter((i) => i.etapa === 'pronto').length, erros = estado.fila.filter((i) => i.etapa === 'erro').length;
  const pendentes = estado.fila.filter((i) => i.etapa === 'fila').length;
  $('resumo').textContent = estado.fila.length ? prontos + ' de ' + estado.fila.length + ' pronto(s)' + (erros ? ' · ' + erros + ' erro(s)' : '') : '';
  $('bt_rodar').disabled = estado.rodando || !pendentes || !estado.worker;
  $('bt_rodar').textContent = estado.rodando ? 'Anonimizando…' : 'Anonimizar' + (pendentes ? ' (' + pendentes + ')' : '');
  $('bt_parar').hidden = !estado.rodando;
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
    const comMapa = $('op_mapa').checked, comTexto = $('op_txt').checked;
    const r = await w.pedir('anonimizar', { texto: oc.texto, meta: oc.meta, comMapa, comTexto });
    i.motor = r.versao; i.stats = r.estatisticas;
    i.etapa = 'gravando'; desenhar();
    const destino = estado.ajustes.saida === 'outra' ? estado.ajustes.pastaSaida : null;
    i.saida = await invoke('gravar_saida', { pedido: {
      origem: i.caminho, destino, versao_motor: r.versao,
      cabecalho: r.cabecalho.normalize('NFC'), paginas: r.paginas.map((p) => p.normalize('NFC')),
      texto: comTexto ? r.texto : null, mapa: comMapa ? r.mapaJson : null,
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
  if (estado.ajustes.saida === 'outra' && !estado.ajustes.pastaSaida) { alert('Escolha a pasta de saída (ou marque "ao lado de cada original").'); return; }
  estado.rodando = true; estado.parar = false; desenhar();
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
  $('op_mapa').checked = false;   // o mapa nunca vem marcado: cada vez é uma escolha
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

// ── início ────────────────────────────────────────────────────────────────────────────────────────────────────────
function ligar() {
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
  $('op_mapa').onchange = () => { $('aviso_mapa').hidden = !$('op_mapa').checked; };
  $('bt_rodar').onclick = rodar;
  $('bt_parar').onclick = () => { estado.parar = true; $('bt_parar').textContent = 'Parando depois deste…'; };
  $('bt_limpar').onclick = () => { estado.fila = estado.fila.filter((i) => i.etapa !== 'pronto'); desenhar(); };
  $('fila').onclick = (ev) => { const b = ev.target.closest('[data-revelar]'); if (b) invoke('revelar', { caminho: b.dataset.revelar }); };
  $('bt_ajustes').onclick = abrirAjustes;
  $('bt_motor').onclick = abrirAjustes;
  $('bt_inst_padrao').onclick = () => { $('inst').value = estado.instPadrao.split(/\s*;\s*|\n+/).filter(Boolean).join('\n'); };
  $('bt_ajustes_cancelar').onclick = () => $('dlg_ajustes').close();
  $('dlg_ajustes').querySelector('form').onsubmit = (ev) => { ev.preventDefault(); salvarAjustes().then(() => $('dlg_ajustes').close()); };
  $('bt_atualizar').onclick = async () => { $('bt_atualizar').disabled = true; try { await atualizarRegras(true); } finally { $('bt_atualizar').disabled = false; } };
  $('bt_voltar').onclick = async () => {
    try { await invoke('motor_voltar'); await carregarMotor(); $('motor_msg').textContent = 'Em uso agora: ' + estado.motor.manifesto.versao + '.'; }
    catch (e) { $('motor_msg').textContent = String(e); }
  };
  T.webview.getCurrentWebview().onDragDropEvent((ev) => {
    const t = ev.payload.type;
    document.body.classList.toggle('arrastando', t === 'enter' || t === 'over');
    if (t === 'drop') adicionar(ev.payload.paths || []);
  });
}

async function iniciar() {
  registrar('início');
  if (/Mac/.test(navigator.platform)) document.documentElement.classList.add('mac');
  ligar();
  desenhar();
  await lerAjustes();
  registrar('ajustes lidos');
  estado.casos = JSON.parse(await texto('motor/casos.json'));
  registrar('casos lidos: ' + estado.casos.casos.length);
  await carregarMotor();
  registrar('motor ' + estado.motor.manifesto.versao + ' carregado (' + estado.motor.origem + ')');
  desenhar();
  invoke('info_app').then((info) => {
    const f = info.ferramentas;
    $('ferramentas_info').textContent = 'Skiá ' + info.versao + ' · ' + (f.Ok ? f.Ok.join(' · ') : 'PROBLEMA nas ferramentas: ' + f.Err);
    if (f.Err) alert('As ferramentas embutidas (pdftotext, pdftoppm, tesseract) não rodaram: ' + f.Err);
  });
  estado.e2e = await invoke('e2e_plano');
  if (estado.e2e) return testeDePontaAPonta(estado.e2e);
  if (estado.ajustes.checarAoAbrir !== false) atualizarRegras(false);
}

/** Modo de teste (SKIA_E2E): enfileira, roda, confere a atualização das regras e grava o relatório. */
async function testeDePontaAPonta(p) {
  const rel = { inicio: new Date().toISOString(), motor: estado.motor.manifesto.versao, itens: [] };
  try {
    estado.ajustes.saida = p.saida ? 'outra' : 'ao_lado';
    estado.ajustes.pastaSaida = p.saida || '';
    $('op_txt').checked = !!p.txt; $('op_mapa').checked = !!p.mapa;
    await adicionar(p.arquivos);
    await rodar();
    rel.itens = estado.fila.map((i) => ({ nome: i.nome, caminho: i.caminho, etapa: i.etapa, erro: i.erro, saida: i.saida, stats: i.stats,
      avisos: i.avisos, paginas: i.paginas, e2e: i.e2e, motor: i.motor, segundos: i.segundos }));
    if (p.atualizar) rel.atualizacao = await atualizarRegras(true);
    rel.motorFinal = estado.motor.manifesto.versao + ' (' + estado.motor.origem + ', ' + estado.motor.manifesto.data + ')';
  } catch (e) { rel.erro = String(e); }
  rel.fim = new Date().toISOString();
  await invoke('e2e_fim', { relatorio: JSON.stringify(rel, null, 1) });
}

iniciar().catch((e) => { registrar('o Skiá não iniciou: ' + (e.message || e)); document.body.insertAdjacentHTML('afterbegin', '<div class="aviso">O Skiá não iniciou: ' + esc(e.message || e) + '</div>'); });
