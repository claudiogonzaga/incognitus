// Σκιά · Skiá — os casos de teste do motor: a suíte de regressão do Graphé (testes/anon/regressao.js, 63 casos com
// dados 100% fictícios), com a mesma conferência, para o build e para o app — que a roda num motor baixado antes de
// trocar o motor em uso. Caso com uma conferência que este arquivo não conhece conta como falha (não passa calado).
var SkiaCasos = (function () {
  var CHAVES = { id: 1, c: 1, t: 1, req: 1, oculto: 1, claro: 1, tokenTipo: 1, fundido: 1, papeisDistintos: 1, genero: 1 };
  function norm(s) { return String(s).replace(/\s+/g, ' '); }
  function valores(mapa) { return Object.keys((mapa && mapa.tokens) || {}).map(function (t) { return mapa.tokens[t]; }); }

  // r = { corpo, mapa } do lote; devolve a lista de erros (vazia = passou). A mesma de regressao.js.
  function conferir(k, r) {
    var corpo = r ? norm(r.corpo) : '', erros = [];
    if (!r) erros.push('exceção no pipeline');
    (k.oculto || []).forEach(function (s) { if (corpo.toLowerCase().indexOf(norm(s).toLowerCase()) >= 0) erros.push('em claro: ' + s); });
    (k.claro || []).forEach(function (s) { if (corpo.indexOf(norm(s)) < 0) erros.push('tokenizado: ' + s); });
    if (r && k.tokenTipo) k.tokenTipo.forEach(function (tt) { if (!new RegExp('«' + tt + '_\\d+»').test(corpo)) erros.push('sem token ' + tt); });
    if (r && k.fundido) {
      var f = valores(r.mapa).filter(function (e) { return / E /i.test(' ' + (e.valor || '') + ' ') && /GERALDO/i.test(e.valor || ''); });
      if (f.length) erros.push('pai e mãe num token só');
    }
    if (r && k.papeisDistintos) {
      var ms = valores(r.mapa).filter(function (e) { return /MARCOS/i.test(e.valor || ''); });
      var tipos = {}; ms.forEach(function (e) { tipos[e.tipo] = 1; });
      if (ms.length < 2 || !tipos.TESTEMUNHA) erros.push('homônimo sem papel próprio (' + ms.map(function (e) { return e.tipo; }).join(',') + ')');
    }
    if (r && k.genero) Object.keys(k.genero).forEach(function (nome) {
      var e = valores(r.mapa).filter(function (x) { return (x.valor || '').toUpperCase().indexOf(nome) >= 0; })[0];
      if (!e || e.genero !== k.genero[nome]) erros.push('gênero de ' + nome + ': ' + (e && e.genero));
    });
    return erros;
  }

  // A volta: os autos anonimizados, reidentificados com o próprio mapa, não podem guardar token nenhum, nem sobra.
  function voltar(M, r) {
    var v;
    try { v = M.reidentificar(r.corpo, r.mapa); } catch (e) { return ['reidentificação: ' + ((e && e.message) || e)]; }
    var erros = [], ficou = Object.keys(r.mapa.tokens).filter(function (t) { return v.texto.indexOf(t) >= 0; });
    if (ficou.length) erros.push('reidentificação: sobrou ' + ficou.join(', '));
    if (v.sobras.length) erros.push('reidentificação, sobras: ' + v.sobras.join(', '));
    return erros;
  }

  // Roda o pacote de casos ({ capa, reqPadrao, casos }) num motor (o SkiaMotor de um contexto/worker).
  function rodar(M, pacote, aoAndar) {
    var ok = 0, falhas = [], casos = (pacote && pacote.casos) || [];
    for (var i = 0; i < casos.length; i++) {
      var k = casos[i], erros, r = null;
      var estranhas = Object.keys(k).filter(function (x) { return !CHAVES[x]; });
      if (estranhas.length) erros = ['conferência desconhecida: ' + estranhas.join(', ')];
      else {
        try { r = M.anonimizar(pacote.capa + k.t + '\f', { req: k.req || pacote.reqPadrao, incluirMapa: true }); } catch (e) { r = null; }
        erros = conferir(k, r && { corpo: r.corpo, mapa: r.mapa });
        if (r && !erros.length) erros = voltar(M, r);
      }
      if (erros.length) falhas.push({ id: k.id, c: k.c, erros: erros }); else ok++;
      if (aoAndar) aoAndar(i + 1, casos.length);
    }
    return { total: casos.length, ok: ok, falhas: falhas, versao: M.versao, api: M.api };
  }
  return { conferir: conferir, rodar: rodar };
})();
if (typeof module !== 'undefined' && module.exports) module.exports = SkiaCasos;
