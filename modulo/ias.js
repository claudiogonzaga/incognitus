// Incognitus — as IAs do 🧠 (17/11/2026, pedido do usuário: "no botão de gerar a minuta deve ser possível configurar
// qual IA, qual modelo e o nível de esforço"). A conversa do 🧠 é montada no formato da Anthropic e passa pelo porteiro
// assim; só depois, na hora de sair, é traduzida para o formato da IA escolhida — e a resposta volta ao formato da
// Anthropic, para o resto do app (a análise, a minuta, as consultas, a continuação) seguir igual.
//
// - OpenAI e DeepInfra: a API "chat completions" (a DeepInfra é compatível com a da OpenAI).
// - Google Gemini: generateContent. O Gemini 3 exige que a assinatura do pensamento (thoughtSignature) de cada consulta
//   volte no pedido seguinte: ela fica guardada no próprio bloco da conversa (campo _assinatura), que só esta IA lê.
// Este arquivo não depende do navegador: os testes o carregam no Node.
var SkiaIas = (function () {
  var PROVEDORES = {
    anthropic: {
      rotulo: 'Anthropic (Claude)', curto: 'Claude', a: 'à Anthropic', da: 'da Anthropic', A: 'A Anthropic', ondeChave: 'console.anthropic.com/settings/keys', formaChave: 'sk-ant-…',
      modelos: [['claude-opus-5-5', 'Claude Opus 5.5'], ['claude-sonnet-5-5', 'Claude Sonnet 5.5 (mais barato)'],
        ['claude-fable-5-2', 'Claude Fable 5.2 (o mais capaz, o mais caro)'], ['claude-haiku-5-5', 'Claude Haiku 5.5 (o mais barato)']],
      esforcos: ['low', 'medium', 'high', 'xhigh', 'max'], padrao: 'claude-opus-5-5', esforcoPadrao: 'high',
    },
    openai: {
      rotulo: 'OpenAI (ChatGPT)', curto: 'OpenAI', a: 'à OpenAI', da: 'da OpenAI', A: 'A OpenAI', ondeChave: 'platform.openai.com/api-keys', formaChave: 'sk-…',
      modelos: [['gpt-5', 'GPT-5'], ['gpt-5-mini', 'GPT-5 mini (mais barato)'], ['gpt-4.1', 'GPT-4.1 (sem raciocínio)']],
      esforcos: ['low', 'medium', 'high'], padrao: 'gpt-5', esforcoPadrao: 'high',
    },
    gemini: {
      rotulo: 'Google Gemini', curto: 'Gemini', a: 'ao Google (Gemini)', da: 'do Google (Gemini)', A: 'O Google (Gemini)', ondeChave: 'aistudio.google.com/apikey', formaChave: 'AIza…',
      modelos: [['gemini-3.1-pro-preview', 'Gemini 3.1 Pro'], ['gemini-3.8-flash', 'Gemini 3.8 Flash (mais barato)'],
        ['gemini-flash-latest', 'Gemini Flash (o mais novo)']],
      esforcos: ['low', 'high'], padrao: 'gemini-3.1-pro-preview', esforcoPadrao: 'high',
    },
    deepinfra: {
      rotulo: 'DeepInfra (modelos abertos)', curto: 'DeepInfra', a: 'à DeepInfra', da: 'da DeepInfra', A: 'A DeepInfra', ondeChave: 'deepinfra.com/dash/keys', formaChave: 'a chave da DeepInfra',
      modelos: [['zai-org/GLM-5.2', 'GLM 5.2'], ['deepseek-ai/DeepSeek-V3.2', 'DeepSeek V3.2'], ['Qwen/Qwen3.8-Flash', 'Qwen 3.8 Flash (barato, autos longos)'],
        ['Qwen/Qwen3.5-397B-A17B', 'Qwen 3.5 397B'], ['Qwen/Qwen4-Max', 'Qwen 4 Max (o mais capaz, mais caro)'], ['Qwen/Qwen3-235B-A22B', 'Qwen3 235B']],
      esforcos: [], padrao: 'zai-org/GLM-5.2', esforcoPadrao: '',
    },
  };
  var ESFORCOS = { low: 'Baixo', medium: 'Médio', high: 'Alto', xhigh: 'Muito alto', max: 'Máximo' };

  /** A escolha valida: IA conhecida, modelo não vazio, esforço que aquela IA aceita (senão, o mais perto). */
  function normalizar(e) {
    e = e || {};
    var prov = PROVEDORES[e.provedor] ? e.provedor : 'anthropic', P = PROVEDORES[prov];
    var modelo = String(e.modelo || '').trim() || P.padrao;
    var esf = String(e.esforco || '');
    if (P.esforcos.indexOf(esf) < 0) {
      var ordem = ['low', 'medium', 'high', 'xhigh', 'max'], i = ordem.indexOf(esf);
      esf = '';
      if (P.esforcos.length) {
        // o nível pedido ou o mais perto abaixo dele; sem nível, o padrão da IA
        if (i < 0) esf = P.esforcoPadrao;
        else for (var k = i; k >= 0 && !esf; k--) if (P.esforcos.indexOf(ordem[k]) >= 0) esf = ordem[k];
        if (!esf) esf = P.esforcos[0];
      }
    }
    return { provedor: prov, modelo: modelo, esforco: esf };
  }

  function nomeDoModelo(prov, modelo) {
    var P = PROVEDORES[prov] || PROVEDORES.anthropic;
    for (var i = 0; i < P.modelos.length; i++) if (P.modelos[i][0] === modelo) return P.modelos[i][1].replace(/\s*\(.*\)$/, '');
    return modelo;
  }

  // ── da Anthropic para a IA escolhida ───────────────────────────────────────────────────────────────────────────
  function textoDoSystem(s) {
    if (typeof s === 'string') return s;
    return (s || []).map(function (b) { return b && b.type === 'text' ? b.text : ''; }).join('\n\n');
  }
  function blocos(c) { return typeof c === 'string' ? [{ type: 'text', text: c }] : (c || []); }
  function textoDoResultado(c) {
    if (typeof c === 'string') return c;
    return (c || []).map(function (b) { return b && b.type === 'text' ? b.text : ''; }).join('\n');
  }

  /** Para a OpenAI e a DeepInfra (chat completions). */
  function paraOpenAi(corpo, e) {
    var msgs = [{ role: 'system', content: textoDoSystem(corpo.system) }];
    (corpo.messages || []).forEach(function (m) {
      var bs = blocos(m.content);
      if (m.role === 'assistant') {
        var txt = bs.filter(function (b) { return b.type === 'text'; }).map(function (b) { return b.text; }).join('');
        var chamadas = bs.filter(function (b) { return b.type === 'tool_use'; }).map(function (b) {
          return { id: b.id, type: 'function', function: { name: b.name, arguments: JSON.stringify(b.input || {}) } };
        });
        var am = { role: 'assistant', content: txt || null };
        if (chamadas.length) am.tool_calls = chamadas;
        msgs.push(am);
      } else {
        // os resultados das consultas vêm primeiro, cada um como mensagem "tool", logo depois das chamadas
        bs.filter(function (b) { return b.type === 'tool_result'; }).forEach(function (b) {
          msgs.push({ role: 'tool', tool_call_id: b.tool_use_id, content: (b.is_error ? 'ERRO: ' : '') + textoDoResultado(b.content) });
        });
        var t = bs.filter(function (b) { return b.type === 'text'; }).map(function (b) { return b.text; }).join('\n\n');
        if (t) msgs.push({ role: 'user', content: t });
      }
    });
    var out = { model: e.modelo, messages: msgs };
    if (corpo.tools && corpo.tools.length) {
      out.tools = corpo.tools.map(function (f) { return { type: 'function', function: { name: f.name, description: f.description, parameters: f.input_schema } }; });
    }
    if (e.provedor === 'openai' && e.esforco && /^(gpt-5|o\d)/.test(e.modelo)) out.reasoning_effort = e.esforco === 'low' ? 'low' : e.esforco === 'medium' ? 'medium' : 'high';
    if (e.provedor === 'deepinfra') out.max_tokens = 32000;
    return out;
  }

  function semExtras(schema) {
    if (!schema || typeof schema !== 'object') return schema;
    if (Array.isArray(schema)) return schema.map(semExtras);
    var o = {};
    Object.keys(schema).forEach(function (k) { if (k !== 'additionalProperties' && k !== '$schema') o[k] = semExtras(schema[k]); });
    return o;
  }

  /** Para o Gemini (generateContent). */
  function paraGemini(corpo, e) {
    var nomes = {}, contents = [];
    (corpo.messages || []).forEach(function (m) {
      var bs = blocos(m.content), parts = [];
      if (m.role === 'assistant') {
        bs.forEach(function (b) {
          if (b.type === 'text' && b.text) { var p = { text: b.text }; if (b._assinatura) p.thoughtSignature = b._assinatura; parts.push(p); }
          if (b.type === 'tool_use') {
            nomes[b.id] = b.name;
            var c = { functionCall: { name: b.name, args: b.input || {} } };
            if (b._assinatura) c.thoughtSignature = b._assinatura;
            parts.push(c);
          }
        });
        if (parts.length) contents.push({ role: 'model', parts: parts });
      } else {
        bs.forEach(function (b) {
          if (b.type === 'tool_result') {
            var r = { resultado: textoDoResultado(b.content) };
            if (b.is_error) r.erro = true;
            parts.push({ functionResponse: { name: nomes[b.tool_use_id] || 'ferramenta', response: r } });
          } else if (b.type === 'text' && b.text) parts.push({ text: b.text });
        });
        if (parts.length) contents.push({ role: 'user', parts: parts });
      }
    });
    var out = { model: e.modelo, systemInstruction: { parts: [{ text: textoDoSystem(corpo.system) }] }, contents: contents };
    if (corpo.tools && corpo.tools.length) {
      out.tools = [{ functionDeclarations: corpo.tools.map(function (f) { return { name: f.name, description: f.description, parameters: semExtras(f.input_schema) }; }) }];
    }
    if (e.esforco) {
      if (/gemini-3|gemini-flash-latest|gemini-pro-latest/.test(e.modelo)) out.generationConfig = { thinkingConfig: { thinkingLevel: e.esforco === 'low' ? 'low' : 'high' } };
      else if (/gemini-2\.5/.test(e.modelo)) out.generationConfig = { thinkingConfig: { thinkingBudget: e.esforco === 'low' ? 2048 : -1 } };
    }
    return out;
  }

  // ── da IA escolhida de volta para a Anthropic ──────────────────────────────────────────────────────────────────
  var n = 0;
  function deOpenAi(j) {
    var c = (j.choices || [])[0] || {}, msg = c.message || {}, content = [];
    if (msg.content) content.push({ type: 'text', text: String(msg.content) });
    (msg.tool_calls || []).forEach(function (t) {
      var input = {};
      try { input = JSON.parse((t.function && t.function.arguments) || '{}') || {}; } catch (e) { input = { _ilegivel: String((t.function && t.function.arguments) || '') }; }
      content.push({ type: 'tool_use', id: t.id || ('call_' + (++n)), name: (t.function && t.function.name) || '', input: input });
    });
    var fim = c.finish_reason, u = j.usage || {}, cache = (u.prompt_tokens_details && u.prompt_tokens_details.cached_tokens) || 0;
    var stop = msg.refusal ? 'refusal' : fim === 'length' ? 'max_tokens' : fim === 'content_filter' ? 'refusal'
      : (fim === 'tool_calls' || content.some(function (b) { return b.type === 'tool_use'; })) ? 'tool_use' : 'end_turn';
    return { role: 'assistant', model: j.model || '', content: content, stop_reason: stop,
      usage: { input_tokens: Math.max(0, (u.prompt_tokens || 0) - cache), cache_read_input_tokens: cache, output_tokens: u.completion_tokens || 0 } };
  }

  function deGemini(j) {
    var c = (j.candidates || [])[0] || {}, parts = (c.content && c.content.parts) || [], content = [];
    parts.forEach(function (p) {
      if (p.thought) return;   // o resumo do pensamento não entra na resposta
      if (typeof p.text === 'string') {
        var ult = content[content.length - 1];
        if (ult && ult.type === 'text' && !p.thoughtSignature) { ult.text += p.text; return; }
        var b = { type: 'text', text: p.text };
        if (p.thoughtSignature) b._assinatura = p.thoughtSignature;
        content.push(b);
      } else if (p.functionCall) {
        var t = { type: 'tool_use', id: p.functionCall.id || ('gcall_' + (++n)), name: p.functionCall.name, input: p.functionCall.args || {} };
        if (p.thoughtSignature) t._assinatura = p.thoughtSignature;
        content.push(t);
      }
    });
    var fim = c.finishReason || '', u = j.usageMetadata || {}, cache = u.cachedContentTokenCount || 0;
    var bloqueio = (j.promptFeedback && j.promptFeedback.blockReason) || /SAFETY|RECITATION|BLOCKLIST|PROHIBITED|SPII/.test(fim);
    var stop = bloqueio ? 'refusal' : fim === 'MAX_TOKENS' ? 'max_tokens'
      : content.some(function (b) { return b.type === 'tool_use'; }) ? 'tool_use' : 'end_turn';
    return { role: 'assistant', model: '', content: content, stop_reason: stop,
      usage: { input_tokens: Math.max(0, (u.promptTokenCount || 0) - cache), cache_read_input_tokens: cache,
        output_tokens: (u.candidatesTokenCount || 0) + (u.thoughtsTokenCount || 0) } };
  }

  function para(prov, corpo, e) { return prov === 'gemini' ? paraGemini(corpo, e) : paraOpenAi(corpo, e); }
  function de(prov, json) { return prov === 'gemini' ? deGemini(json) : deOpenAi(json); }

  return { PROVEDORES: PROVEDORES, ESFORCOS: ESFORCOS, normalizar: normalizar, nomeDoModelo: nomeDoModelo, para: para, de: de,
    paraOpenAi: paraOpenAi, paraGemini: paraGemini, deOpenAi: deOpenAi, deGemini: deGemini };
})();
if (typeof module !== 'undefined' && module.exports) module.exports = SkiaIas;
if (typeof self !== 'undefined') self.SkiaIas = SkiaIas;
