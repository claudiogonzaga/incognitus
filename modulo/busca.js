// Incognitus — a busca nos modelos da pasta (17/11/2026, pedido do usuário: "ele dá uma pasta com todos os seus modelos e
// o Incognitus usa BERT para tentar achar o melhor modelo"). Um BERT de frases (intfloat/multilingual-e5-small, MIT,
// 12 camadas, 384 dimensões, na versão int8 do Xenova) transforma cada modelo e cada pedido de busca num vetor; o modelo
// mais parecido é o de maior cosseno. Roda no próprio computador (onnxruntime-web, WebAssembly), sem rede.
//
// - Tokenizador Unigram (SentencePiece, o do XLM-RoBERTa) próprio, lido do tokenizer.json: normalização NFKC, espaços
//   viram "▁" com um no começo, e a melhor segmentação (Viterbi) pelas pontuações do vocabulário. Sem dependências.
// - O e5 pede prefixos: "query: " na busca e "passage: " no modelo. Média dos vetores (só onde há texto) e norma 1.
// Este arquivo não depende do navegador nem do Node: recebe o `ort` (onnxruntime) e os bytes do modelo.
var SkiaBusca = (function () {
  var MAX = 512;

  /** O tokenizer.json (Hugging Face, Unigram) → { pecas: Map(peça → [id, nota]), unk, bos, eos, pad, maxLen }. */
  function lerTokenizer(json) {
    var t = typeof json === 'string' ? JSON.parse(json) : json;
    if (!t.model || t.model.type !== 'Unigram') throw new Error('o tokenizer não é Unigram');
    var pecas = new Map(), maxLen = 0;
    t.model.vocab.forEach(function (par, id) {
      pecas.set(par[0], [id, par[1]]);
      if (par[0].length > maxLen) maxLen = par[0].length;
    });
    var esp = {};
    (t.added_tokens || []).forEach(function (a) { esp[a.content] = a.id; });
    var unk = t.model.unk_id != null ? t.model.unk_id : esp['<unk>'];
    var minimo = Infinity;
    t.model.vocab.forEach(function (par) { if (par[1] < minimo) minimo = par[1]; });
    return { pecas: pecas, maxLen: Math.min(maxLen, 64), unk: unk, bos: esp['<s>'] != null ? esp['<s>'] : 0,
      eos: esp['</s>'] != null ? esp['</s>'] : 2, pad: esp['<pad>'] != null ? esp['<pad>'] : 1, notaUnk: minimo - 10 };
  }

  /** Texto → ids (sem <s> e </s>). */
  function tokenizar(tok, texto) {
    var s = String(texto || '').normalize('NFKC').replace(/[\u0000-\u0008\u000b\u000c\u000e-\u001f\u007f]/g, '')
      .replace(/\s+/g, ' ').trim();
    if (!s) return [];
    var ids = [];
    s.split(' ').forEach(function (palavra) {
      var w = '▁' + palavra, n = w.length;
      // Viterbi: melhor[i] = a maior soma de notas até a posição i
      var melhor = new Float64Array(n + 1).fill(-Infinity), de = new Int32Array(n + 1).fill(-1), qual = new Int32Array(n + 1).fill(-1);
      melhor[0] = 0;
      for (var i = 0; i < n; i++) {
        if (melhor[i] === -Infinity) continue;
        var cp = w.codePointAt(i), lg = cp > 0xffff ? 2 : 1, temUm = false;
        for (var k = 1; k <= tok.maxLen && i + k <= n; k++) {
          var p = tok.pecas.get(w.slice(i, i + k));
          if (!p) continue;
          if (k === lg) temUm = true;
          var v = melhor[i] + p[1];
          if (v > melhor[i + k]) { melhor[i + k] = v; de[i + k] = i; qual[i + k] = p[0]; }
        }
        // como no SentencePiece: sem peça de um caractere só aqui, ele também pode virar <unk> (com a nota mais baixa)
        if (!temUm) {
          var vu = melhor[i] + tok.notaUnk;
          if (vu > melhor[i + lg]) { melhor[i + lg] = vu; de[i + lg] = i; qual[i + lg] = tok.unk; }
        }
      }
      var out = [], j = n;
      while (j > 0) { out.push(qual[j]); j = de[j]; }
      out.reverse();
      // <unk> seguidos viram um só, como no SentencePiece
      out.forEach(function (id) { if (!(id === tok.unk && ids[ids.length - 1] === tok.unk)) ids.push(id); });
    });
    return ids;
  }

  /** Cria o embutidor. ort: onnxruntime; bytesModelo: Uint8Array; tokenizerJson: o texto do tokenizer.json. */
  async function criar(ort, bytesModelo, tokenizerJson, opcoes) {
    var tok = lerTokenizer(tokenizerJson);
    var sess = await ort.InferenceSession.create(bytesModelo, Object.assign({ executionProviders: ['wasm'] }, opcoes || {}));
    var entradas = sess.inputNames, saida = sess.outputNames.indexOf('last_hidden_state') >= 0 ? 'last_hidden_state' : sess.outputNames[0];

    /** Um texto → vetor (Float32Array de norma 1). `tipo`: 'query' (a busca) ou 'passage' (o modelo); outro, sem prefixo.
     *  Corta em 512 peças. */
    async function vetor(texto, tipo) {
      var ids = tokenizar(tok, (tipo === 'query' ? 'query: ' : tipo === 'passage' ? 'passage: ' : '') + texto).slice(0, MAX - 2);
      var L = ids.length + 2, x = new BigInt64Array(L), m = new BigInt64Array(L);
      x[0] = BigInt(tok.bos); for (var i = 0; i < ids.length; i++) x[i + 1] = BigInt(ids[i]); x[L - 1] = BigInt(tok.eos);
      m.fill(1n);
      var feed = { input_ids: new ort.Tensor('int64', x, [1, L]), attention_mask: new ort.Tensor('int64', m, [1, L]) };
      if (entradas.indexOf('token_type_ids') >= 0) feed.token_type_ids = new ort.Tensor('int64', new BigInt64Array(L), [1, L]);
      var r = await sess.run(feed), d = r[saida].data, H = d.length / L, v = new Float32Array(H);
      for (var t = 0; t < L; t++) for (var h = 0; h < H; h++) v[h] += d[t * H + h];
      var norma = 0;
      for (var h2 = 0; h2 < H; h2++) { v[h2] /= L; norma += v[h2] * v[h2]; }
      norma = Math.sqrt(norma) || 1;
      for (var h3 = 0; h3 < H; h3++) v[h3] /= norma;
      return v;
    }
    return { vetor: vetor, tokenizar: function (s) { return tokenizar(tok, s); } };
  }

  /** O cosseno de dois vetores de norma 1. */
  function cosseno(a, b) {
    var s = 0;
    for (var i = 0; i < a.length; i++) s += a[i] * b[i];
    return s;
  }

  return { criar: criar, lerTokenizer: lerTokenizer, tokenizar: tokenizar, cosseno: cosseno };
})();
if (typeof module !== 'undefined' && module.exports) module.exports = SkiaBusca;
if (typeof self !== 'undefined') self.SkiaBusca = SkiaBusca;
