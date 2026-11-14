// Incognitus — a "anonimização rigorosa": um modelo BERT de reconhecimento de entidades (NER) lê o texto que o motor já
// anonimizou e aponta o que ainda parece nome de pessoa. Roda no próprio computador (onnxruntime-web, WebAssembly), sem
// rede. Este arquivo não depende do navegador nem do Node: recebe o `ort` (onnxruntime) e os bytes do modelo.
//
// - Tokenizador WordPiece próprio (o do BERT "cased": sem minúsculas, sem tirar acento), que guarda a posição de cada
//   pedaço no texto original — o pipeline pronto do transformers.js não devolve posições.
// - Janela deslizante de 510 pedaços com sobreposição; cada pedaço fica com o rótulo da janela em que estava mais ao centro.
// - Saída: trechos { ini, fim, tipo: 'PESSOA' | 'ENDERECO', prob } no texto recebido.
var SkiaNer = (function () {
  var JAN = 510, PASSO = 384, MAX_PALAVRA = 100;

  function ehEspaco(c) { return /\s/.test(c) || c === ' '; }
  function ehControle(c) {
    var n = c.charCodeAt(0);
    return (n < 32 && c !== '\t' && c !== '\n' && c !== '\r') || (n >= 0x7f && n < 0xa0) || n === 0xfffd || n === 0x200b ||
      n === 0x200c || n === 0x200d || n === 0xfeff;
  }
  function ehPontuacao(c) {
    var n = c.charCodeAt(0);
    if ((n >= 33 && n <= 47) || (n >= 58 && n <= 64) || (n >= 91 && n <= 96) || (n >= 123 && n <= 126)) return true;
    return /\p{P}/u.test(c);   // como o BERT: só pontuação (categoria P); símbolo (©, ½) é letra
  }
  function ehCjk(c) {
    var n = c.codePointAt(0);
    return (n >= 0x4e00 && n <= 0x9fff) || (n >= 0x3400 && n <= 0x4dbf) || (n >= 0xf900 && n <= 0xfaff) || (n >= 0x20000 && n <= 0x2fa1f);
  }

  /** O vocabulário do tokenizer.json (Hugging Face) → { vocab: Map, unk, cls, sep, pad }. */
  function lerTokenizer(json) {
    var t = typeof json === 'string' ? JSON.parse(json) : json, v = new Map();
    Object.keys(t.model.vocab).forEach(function (k) { v.set(k, t.model.vocab[k]); });
    return { vocab: v, prefixo: t.model.continuing_subword_prefix || '##', unk: v.get('[UNK]'), cls: v.get('[CLS]'),
      sep: v.get('[SEP]'), pad: v.get('[PAD]') || 0 };
  }

  /** Texto → pedaços { id, ini, fim } (posições no texto original). */
  function tokenizar(tok, texto) {
    var out = [], palavras = [], i = 0, n = texto.length, ini = -1;
    // palavras: corridas sem espaço; pontuação vira palavra sozinha
    for (i = 0; i <= n; i++) {
      var c = i < n ? texto[i] : ' ';
      if (i < n && (c >= '\ud800' && c <= '\udbff') && i + 1 < n) { /* par substituto: trata como letra */ if (ini < 0) ini = i; i++; continue; }
      if (ehEspaco(c) || ehControle(c)) { if (ini >= 0) { palavras.push([ini, i]); ini = -1; } continue; }
      if (ehPontuacao(c) || ehCjk(c)) { if (ini >= 0) { palavras.push([ini, i]); ini = -1; } palavras.push([i, i + 1]); continue; }
      if (ini < 0) ini = i;
    }
    palavras.forEach(function (p) {
      var w = texto.slice(p[0], p[1]);
      if (w.length > MAX_PALAVRA) { out.push({ id: tok.unk, ini: p[0], fim: p[1] }); return; }
      var pos = 0, pedacos = [];
      while (pos < w.length) {
        var fim = w.length, achou = null;
        while (fim > pos) {
          var sub = (pos > 0 ? tok.prefixo : '') + w.slice(pos, fim);
          if (tok.vocab.has(sub)) { achou = tok.vocab.get(sub); break; }
          fim--;
        }
        if (achou === null) { pedacos = null; break; }
        pedacos.push({ id: achou, ini: p[0] + pos, fim: p[0] + fim });
        pos = fim;
      }
      if (pedacos) out.push.apply(out, pedacos); else out.push({ id: tok.unk, ini: p[0], fim: p[1] });
    });
    return out;
  }

  function softmax(arr, ofs, k) {
    var m = -Infinity, s = 0, r = new Array(k), j;
    for (j = 0; j < k; j++) if (arr[ofs + j] > m) m = arr[ofs + j];
    for (j = 0; j < k; j++) { r[j] = Math.exp(arr[ofs + j] - m); s += r[j]; }
    for (j = 0; j < k; j++) r[j] /= s;
    return r;
  }

  /**
   * Cria o reconhecedor. ort: o onnxruntime (web ou node); bytesModelo: Uint8Array do model.onnx; tokenizerJson: texto do
   * tokenizer.json; rotulos: { 0: 'O', 1: 'B-PESSOA', … } (o id2label do config.json).
   */
  async function criar(ort, bytesModelo, tokenizerJson, rotulos, opcoes) {
    var tok = lerTokenizer(tokenizerJson);
    var sess = await ort.InferenceSession.create(bytesModelo, Object.assign({ executionProviders: ['wasm'] }, opcoes || {}));
    var entradas = sess.inputNames, nomeSaida = sess.outputNames[0];
    var id2 = {}; Object.keys(rotulos).forEach(function (k) { id2[+k] = rotulos[k]; });
    var K = Object.keys(id2).length;

    async function rotularJanela(ids) {
      var L = ids.length + 2, x = new BigInt64Array(L), m = new BigInt64Array(L), z = new BigInt64Array(L);
      x[0] = BigInt(tok.cls); for (var i = 0; i < ids.length; i++) x[i + 1] = BigInt(ids[i]); x[L - 1] = BigInt(tok.sep);
      m.fill(1n);
      var feed = { input_ids: new ort.Tensor('int64', x, [1, L]), attention_mask: new ort.Tensor('int64', m, [1, L]) };
      if (entradas.indexOf('token_type_ids') >= 0) feed.token_type_ids = new ort.Tensor('int64', z, [1, L]);
      var r = await sess.run(feed), d = r[nomeSaida].data, out = [];
      for (var t = 1; t < L - 1; t++) out.push(softmax(d, t * K, K));
      return out;
    }

    /** Texto → trechos { ini, fim, tipo, prob }. */
    async function reconhecer(texto) {
      var pedacos = tokenizar(tok, texto), n = pedacos.length;
      if (!n) return [];
      var melhor = new Array(n), centro = new Array(n).fill(-1);
      for (var i = 0; ; i += PASSO) {
        var j = Math.min(n, i + JAN), probs = await rotularJanela(pedacos.slice(i, j).map(function (p) { return p.id; }));
        for (var t = i; t < j; t++) {
          var dist = Math.min(t - i, j - 1 - t);   // quanto mais ao centro da janela, melhor o contexto
          if (dist > centro[t]) { centro[t] = dist; melhor[t] = probs[t - i]; }
        }
        if (j >= n) break;
      }
      var trechos = [], cur = null;
      for (var k = 0; k < n; k++) {
        var p = melhor[k], arg = 0;
        for (var q = 1; q < K; q++) if (p[q] > p[arg]) arg = q;
        var rot = id2[arg] || 'O', tipo = rot === 'O' ? null : rot.slice(2), bio = rot.charAt(0);
        // pedaço colado no anterior (## da mesma palavra) nunca abre trecho novo
        var colado = k > 0 && pedacos[k].ini === pedacos[k - 1].fim;
        if (tipo && cur && cur.tipo === tipo && (bio === 'I' || colado)) {
          cur.fim = pedacos[k].fim; cur.soma += p[arg]; cur.n++;
        } else {
          if (cur) trechos.push(cur);
          cur = tipo ? { ini: pedacos[k].ini, fim: pedacos[k].fim, tipo: tipo, soma: p[arg], n: 1 } : null;
        }
      }
      if (cur) trechos.push(cur);
      // palavra inteira (o modelo às vezes rotula só um pedaço: "Fic|tícias") e trechos do mesmo tipo separados só por
      // espaço, vírgula ou hífen viram um só (o endereço sai em pedaços: "Rua das Palmeiras", "120", "Bairro")
      var letra = /[\p{L}\p{N}]/u, unidos = [];
      trechos.forEach(function (t) {
        while (t.ini > 0 && letra.test(texto[t.ini - 1])) t.ini--;
        while (t.fim < texto.length && letra.test(texto[t.fim])) t.fim++;
        var u = unidos[unidos.length - 1];
        if (u && u.tipo === t.tipo && t.ini <= u.fim + 3 && /^[\s,\-–]*$/.test(texto.slice(u.fim, t.ini)) && texto.slice(u.fim, t.ini).indexOf('\n') < 0) {
          u.fim = Math.max(u.fim, t.fim); u.soma += t.soma; u.n += t.n;
        } else if (u && t.ini < u.fim) {
          u.fim = Math.max(u.fim, t.fim);
        } else unidos.push(t);
      });
      return unidos.map(function (t) { return { ini: t.ini, fim: t.fim, tipo: t.tipo, prob: Math.round(t.soma / t.n * 1000) / 1000 }; });
    }

    return { reconhecer: reconhecer, tokenizar: function (s) { return tokenizar(tok, s); } };
  }

  return { criar: criar, tokenizar: tokenizar, lerTokenizer: lerTokenizer };
})();
if (typeof module !== 'undefined' && module.exports) module.exports = SkiaNer;
if (typeof self !== 'undefined') self.SkiaNer = SkiaNer;
