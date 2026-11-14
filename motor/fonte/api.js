// ── API do Incognitus (versão 2: com a reidentificação) ──────────────────────────────────────────────────────────────────────────────────────────
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
    if (falhas.length) meta.ocr.falhas = falhas.slice();   // 1.11.0: o cabeçalho avisa as páginas em que o OCR falhou
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

  // ─ Os carimbos do PJe na SAÍDA (pedido do Promotor, 09/11/2026): quem gerou o PDF e quem assina saem do texto (a
  // data, a hora, o número do documento e o "Num. … - Pág." ficam). A função mora no graphe.js (_anonSemCarimbosSaida_,
  // desde e83bd54: um motor só para os dois); lá ela não está ligada na saída do Graphé, aqui sim. O lote continua lendo o
  // carimbo para saber quem é autoridade ou advogado (regra 4: em claro no corpo); só a SAÍDA perde o nome.
  // O aviso do cabeçalho do Graphé aponta a revisão humana da planilha ("🔒 Anonimizar autos"), que o Incognitus não tem.
  var RE_AVISO_REVISAO = /\s*\(um humano confere com "[^"]*"\)/;
  var AVISO_INCOGNITUS = ' (o Incognitus não tem essa revisão: quase sempre são termos que não são nomes — exames, remédios, ' +
    'lugares, rótulos de formulário —, mas confira os nomes de pessoas no texto antes de usar)';

  // ─ A anonimização RIGOROSA (opcional, mais lenta; pedido do Promotor, 14/11/2026). O app roda um modelo de NER (BERT)
  // sobre a saída e manda, em op.extras, o que ainda parecia nome ou endereço: [{ nome, tipo: 'PESSOA'|'ENDERECO',
  // pagina, prob, ctx, colado }]. Aqui se decide o que entra — nunca precedente citado, autoridade em claro, palavra
  // comum ou palavra fora do léxico de nomes (salvo o pedaço "colado" num token do motor, que é resto de nome que o OCR
  // estragou) — e o texto é refeito com esses nomes no mapa. Sem extras, nada muda (a paridade com o Graphé continua).
  var RE_RUA = /(?:^|[^A-Za-zÀ-ÿ])(?:rua|r\.|avenida|av\.?|travessa|tv\.?|alameda|rodovia|rod\.|estrada|quadra|qd\.?|lote|lt\.?|bairro|setor|residencial|condom[ií]nio|casa|apto?\.?|apartamento|jardim|jd\.?|vila|loteamento)(?![A-Za-zÀ-ÿ])|n[º°o]\.?\s*\d/i;
  var RE_PONTAS = /^[\s"'“”‘’(\[{,.;:–—\-]+|[\s"'“”‘’)\]},.;:–—\-]+$/g;
  function reEscapar(s) { return s.replace(/[.*+?^${}()|[\]\\]/g, '\\$&'); }
  function trocarLiteral(s, valor, token, semCaixa) {
    var re = new RegExp('(^|[^A-Za-zÀ-ÿ0-9«])(' + reEscapar(valor).replace(/\s+/g, '\\s+') + ')(?![A-Za-zÀ-ÿ0-9»])', semCaixa ? 'gi' : 'g');
    return s.replace(re, function (m0, pre) { return pre + token; });
  }
  function aplicarExtras(r, extras, todo) {
    var mapa = r.mapa, aceitos = [], recusados = [], literais = [], vistos = {};
    var stop = _anonStopSet_(), pren = _anonPrenomesSet_(), sob = _anonSobrenomesSet_(), comum = _anonCaixaComumSet_();
    var minus = _anonMinusculasDoTexto_(todo), branca = _anonListaBrancaSet_(), fora = _anonTextoForaDePrecedentes_(todo);
    var U = function (w) { return _anonSemAcento_(w).toUpperCase(); };
    (extras || []).forEach(function (x) {
      var nome = String(x.nome || '').replace(/\s+/g, ' ').replace(RE_PONTAS, '');
      var tipo = x.tipo === 'ENDERECO' ? 'ENDERECO' : 'PESSOA', k = tipo + ':' + _anonChave_(nome);
      if (!nome || vistos[k]) return;
      vistos[k] = 1;
      var recusar = function (motivo) { recusados.push({ nome: nome, tipo: tipo, motivo: motivo, pagina: x.pagina, ctx: x.ctx || '' }); };
      if (todo.indexOf(nome) < 0) return recusar('não aparece assim no texto original');
      if (fora.indexOf(nome) < 0) return recusar('só aparece dentro de precedente citado');
      if (tipo === 'ENDERECO') {
        if (!RE_RUA.test(nome)) return recusar('sem cara de endereço (falta rua, quadra, bairro ou número)');
        var tE = _anonToken_(mapa, 'END', nome);
        literais.push([nome, tE, false]);
        return aceitos.push({ nome: nome, tipo: tipo, token: tE, pagina: x.pagina });
      }
      var ws = _anonPalavrasSignificativas_(nome), kSig = _anonChaveSig_(nome);
      if (!ws.some(function (w) { return w.length >= 3; })) return recusar('curto demais');
      if (ws.every(function (w) { return stop[U(w)] || comum[U(w)]; })) return recusar('palavra comum');
      if (_anonEhPedacoDeEmClaro_(mapa, kSig)) return recusar('autoridade, servidor ou advogado em claro');
      if (branca[_anonChave_(nome)]) return recusar('lista de autoridades ou de lugares');
      if (!x.colado && !ws.some(function (w) { return pren[U(w)] || sob[U(w)]; })) return recusar('nenhuma palavra do léxico de nomes');
      if (ws.length >= 2) {
        var reg = _anonRegistrarNome_(mapa, nome, 'PES', { isolada: false });
        if (!reg || !reg.token) return recusar('o motor recusou (pessoa jurídica, órgão público ou autoridade)');
        return aceitos.push({ nome: nome, tipo: tipo, token: reg.token, pagina: x.pagina });
      }
      // uma palavra só (prenome solto): token de pessoa (nome grego), trocado onde a palavra aparecer sozinha
      if (!x.colado && (minus[_anonSemAcento_(ws[0]).toLowerCase()] || 0) > 0) return recusar('a palavra também aparece em minúscula no texto');
      var tP = _anonToken_(mapa, 'PES', nome, 'solto:' + _anonChave_(nome), null, _anonGeneroPorPrenome_(nome));
      literais.push([nome, tP, true]);
      aceitos.push({ nome: nome, tipo: tipo, token: tP, pagina: x.pagina });
    });
    if (aceitos.length) {
      try { _anonFundirParecidos_(mapa); } catch (e) { (mapa.falhas = mapa.falhas || []).push('rigoroso: fundir'); }
      try { _anonVariantesParecidas_(todo, mapa); } catch (e) { (mapa.falhas = mapa.falhas || []).push('rigoroso: variantes'); }
      literais.sort(function (a, b) { return b[0].length - a[0].length; });
      r.paginasAnon = r.paginas.map(function (p) {
        var s = _anonAplicarIdentificadores_(_anonAplicarNomes_(p, mapa), mapa);
        literais.forEach(function (l) { s = trocarLiteral(s, l[0], l[1], l[2]); });
        return s;
      });
      r.corpo = _anonCorpoPaginas_(r.paginasAnon, 1);
      try { r.sobras = _anonVarreduraSobras_(r.corpo, mapa); } catch (e) { (mapa.falhas = mapa.falhas || []).push('rigoroso: sobras'); }
    }
    return { aceitos: aceitos, recusados: recusados };
  }

  // ─ A lista "CONFIRA" (vai ao lado do mapa, na pasta que não se manda à IA): onde olhar antes de usar o PDF.
  function paginaDe(paginas, nome) {
    for (var i = 0; i < paginas.length; i++) {
      var j = paginas[i].indexOf(nome);
      if (j >= 0) return { n: i + 1, ctx: paginas[i].slice(Math.max(0, j - 60), j + nome.length + 60).replace(/\s+/g, ' ').trim() };
    }
    return null;
  }
  function montarConfira(r, pg, rig, op, rotulo) {
    rotulo = rotulo || function (n) { return 'p. ' + n; };
    var lista = function (ns) { return ns.map(function (n) { return rotulo(n).replace(/^p\. /, ''); }).join('; '); };
    var L = [], m = r.mapa, falhas = (m.falhas || []).concat(m.etapasComFalha || []);
    L.push('INCOGNITUS — CONFIRA ANTES DE USAR');
    L.push('Este arquivo tem NOMES REAIS (como o mapa): fica no computador e nunca vai para a IA.');
    L.push('Motor ' + ANON_VERSAO + (op.rigoroso ? ' · anonimização rigorosa (modelo de linguagem)' : '') + '.');
    L.push('');
    L.push('1. Avisos');
    // 1.11.0: as etapas que falharam e os tetos atingidos vêm do mapa, com as linhas ⚠ do cabeçalho do Graphé
    if (typeof _anonAvisosDeFalhas_ === 'function') {
      // (o texto do cabeçalho, uma linha por aviso; as sobras têm a seção 3)
      var av = String(_anonAvisosDeFalhas_(m) || '').split('\n').filter(function (l) { return l.trim() && !/^⚠ SOBRAS/.test(l); });
      av.forEach(function (l) { L.push('- ' + l); });
      var soNossas = (m.falhas || []).filter(function (n) { return typeof ANON_ETAPAS_ROTULO !== 'undefined' && !ANON_ETAPAS_ROTULO[n]; });
      if (!av.length && falhas.length) L.push('- ⚠ Etapas do motor que FALHARAM (o texto pode ter saído menos anonimizado): ' + falhas.join(', ') + '.');
      else if (soNossas.length) L.push('- ⚠ Também falharam: ' + soNossas.join(', ') + '.');
    } else if (falhas.length) L.push('- ⚠ Etapas do motor que FALHARAM (o texto pode ter saído menos anonimizado): ' + falhas.join(', ') + '.');
    if (pg.semTexto.length) L.push('- Páginas sem texto (confira no PDF original): ' + lista(pg.semTexto) + '.');
    if (pg.poucoTexto.length) L.push('- Páginas com pouco texto (podem ter imagem sem OCR): ' + lista(pg.poucoTexto) + '.');
    if (pg.ocrFalhou && pg.ocrFalhou.length) L.push('- ⚠ Páginas em que o OCR FALHOU (o texto delas pode faltar): ' + lista(pg.ocrFalhou) + '.');
    if (pg.ocr && pg.ocr.paginas && pg.ocr.paginas.length) L.push('- Páginas lidas por OCR (nomes e números podem ter erro de leitura): ' + lista(pg.ocr.paginas) + '.');
    if (L[L.length - 1] === '1. Avisos') L.push('- nenhum.');
    L.push('');
    var cand = (r.cand || []).filter(function (c) { return c && !c.provavel && !c.ctxPessoa; });
    L.push('2. Possíveis nomes deixados em claro pelo motor (' + cand.length + ')');
    cand.slice(0, 200).forEach(function (c) {
      var p = paginaDe(r.paginas, c.nome);
      L.push('- ' + c.nome + (p ? ' · ' + rotulo(p.n) + ' · …' + p.ctx + '…' : ''));
    });
    if (cand.length > 200) L.push('- … e mais ' + (cand.length - 200) + '.');
    if (!cand.length) L.push('- nenhum.');
    L.push('');
    var sob = (r.sobras && r.sobras.posicoes) || [];
    L.push('3. Sobras: o que a varredura final achou com cara de dado pessoal (' + sob.length + ')');
    sob.slice(0, 200).forEach(function (s) {
      L.push('- ' + s.tipo + (s.pagina ? ' · ' + rotulo(s.pagina) : '') + ' · …' + String(r.corpo || '').slice(Math.max(0, s.ini - 50), s.fim + 50).replace(/\s+/g, ' ').trim() + '…');
    });
    if (!sob.length) L.push('- nenhuma.');
    if (op.rigoroso) {
      L.push('');
      L.push('4. Anonimização rigorosa (o modelo de linguagem leu o texto depois do motor)');
      L.push('- Tarjados a mais: ' + rig.aceitos.length + '.');
      rig.aceitos.forEach(function (a) { L.push('  · ' + a.nome + ' → ' + a.token + (a.pagina ? ' (' + rotulo(a.pagina) + ')' : '')); });
      L.push('- Apontados pelo modelo e NÃO tarjados (confira): ' + rig.recusados.length + '.');
      rig.recusados.slice(0, 300).forEach(function (x) {
        L.push('  · ' + x.nome + ' — ' + x.motivo + (x.pagina ? ' · ' + rotulo(x.pagina) : '') + (x.ctx ? ' · …' + x.ctx.replace(/\s+/g, ' ') + '…' : ''));
      });
    }
    return L.join('\n') + '\n';
  }

  // O lote automático + o cabeçalho, sem montar a saída. op: { meta, req, idCaso, aba, extras, rigoroso, cabExtra }.
  function nucleo(texto, op) {
    texto = String(texto == null ? '' : texto);
    var meta = op.meta || null;
    if (!texto || texto.trim().length < 20) {
      throw new Error('O PDF não devolveu texto (só imagens' + ((meta && meta.ocr && meta.ocr.motivo) ?
        ', e o OCR não leu: ' + meta.ocr.motivo : ', e nem o OCR leu') + ').');
    }
    var req = op.req || '';
    var r = SKIA_PIPELINE.rodar(G, texto, { req: req, trs: [], equipe: [] });
    var rig = { aceitos: [], recusados: [] };
    if (op.extras && op.extras.length) rig = aplicarExtras(r, op.extras, r.paginas.join('\n'));
    var partes = _anonNomesDaLinha_(req).concat(_anonPartesDaCapa_(r.paginas));
    var pg = _anonPaginasFracasInfo_(r.paginas, meta);
    var naoConf = (r.cand || []).filter(function (c) { return c && !c.provavel && !c.ctxPessoa; }).length;
    var corpo = r.corpo.replace(/\s+$/, '');
    var idCaso = (op.idCaso != null) ? op.idCaso : ((r.paginas.slice(0, ANON_PAGINAS_CAPA).join('\n').match(RE_CNJ) || [''])[0]);
    var cab = _anonCabecalho_({ idCaso: idCaso, aba: (op.aba != null) ? op.aba : 'Incognitus', paginas: r.paginas.length, mapa: r.mapa,
      semTexto: pg.semTexto, historico: [], corpo: corpo, partes: partes, nTranscricoes: 0, truncado: false, chars: texto.length,
      naoConfirmados: naoConf, poucoTexto: pg.poucoTexto, ocr: pg.ocr, ocrPendente: pg.ocrPendente, ocrFalhou: pg.ocrFalhou }).replace(RE_AVISO_REVISAO, AVISO_INCOGNITUS);
    if (op.cabExtra) {
      var k = cab.lastIndexOf(ANON_FIM_CABECALHO);
      cab = k >= 0 ? cab.slice(0, k) + op.cabExtra + '\n' + cab.slice(k) : cab + '\n' + op.cabExtra;
    }
    var estat = { paginas: r.paginas.length, tokens: Object.keys(r.mapa.tokens).length, pessoas: contarPessoas(r.mapa),
      candidatos: (r.cand || []).length, confirmados: (r.conf || []).length, naoConfirmados: naoConf,
      sobras: (r.sobras && r.sobras.total) || 0, ocr: (pg.ocr && pg.ocr.paginas) ? pg.ocr.paginas.length : 0,
      semTexto: pg.semTexto.length, poucoTexto: pg.poucoTexto.length,
      falhas: (r.mapa.falhas || []).length, rigorosoAceitos: rig.aceitos.length, rigorosoRecusados: rig.recusados.length };
    return { r: r, rig: rig, pg: pg, corpo: corpo, cab: cab, idCaso: idCaso, estat: estat };
  }

  // Cada página como no corpo do .txt ("===== PÁGINA n =====" e o texto): o PDF do Incognitus tem uma página para cada
  // uma. Os carimbos do PJe saem sem os nomes (_anonSemCarimbosSaida_); o corpo cru (res.corpo, com o mapa) fica como o do Graphé.
  function anonimizar(texto, op) {
    op = op || {};
    var n = nucleo(texto, op), r = n.r;
    var paginas = r.paginasAnon.map(function (p, i) { return _anonCorpoPaginas_([_anonSemCarimbosSaida_(p)], i + 1); });
    var res = {
      api: 2, versao: ANON_VERSAO, cabecalho: n.cab, paginas: paginas, texto: n.cab + '\n\n' + _anonSemCarimbosSaida_(n.corpo),
      estatisticas: n.estat, confira: montarConfira(r, n.pg, n.rig, op)
    };
    if (op.incluirMapa) { res.mapa = r.mapa; res.corpo = r.corpo; }
    return res;
  }

  // ─ VÁRIOS DOCUMENTOS DO MESMO CASO (pedido do Promotor, 14/11/2026): os autos, o relatório, o termo… anonimizados JUNTOS,
  // com um mapa só — a mesma pessoa leva o mesmo token em todos, e a minuta do caso volta com um mapa. O texto de todos
  // vai ao motor de uma vez (como os autos inteiros no Graphé) e a saída é repartida: cada documento com as suas páginas
  // (numeradas a partir de 1), o cabeçalho do caso (com a lista dos documentos) e o nome do arquivo sem nomes de pessoa.
  // docs: [{ nome, texto, meta }] (nome = o do arquivo; texto = o do pdftotext/OCR, uma página por \f).
  var RE_EXT = /\.(?:pdf|docx?|odt|txt|md|rtf)$/i;
  var RE_SEP_ARQ = /\s+[-–—]+\s+|\s*[_;,()\[\]{}]\s*|\s{2,}/;
  // Do nome do arquivo, os pedaços que são nome de pessoa ("relatorio - não juntar - JOÃO DA SILVA SAURO.pdf"): o
  // primeiro nome no léxico de prenomes, mais um nome ou sobrenome, nenhuma palavra de parada, só letras. Vão ao motor
  // como a linha do pedido no Graphé (partes): a pessoa é tarjada no texto e no nome do arquivo.
  function partesDoNomeDeArquivo(nome) {
    var pren = _anonPrenomesSet_(), sob = _anonSobrenomesSet_(), stop = _anonStopSet_(), out = [];
    var U = function (w) { return _anonSemAcento_(w).toUpperCase(); };
    String(nome || '').replace(RE_EXT, '').split(RE_SEP_ARQ).forEach(function (pedaco) {
      var p = pedaco.replace(/\s+/g, ' ').trim();
      if (!p || /[^A-Za-zÀ-ÿ' .]/.test(p)) return;
      var ws = _anonPalavrasSignificativas_(p);
      if (ws.length < 2 || ws.length > 7) return;
      if (!pren[U(ws[0])] || ws.some(function (w) { return stop[U(w)] || w.length < 2; })) return;
      // mais um nome do léxico, ou o nome todo em CAIXA ALTA com três palavras ou mais ("JOAO TESTEFULANO PIRAMBOIA")
      if (!ws.slice(1).some(function (w) { return pren[U(w)] || sob[U(w)]; }) && !(ws.length >= 3 && p === p.toUpperCase())) return;
      out.push(p);
    });
    return out;
  }
  // O nome do arquivo de saída: o mesmo, com os nomes e os dados do mapa trocados pelos tokens.
  function nomeAnonimizado(nome, mapa) {
    var m = String(nome || '').match(RE_EXT), ext = m ? m[0] : '', base = String(nome || '').slice(0, String(nome || '').length - ext.length);
    var s = base;
    try {
      // só os tokens de NOME (uma palavra do léxico de nomes, ao menos): um papel que o motor tenha tomado por nome
      // ("COMPANHEIRA DO FLAGRANTEADO") não troca a palavra no nome do arquivo ("termo da companheira")
      var pren = _anonPrenomesSet_(), sob = _anonSobrenomesSet_(), tokens = {};
      Object.keys(mapa.tokens || {}).forEach(function (t) {
        var e = mapa.tokens[t];
        if (e && e.variantes && !_anonPalavrasSignificativas_(e.valor || '').some(function (w) { var u = _anonSemAcento_(w).toUpperCase(); return pren[u] || sob[u]; })) return;
        tokens[t] = e;
      });
      var m = Object.assign({}, mapa, { tokens: tokens });
      s = _anonAplicarIdentificadores_(_anonAplicarNomes_(s, m), m);
      var autIa = {};
      (mapa.autoridadesIa || []).concat(mapa.emClaro || []).forEach(function (k) { autIa[k] = 1; });
      s = _anonPrenomesEmNomeDeArquivo_(s + '.pdf', m, autIa).replace(/\.pdf$/, '');
    } catch (e) { (mapa.falhas = mapa.falhas || []).push('nome do arquivo'); }
    return s.replace(/[\/\\:*?"<>|\u0000-\u001f]/g, '-').replace(/\s+/g, ' ').trim() || 'documento';
  }
  // O que identifica o caso, para o app juntar documentos: o número do processo (no nome do arquivo ou na capa), os
  // documentos (CPF, RG, CNH, CNS, telefone, placa, e-mail) e os nomes de pessoa (a chave sem acento e sem conectivos).
  var TIPOS_LIGAM = { CPF: 1, RG: 1, CNH: 1, DOC: 1, CNS: 1, CERT: 1, RJI: 1, TEL: 1, PLACA: 1, EMAIL: 1 };
  var TIPOS_NAO_PESSOA = { ALIAS: 1, PRENOME: 1, ALCUNHA: 1, INICIAIS: 1, EMP: 1 };
  function assinatura(mapa, nomes, idCaso) {
    var cnj = {}, ids = {}, pessoas = {};
    (nomes || []).concat([idCaso || '']).forEach(function (x) { (String(x).match(new RegExp(RE_CNJ.source, 'g')) || []).forEach(function (c) { cnj[c] = 1; }); });
    Object.keys(mapa.tokens || {}).forEach(function (t) {
      var e = mapa.tokens[t];
      if (!e || !e.valor) return;
      if (TIPOS_LIGAM[e.tipo]) {
        var v = (e.tipo === 'EMAIL' || e.tipo === 'PLACA') ? String(e.valor).toLowerCase().replace(/[^a-z0-9@.]/g, '') : _anonDigitos_(e.valor);
        if (v.length >= 6) ids[e.tipo + ':' + v] = 1;
      } else if (e.variantes && !TIPOS_NAO_PESSOA[e.tipo] && !_anonEhSoIniciais_(e.valor)) {
        var k = _anonChaveSig_(e.valor);
        if (k.split(' ').length >= 2) pessoas[k] = 1;
      }
    });
    return { cnj: Object.keys(cnj), ids: Object.keys(ids), pessoas: Object.keys(pessoas) };
  }
  function juntarMeta(docs, cortes) {
    var o = { pedido: false, aplicado: false, paginas: [], fracas: 0, falhas: [] }, pend = [], pouco = [], chars = 0, algum = false;
    docs.forEach(function (d, i) {
      var m = d.meta || null, de = cortes[i];
      if (!m) return;
      algum = true; chars += m.chars || 0;
      var oc = m.ocr || {};
      o.pedido = o.pedido || !!oc.pedido; o.aplicado = o.aplicado || !!oc.aplicado; o.fracas += oc.fracas || 0;
      (oc.paginas || []).forEach(function (n) { o.paginas.push(n + de); });
      (oc.falhas || []).forEach(function (n) { o.falhas.push(n + de); });
      (m.ocrPendente || []).forEach(function (n) { pend.push(n + de); });
      (m.poucoTexto || []).forEach(function (n) { pouco.push(n + de); });
      if (oc.motivo && !o.motivo) o.motivo = oc.motivo;
    });
    if (!algum) return null;
    if (o.aplicado) o.motivo = undefined;
    if (!o.falhas.length) delete o.falhas;
    var meta = { truncado: false, chars: chars, ocr: o, ocrPendente: pend };
    if (pouco.length) meta.poucoTexto = pouco;
    return meta;
  }
  function anonimizarCaso(docs, op) {
    op = op || {};
    docs = (docs || []).filter(function (d) { return d && d.texto != null; });
    if (!docs.length) throw new Error('nenhum documento');
    var cortes = [], todas = [];
    docs.forEach(function (d) {
      var ps = _anonPaginas_(String(d.texto));
      if (!ps.length) ps = [''];
      cortes.push(todas.length);
      todas = todas.concat(ps);
    });
    cortes.push(todas.length);
    var nomes = docs.map(function (d) { return d.nome || ''; });
    var req = [];
    nomes.forEach(function (nm) { partesDoNomeDeArquivo(nm).forEach(function (p) { if (req.indexOf(p) < 0) req.push(p); }); });
    var lista = '';
    if (docs.length > 1) {
      lista = 'DOCUMENTOS DESTE CASO (' + docs.length + ', anonimizados juntos, com um mapa só): ' + docs.map(function (d, i) {
        return (i + 1) + ') «DOC' + (i + 1) + '» — páginas ' + (cortes[i] + 1) + ' a ' + cortes[i + 1] + ' do caso';
      }).join('; ') + '.';
    }
    var oo = Object.assign({}, op, { meta: docs.length === 1 ? (docs[0].meta || null) : juntarMeta(docs, cortes), req: req.join('; '), cabExtra: lista });
    var n = nucleo(todas.join('\f') + '\f', oo), r = n.r, mapa = r.mapa;
    var anon = nomes.map(function (nm) { return nomeAnonimizado(nm, mapa); });
    if (lista) {
      n.cab = n.cab.replace(/«DOC(\d+)»/g, function (m0, k) { return '"' + anon[+k - 1] + '"'; });
    }
    var rotulo = function (g) {
      if (docs.length === 1) return 'p. ' + g;
      for (var i = 0; i < docs.length; i++) if (g > cortes[i] && g <= cortes[i + 1]) return anon[i] + ', p. ' + (g - cortes[i]);
      return 'p. ' + g;
    };
    var documentos = docs.map(function (d, i) {
      var pa = r.paginasAnon.slice(cortes[i], cortes[i + 1]);
      var paginas = pa.map(function (p, k) { return _anonCorpoPaginas_([_anonSemCarimbosSaida_(p)], k + 1); });
      var texto = n.cab + '\n\n' + _anonSemCarimbosSaida_(docs.length === 1 ? n.corpo : _anonCorpoPaginas_(pa, 1).replace(/\s+$/, ''));
      return { nome: d.nome || '', nomeSaida: anon[i], paginas: paginas, texto: texto, de: cortes[i] + 1, ate: cortes[i + 1] };
    });
    var res = {
      api: 2, versao: ANON_VERSAO, cabecalho: n.cab, documentos: documentos, estatisticas: n.estat,
      confira: montarConfira(r, n.pg, n.rig, op, rotulo), assinatura: assinatura(mapa, nomes, n.idCaso), idCaso: n.idCaso,
      paginasCaso: r.paginasAnon.map(function (p) { return _anonSemCarimbosSaida_(p); })
    };
    if (op.incluirMapa) res.mapa = mapa;
    return res;
  }

  // ─ A reidentificação, como o 🔓 do Graphé: cada token do mapa volta a ser o nome (_anonReidentificarTexto_, que aceita
  // os delimitadores que a IA tenha trocado — «», <<>>, [[]], {{}} — e só troca nome grego com delimitador), o título
  // também (_anonReidTitulo_), e o que sobrou (órfãos, restantes, malformados: _anonSobrasReid_) vai para o aviso.
  function lerMapa(mapa) {
    if (typeof mapa === 'string') { try { mapa = JSON.parse(mapa); } catch (e) { mapa = null; } }
    if (!mapa || typeof mapa !== 'object' || !mapa.tokens || typeof mapa.tokens !== 'object') {
      throw new Error('Este arquivo não é um mapa de anonimização (do Incognitus ou do Graphé).');
    }
    return mapa;
  }

  function reidentificar(texto, mapa, titulo) {
    mapa = lerMapa(mapa);
    var r = _anonReidentificarTexto_(texto, mapa);
    var t = titulo ? _anonReidTitulo_(titulo, mapa) : { texto: '', n: 0 };
    return { texto: r.texto, n: r.n, titulo: t.texto, nTitulo: t.n,
      sobras: _anonSobrasListadas_(_anonSobrasReid_(r.texto, t.texto, mapa)) };
  }

  // Para documento com formatação (.docx): ONDE trocar, em cada trecho (um parágrafo), para o app trocar sem mexer no
  // resto. Os padrões e a ordem (o token mais longo primeiro) são os de _anonReidentificarTexto_; o que já foi trocado
  // não se troca de novo. Cada trecho: [{ i, f, v }] — do caractere i ao f (exclusivo) entra o valor v.
  function trechos(textos, mapa) {
    mapa = lerMapa(mapa);
    var tokens = Object.keys(mapa.tokens).sort(function (a, b) { return b.length - a.length; });
    var res = [];
    for (var k = 0; k < (textos || []).length; k++) {
      var s = String(textos[k] == null ? '' : textos[k]), achados = [];
      // 1.11.0: o token que a IA alterou (caixa, acento, quebra ou hífen dentro dele, «CPF 01»): com o mesmo tamanho, as
      // posições valem e o texto normalizado é o que se procura; se mudou de tamanho, o parágrafo é trocado inteiro.
      if (typeof _anonNormalizarTokensReid_ === 'function') {
        var s2 = _anonNormalizarTokensReid_(s, mapa);
        if (s2 !== s && s2.length !== s.length) { res.push([{ i: 0, f: s.length, v: _anonReidentificarTexto_(s, mapa).texto }]); continue; }
        s = s2;
      }
      for (var i = 0; i < tokens.length; i++) {
        var re = new RegExp(_anonPadraoToken_(tokens[i]), 'g'), m;
        while ((m = re.exec(s)) !== null) {
          if (!m[0].length) { re.lastIndex++; continue; }
          var a = m.index, b = a + m[0].length, livre = true;
          for (var j = 0; j < achados.length && livre; j++) if (a < achados[j].f && b > achados[j].i) livre = false;
          if (livre) achados.push({ i: a, f: b, v: mapa.tokens[tokens[i]].valor });
        }
      }
      res.push(achados.sort(function (x, y) { return x.i - y.i; }));
    }
    return res;
  }

  function sobras(texto, titulo, mapa) {
    mapa = lerMapa(mapa);
    return _anonSobrasListadas_(_anonSobrasReid_(texto, titulo || '', mapa));
  }

  // O dicionário do mapa, para a janela: cada token e o nome (ou o dado) real.
  function dicionario(mapa) {
    mapa = lerMapa(mapa);
    var lista = Object.keys(mapa.tokens).filter(function (t) { return mapa.tokens[t] && mapa.tokens[t].tipo !== 'ALIAS'; })
      .map(function (t) { return { token: t, valor: mapa.tokens[t].valor, tipo: mapa.tokens[t].tipo }; });
    return { idCaso: mapa.idCaso || '', versao: mapa.versao || '', criado: mapa.criado || '', pessoas: contarPessoas(mapa), lista: lista };
  }

  return {
    api: 2, versao: ANON_VERSAO, configurar: configurar, lerPdftotext: lerPdftotext, aplicarOcr: aplicarOcr,
    anonimizar: anonimizar, anonimizarCaso: anonimizarCaso, partesDoNomeDeArquivo: partesDoNomeDeArquivo,
    nomeAnonimizado: nomeAnonimizado, charsUteisServico: charsUteisServico, textoDoOcr: textoDoOcr,
    reidentificar: reidentificar, trechos: trechos, sobras: sobras, dicionario: dicionario
  };
})();
