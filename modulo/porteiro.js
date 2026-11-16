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
  return normal(v).split(' ').filter((p) => p && !LIGA.test(p)).length >= 2;
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

window.Porteiro = { conferir, barrarSeVazar, textosDoPedido };
})();
