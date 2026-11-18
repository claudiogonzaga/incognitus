// Vem do Graphé Desktop (nucleo/porteiro.mjs), sem mudança de regra; aqui como script da janela (window.Porteiro).
(function () {
'use strict';
// O PORTEIRO: antes de cada envio à Anthropic, confere que nenhum dado que a anonimização trocou por token ficou em
// claro no pedido. Olha o mapa do caso (que nunca sai do computador) e devolve só os tokens achados, nunca o dado.
//   · nomes de pessoa e endereços com duas palavras ou mais (o valor e as variantes), sem acento e sem caixa;
//   · CPF, CNPJ, RG, CNH, telefone, CEP, placa, conta, coordenadas e afins, com ou sem pontuação no meio;
//   · e-mails e perfis de rede, sem caixa.
// Ficam de fora: prenome sozinho, alcunha e iniciais (casariam com palavras comuns) e datas de nascimento (a mesma data
// pode ser a de outro ato do processo).

const SEM_CONFERIR = new Set(['PRENOME', 'ALCUNHA', 'INICIAIS', 'ALIAS', 'NASC']);
const POR_DIGITOS = new Set(['CPF', 'CPFMASC', 'CNPJ', 'RG', 'CNH', 'DOC', 'CNS', 'CERT', 'RJI', 'CAR', 'TEL', 'CEP', 'BANCO', 'GPS', 'PLACA']);
const LITERAIS = new Set(['EMAIL', 'REDE']);
const LIGA = /^(D[AEO]S?|E|DI|DEL|VAN|VON)$/;

const normal = (s) => String(s == null ? '' : s).normalize('NFD').replace(/[̀-ͯ]/g, '').toUpperCase().replace(/\s+/g, ' ').trim();
const escapar = (s) => s.replace(/[.*+?^${}()|[\]\\]/g, '\\$&');

/** Todos os textos de um pedido (o system, os blocos, os resultados das consultas), um por linha. */
function textosDoPedido(pedido) {
  const out = [];
  const pilha = [typeof pedido === 'string' ? JSON.parse(pedido) : pedido];
  while (pilha.length) {
    const x = pilha.pop();
    if (typeof x === 'string') out.push(x);
    else if (Array.isArray(x)) for (const e of x) pilha.push(e);
    else if (x && typeof x === 'object') for (const k in x) if (k !== 'signature' && k !== 'id' && k !== 'tool_use_id') pilha.push(x[k]);
  }
  return out.join('\n');
}

/** Um dado com 2 palavras ou mais (fora as ligações "de", "da"…) — o que pode ir ao conferir por nome. */
function nomeComposto(v) {
  // só conta a palavra que tem letra: "al. [2025" (resto de citação que o motor tomou por nome) não é nome composto
  return normal(v).split(' ').filter((p) => p && /[A-Z]/.test(p) && !LIGA.test(p)).length >= 2;
}

/** O teste de um valor: por dígitos e letras (com ou sem pontuação no meio), literal ou pelo nome sem acento. */
function testeDoValor(tipo, valor) {
  const v = String(valor || '').trim();
  if (!v) return null;
  if (LITERAIS.has(tipo)) {
    if (v.length < 6) return null;
    const re = new RegExp(escapar(v), 'i');
    return (t) => re.test(t.cru);
  }
  if (POR_DIGITOS.has(tipo)) {
    const alnum = v.replace(/[^0-9A-Za-z]/g, '');
    if ((alnum.match(/\d/g) || []).length < 5) return null;
    const re = new RegExp('(?<![0-9A-Za-z])' + alnum.split('').map(escapar).join('[\\s.\\-/]?') + '(?![0-9A-Za-z])', 'i');
    return (t) => re.test(t.cru);
  }
  if (!nomeComposto(v)) return null;
  const alvo = normal(v);
  return (t) => {
    let i = t.normal.indexOf(alvo);
    while (i >= 0) {
      const antes = i > 0 ? t.normal[i - 1] : ' ', depois = t.normal[i + alvo.length] || ' ';
      if (!/[A-Z0-9]/.test(antes) && !/[A-Z0-9]/.test(depois)) return true;
      i = t.normal.indexOf(alvo, i + 1);
    }
    return false;
  };
}

/** Os tokens do mapa cujo dado aparece em claro no texto: [{ token, tipo }], sem o dado. */
function conferir(texto, mapa) {
  const t = { cru: String(texto || ''), normal: normal(texto) };
  const achados = [];
  const tokens = (mapa && mapa.tokens) || {};
  for (const token of Object.keys(tokens)) {
    const e = tokens[token] || {}, tipo = String(e.tipo || '').toUpperCase();
    if (SEM_CONFERIR.has(tipo)) continue;
    const valores = [e.valor].concat(Array.isArray(e.variantes) ? e.variantes : []);
    if (valores.some((v) => { const teste = testeDoValor(tipo, v); return teste && teste(t); })) achados.push({ token, tipo });
  }
  return achados;
}

/** Onde o dado aparece no texto: { i, f } (do caractere i ao f) ou null. Para o trecho que a janela mostra. */
function localizar(texto, tipo, valor) {
  const cru = String(texto || ''), v = String(valor || '').trim();
  if (!v) return null;
  let re;
  if (LITERAIS.has(tipo)) re = new RegExp(escapar(v), 'i');
  else if (POR_DIGITOS.has(tipo)) {
    const alnum = v.replace(/[^0-9A-Za-z]/g, '');
    re = new RegExp('(?<![0-9A-Za-z])' + alnum.split('').map(escapar).join('[\\s.\\-/]?') + '(?![0-9A-Za-z])', 'i');
  } else {
    // o nome sem acento e sem caixa, num texto dobrado do mesmo tamanho (NFC → NFD sem os acentos: 1 letra por letra)
    const dobrado = cru.normalize('NFC').split('').map((c) => c.normalize('NFD').replace(/[̀-ͯ]/g, '').toUpperCase().charAt(0) || c).join('');
    const partes = normal(v).split(' ').filter(Boolean).map(escapar);
    const m = new RegExp('(?<![A-Z0-9])' + partes.join('\\s+') + '(?![A-Z0-9])').exec(dobrado);
    return m ? { i: m.index, f: m.index + m[0].length } : null;
  }
  const m = re.exec(cru);
  return m ? { i: m.index, f: m.index + m[0].length } : null;
}

/** O exame do pedido para a confirmação (16/11/2026: o porteiro não barra mais, mostra e pergunta): cada token do mapa
 *  cujo dado aparece em claro, com o dado e um trecho em volta. [{ token, tipo, valor, onde, antes, dado, depois }].
 *  `partes`: [{ rotulo, texto }] (o prompt, os autos…), para dizer onde está; o dado só aparece na janela, nunca sai. */
function examinar(partes, mapa) {
  const tokens = (mapa && mapa.tokens) || {};
  const ps = partes.map((p) => ({ rotulo: p.rotulo, cru: String(p.texto || ''), t: { cru: String(p.texto || ''), normal: normal(p.texto) } }));
  const out = [];
  for (const token of Object.keys(tokens)) {
    const e = tokens[token] || {}, tipo = String(e.tipo || '').toUpperCase();
    if (SEM_CONFERIR.has(tipo)) continue;
    const valores = [e.valor].concat(Array.isArray(e.variantes) ? e.variantes : []);
    let achado = null;
    for (const p of ps) {
      for (const v of valores) {
        const teste = testeDoValor(tipo, v);
        if (!teste || !teste(p.t)) continue;
        const pos = localizar(p.cru, tipo, v);
        achado = { token, tipo, valor: String(e.valor || v), casou: String(v), onde: p.rotulo };
        if (pos) Object.assign(achado, { antes: p.cru.slice(Math.max(0, pos.i - 70), pos.i), dado: p.cru.slice(pos.i, pos.f), depois: p.cru.slice(pos.f, pos.f + 70) });
        break;
      }
      if (achado) break;
    }
    if (achado) out.push(achado);
  }
  return out;
}

// ── O REPASSE (16/11/2026, pedido do usuário: "Identificou dados em claro? reaplica a função para esses dados!") ──
// O que o porteiro acha em claro, ele mesmo troca pelo token do mapa: na anonimização (antes de gravar o PDF) e no
// pedido à IA (antes de sair). A regra é a mesma do conferir (o que ele acusa é o que ele troca): nome e endereço
// compostos sem acento, sem caixa e por cima de quebra de linha; documento com ou sem pontuação; e-mail e perfil.

const MARCA = /[\u0300-\u036f]/;

/** O texto dobrado (sem acento, em maiúsculas) e, de cada letra dele, a posição no texto original. */
function dobrar(cru) {
  let dob = '';
  const idx = [];
  for (let i = 0; i < cru.length; i++) {
    const c = cru[i];
    if (c < '\u0080') { dob += c >= 'a' && c <= 'z' ? c.toUpperCase() : c; idx.push(i); continue; }
    const d = c.normalize('NFD').replace(/[\u0300-\u036f]/g, '').toUpperCase();
    for (let k = 0; k < d.length; k++) { dob += d[k]; idx.push(i); }
  }
  return { dob, idx };
}

/** Os testes de um mapa, prontos (guardados por mapa): [{ token, tipo, partes } | { token, tipo, re }]. */
const PRONTOS = new WeakMap();
function preparar(mapa) {
  if (mapa && PRONTOS.has(mapa)) return PRONTOS.get(mapa);
  const lista = [], tokens = (mapa && mapa.tokens) || {};
  for (const token of Object.keys(tokens)) {
    const e = tokens[token] || {}, tipo = String(e.tipo || '').toUpperCase();
    if (SEM_CONFERIR.has(tipo)) continue;
    const vistos = new Set();
    for (const valor of [e.valor].concat(Array.isArray(e.variantes) ? e.variantes : [])) {
      const v = String(valor || '').trim();
      if (!v) continue;
      const ensinado = tipo === 'APRENDIDO', digitos = (v.match(/\d/g) || []).length;
      if (LITERAIS.has(tipo) || (ensinado && v.includes('@'))) {
        if (v.length < 6 || vistos.has(v.toLowerCase())) continue;
        vistos.add(v.toLowerCase());
        lista.push({ token, tipo, valor: v, re: new RegExp(escapar(v), 'gi') });
      } else if (POR_DIGITOS.has(tipo) || (ensinado && digitos >= 5 && digitos >= v.replace(/[^0-9A-Za-z]/g, '').length / 2)) {
        const alnum = v.replace(/[^0-9A-Za-z]/g, '');
        if ((alnum.match(/\d/g) || []).length < 5 || vistos.has(alnum.toUpperCase())) continue;
        vistos.add(alnum.toUpperCase());
        lista.push({ token, tipo, valor: v, re: new RegExp('(?<![0-9A-Za-z])' + alnum.split('').map(escapar).join('[\\s.\\-/]?') + '(?![0-9A-Za-z])', 'gi') });
      } else if (nomeComposto(v) || (ensinado && normal(v).replace(/[^A-Z0-9]/g, '').length >= 3)) {
        const alvo = normal(v);
        if (vistos.has(alvo)) continue;
        vistos.add(alvo);
        lista.push({ token, tipo, valor: v, partes: alvo.split(' ').filter(Boolean) });
      }
    }
  }
  if (mapa && typeof mapa === 'object') PRONTOS.set(mapa, lista);
  return lista;
}

const LETRA = /[A-Z0-9]/, BRANCO = /\s/;

/** As ocorrências de um nome (as palavras, com qualquer espaço ou quebra de linha entre elas) no texto dobrado. */
function casarNome(dob, partes, achou) {
  const p0 = partes[0];
  let i = dob.indexOf(p0);
  while (i >= 0) {
    let j = i + p0.length, ok = !(i > 0 && LETRA.test(dob[i - 1]));
    for (let k = 1; ok && k < partes.length; k++) {
      let b = j;
      while (b < dob.length && BRANCO.test(dob[b])) b++;
      if (b === j || !dob.startsWith(partes[k], b)) ok = false; else j = b + partes[k].length;
    }
    if (ok && !(j < dob.length && LETRA.test(dob[j]))) { achou(i, j); i = dob.indexOf(p0, j); } else i = dob.indexOf(p0, i + 1);
  }
}

/** Troca pelo token cada dado do mapa que aparece em claro no texto. { texto, total, trocas: [{ token, tipo, n, valores }] }.
 *  `valores`: as formas do mapa (o dado ou uma variante dele) que casaram, para o aprendizado anotar o que foi achado.
 *  O mais comprido ganha quando dois se cruzam; o que já é token («…») não é tocado. */
function tarjar(texto, mapa) {
  const cru = String(texto == null ? '' : texto), testes = preparar(mapa), achados = [];
  if (!cru || !testes.length) return { texto: cru, total: 0, trocas: [] };
  let d = null;
  for (const t of testes) {
    if (t.re) {
      t.re.lastIndex = 0;
      let m;
      while ((m = t.re.exec(cru))) { if (!m[0]) { t.re.lastIndex++; continue; } achados.push({ i: m.index, f: m.index + m[0].length, t }); }
    } else {
      if (!d) d = dobrar(cru);
      casarNome(d.dob, t.partes, (a, b) => {
        let f = d.idx[b - 1] + 1;
        while (f < cru.length && MARCA.test(cru[f])) f++;   // o acento solto da última letra vai junto
        achados.push({ i: d.idx[a], f, t });
      });
    }
  }
  if (!achados.length) return { texto: cru, total: 0, trocas: [] };
  // os tokens que já estão no texto ficam como estão
  const fechados = [];
  const reTok = /«[^«»\n]{1,60}»/g;
  let mt;
  while ((mt = reTok.exec(cru))) fechados.push([mt.index, mt.index + mt[0].length]);
  const cruza = (a, lista) => lista.some((x) => a.i < x[1] && x[0] < a.f);
  achados.sort((a, b) => (b.f - b.i) - (a.f - a.i) || a.i - b.i);
  const ficam = [], tomados = [];
  for (const a of achados) {
    if (cruza(a, fechados) || cruza(a, tomados)) continue;
    ficam.push(a); tomados.push([a.i, a.f]);
  }
  if (!ficam.length) return { texto: cru, total: 0, trocas: [] };
  ficam.sort((a, b) => a.i - b.i);
  let out = '', pos = 0;
  const conta = new Map();
  for (const a of ficam) {
    out += cru.slice(pos, a.i) + a.t.token;
    pos = a.f;
    const c = conta.get(a.t.token) || { token: a.t.token, tipo: a.t.tipo, n: 0, valores: [] };
    c.n++;
    if (a.t.valor && !c.valores.includes(a.t.valor)) c.valores.push(a.t.valor);
    conta.set(a.t.token, c);
  }
  return { texto: out + cru.slice(pos), total: ficam.length, trocas: [...conta.values()] };
}

/** O repasse num pedido à IA, no lugar: o system, o que o usuário manda (os autos, os resultados das consultas) e o
 *  texto das respostas anteriores. Os blocos de raciocínio (assinados) não podem mudar: ficam para a pergunta. */
function tarjarPedido(corpo, mapa) {
  const soma = { total: 0, tokens: new Set(), formas: {} };
  const passa = (t) => {
    if (typeof t !== 'string' || !t) return t;
    const x = tarjar(t, mapa);
    if (x.total) {
      soma.total += x.total;
      x.trocas.forEach((k) => {
        soma.tokens.add(k.token);
        const f = soma.formas[k.token] || (soma.formas[k.token] = []);
        k.valores.forEach((v) => { if (!f.includes(v)) f.push(v); });
      });
    }
    return x.texto;
  };
  const blocos = (lista) => {
    for (const b of lista) {
      if (!b || typeof b !== 'object') continue;
      if (b.type === 'text' && typeof b.text === 'string') b.text = passa(b.text);
      else if (b.type === 'tool_result') { if (typeof b.content === 'string') b.content = passa(b.content); else if (Array.isArray(b.content)) blocos(b.content); }
    }
  };
  if (typeof corpo.system === 'string') corpo.system = passa(corpo.system);
  else if (Array.isArray(corpo.system)) blocos(corpo.system);
  for (const m of corpo.messages || []) {
    if (!m) continue;
    if (typeof m.content === 'string') m.content = passa(m.content);
    else if (Array.isArray(m.content)) blocos(m.content);
  }
  return { total: soma.total, tokens: [...soma.tokens], formas: soma.formas };
}

/** Lança se o pedido leva algum dado do mapa em claro. A mensagem diz os tokens, nunca o dado. */
function barrarSeVazar(pedido, mapa) {
  const achados = conferir(textosDoPedido(pedido), mapa);
  if (!achados.length) return;
  const lista = achados.slice(0, 8).map((a) => a.token).join(', ') + (achados.length > 8 ? ' e mais ' + (achados.length - 8) : '');
  const e = new Error('O porteiro barrou o envio: ' + achados.length + (achados.length === 1 ? ' dado do mapa aparece' : ' dados do mapa aparecem') +
    ' em claro no pedido (' + lista + '). Nada foi enviado à IA. Confira a anonimização deste caso.');
  e.porteiro = achados;
  throw e;
}

window.Porteiro = { conferir, barrarSeVazar, textosDoPedido, examinar, localizar, tarjar, tarjarPedido };
})();
