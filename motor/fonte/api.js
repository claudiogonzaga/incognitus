// ── API do Skiá (versão 1) ──────────────────────────────────────────────────────────────────────────────────────────
// O fluxo é o do lote automático do Graphé: SKIA_PIPELINE.rodar — o testes/anon/pipeline.js, acima, sem alteração
// (preparar → classificador → concluir) — e, como no anonConcluir, o cabeçalho de autos anonimizados.
// A leitura do PDF segue o _pdfComOcr_ do Graphé com o serviço cloudrun-pdftotext/main.py: as páginas fracas (pelo
// critério do Graphé) vão ao OCR, e o texto do OCR só entra se tiver mais texto útil (pelo critério do serviço).
var SkiaMotor = (function () {
  var G = (typeof globalThis !== 'undefined') ? globalThis : self;
  var INST_ORIGINAL = ANON_INST_PADRAO;
  var RE_CNJ = /\b\d{7}-\d{2}\.\d{4}\.\d\.\d{2}\.\d{4}\b/;

  // Os Ajustes do app: os contatos institucionais que ficam em claro (o padrão é o ANON_INST_PADRAO do Graphé).
  function configurar(cfg) {
    cfg = cfg || {};
    ANON_INST_PADRAO = (typeof cfg.contatosInstitucionais === 'string') ? cfg.contatosInstitucionais : INST_ORIGINAL;
    if (typeof _ANON_INST_ !== 'undefined') _ANON_INST_ = null;
    return { contatosInstitucionais: ANON_INST_PADRAO };
  }

  // ─ O serviço é Python: subprocess.run(..., text=True) traduz as quebras de linha ("\r\n" e "\r" viram "\n"); o re
  // do Python entende \d e \s em Unicode; str.rstrip() tira todo espaço Unicode. Aqui, o mesmo.
  var PY_ESPACO = '\\t-\\r\\x1c-\\x20\\x85\\xa0\\u1680\\u2000-\\u200a\\u2028\\u2029\\u202f\\u205f\\u3000';
  var RE_PY_FIM = new RegExp('[' + PY_ESPACO + ']+$', 'u');
  function rePython(fonte, flags) {
    var out = '', dentro = false;
    for (var i = 0; i < fonte.length; i++) {
      var c = fonte[i];
      if (c === '\\') {
        var n = fonte[++i];
        if (n === 'd') out += '\\p{Nd}';
        else if (n === 's') out += dentro ? PY_ESPACO : '[' + PY_ESPACO + ']';
        else if (n === 'S' && !dentro) out += '[^' + PY_ESPACO + ']';
        else if (n === 'S') throw new Error('\\S dentro de classe: ' + fonte);
        else out += '\\' + n;
        continue;
      }
      if (c === '[' && !dentro) dentro = true;
      else if (c === ']' && dentro) dentro = false;
      out += c;
    }
    return new RegExp(out, flags);
  }
  // A lista BOILERPLATE do main.py é a ANON_BOILERPLATE do graphe.js (o Graphé tem um teste que compara as duas).
  var RE_BOILERPLATE_PY = ANON_BOILERPLATE.map(function (x) { return rePython(x, 'giu'); });
  function quebrasPython(s) { return String(s == null ? '' : s).replace(/\r\n?/g, '\n'); }
  function textoUtilServico(p) {
    var t = String(p || '');
    for (var i = 0; i < RE_BOILERPLATE_PY.length; i++) t = t.replace(RE_BOILERPLATE_PY[i], ' ');
    return t;
  }
  function charsUteisServico(p) { return (textoUtilServico(p).match(/[A-Za-zÀ-ÿ]{3,}/g) || []).join('').length; }
  // main.py, _ocr_de_uma_pagina: res.stdout.replace("\f", "\n").rstrip() + "\n"
  function textoDoOcr(saida) { return quebrasPython(saida).replace(/\f/g, '\n').replace(RE_PY_FIM, '') + '\n'; }

  // O texto do pdftotext -layout (com um \f no fim de cada página) e as páginas (1-based) que vão ao OCR.
  function lerPdftotext(saida) {
    var texto = quebrasPython(saida);
    return { texto: texto, paginas: _anonPaginas_(texto).length, fracas: _pdfPaginasFracas_(texto) };
  }

  // _pdfComOcr_ sem o serviço: `lidos` = { página: saída crua do tesseract | null (o OCR da página falhou) }.
  function aplicarOcr(texto, fracas, lidos) {
    fracas = fracas || []; lidos = lidos || {};
    var meta = { truncado: false, chars: String(texto || '').length };
    meta.ocr = { pedido: true, aplicado: false, paginas: [], fracas: fracas.length };
    meta.ocrPendente = [];
    if (!fracas.length) { meta.ocr.motivo = 'nenhuma página fraca'; return { texto: texto, meta: meta, falhas: [] }; }
    var partes = String(texto).split('\f'), lidas = [], falhas = [];
    fracas.forEach(function (n) {
      if (!(n >= 1 && n <= partes.length)) return;
      if (typeof lidos[n] !== 'string') { falhas.push(n); return; }
      var lido = textoDoOcr(lidos[n]);
      if (charsUteisServico(lido) > charsUteisServico(partes[n - 1])) { partes[n - 1] = lido; lidas.push(n); }
    });
    meta.ocr.lotes = 1;
    if (!lidas.length) return { texto: texto, meta: meta, falhas: falhas };
    var novo = partes.join('\f');
    meta.ocr.aplicado = true;
    meta.ocr.paginas = lidas.sort(function (a, b) { return a - b; });
    _pdfOcrFecharMeta_(meta, novo, []);
    return { texto: novo, meta: meta, falhas: falhas };
  }

  function contarPessoas(mapa) {
    var n = 0, t;
    for (t in mapa.tokens) if (mapa.tokens[t] && mapa.tokens[t].variantes && mapa.tokens[t].tipo !== 'ALIAS') n++;
    return n;
  }

  // O lote automático + o cabeçalho. op: { meta (de aplicarOcr), req, idCaso, aba, incluirMapa }.
  function anonimizar(texto, op) {
    op = op || {};
    texto = String(texto == null ? '' : texto);
    var meta = op.meta || null;
    if (!texto || texto.trim().length < 20) {
      throw new Error('O PDF não devolveu texto (só imagens' + ((meta && meta.ocr && meta.ocr.motivo) ?
        ', e o OCR não leu: ' + meta.ocr.motivo : ', e nem o OCR leu') + ').');
    }
    var req = op.req || '';
    var r = SKIA_PIPELINE.rodar(G, texto, { req: req, trs: [], equipe: [] });
    var partes = _anonNomesDaLinha_(req).concat(_anonPartesDaCapa_(r.paginas));
    var pg = _anonPaginasFracasInfo_(r.paginas, meta);
    var naoConf = (r.cand || []).filter(function (c) { return c && !c.provavel && !c.ctxPessoa; }).length;
    var corpo = r.corpo.replace(/\s+$/, '');
    var idCaso = (op.idCaso != null) ? op.idCaso : ((r.paginas.slice(0, ANON_PAGINAS_CAPA).join('\n').match(RE_CNJ) || [''])[0]);
    var cab = _anonCabecalho_({ idCaso: idCaso, aba: (op.aba != null) ? op.aba : 'Skiá', paginas: r.paginas.length, mapa: r.mapa,
      semTexto: pg.semTexto, historico: [], corpo: corpo, partes: partes, nTranscricoes: 0, truncado: false, chars: texto.length,
      naoConfirmados: naoConf, poucoTexto: pg.poucoTexto, ocr: pg.ocr, ocrPendente: pg.ocrPendente });
    // Cada página como no corpo do .txt ("===== PÁGINA n =====" e o texto): o PDF do Skiá tem uma página para cada uma.
    var paginas = r.paginasAnon.map(function (p, i) { return _anonCorpoPaginas_([p], i + 1); });
    var res = {
      api: 1, versao: ANON_VERSAO, cabecalho: cab, paginas: paginas, texto: cab + '\n\n' + corpo,
      estatisticas: { paginas: r.paginas.length, tokens: Object.keys(r.mapa.tokens).length, pessoas: contarPessoas(r.mapa),
        candidatos: (r.cand || []).length, confirmados: (r.conf || []).length, naoConfirmados: naoConf,
        sobras: (r.sobras && r.sobras.total) || 0, ocr: (pg.ocr && pg.ocr.paginas) ? pg.ocr.paginas.length : 0,
        semTexto: pg.semTexto.length, poucoTexto: pg.poucoTexto.length }
    };
    if (op.incluirMapa) { res.mapa = r.mapa; res.corpo = r.corpo; }
    return res;
  }

  return {
    api: 1, versao: ANON_VERSAO, configurar: configurar, lerPdftotext: lerPdftotext, aplicarOcr: aplicarOcr,
    anonimizar: anonimizar, charsUteisServico: charsUteisServico, textoDoOcr: textoDoOcr
  };
})();
