// Incognitus — TRAVA de segurança do motor (o modelo é a do GrapheDemo/scripts/sincronizar_codigo.py: "nenhum ID real
// sobrou"). O motor.js vai para um repositório público: não pode levar ID de arquivo do Drive, URL de WebApp, ID de
// planilha, token ou chave, e-mail, prompt de IA, código do Apps Script nem nome de pessoa. Devolve a lista do que
// achou (vazia = pode seguir). Os segredos saem cortados (8 caracteres); nomes saem inteiros, para quem conferir.
//
// Nomes: sequência de 2 a 6 palavras com inicial maiúscula, com o primeiro nome no léxico de prenomes do próprio motor
// (IBGE) e o último no de sobrenomes ou prenomes. Podem ter nomes, e só elas: as listas de palavras do motor (léxicos,
// municípios, prenomes gregos), a ANON_LISTA_BRANCA_FIXA (ministros, leis e lugares, que o motor deixa em claro) e o
// ANON_INST_PADRAO (contatos institucionais públicos, com rua que homenageia alguém). Exceção pontual: TRAVA_PERMITIDOS.
//
// Uso direto: node scripts/trava.mjs motor/motor.js
import fs from 'node:fs';
import vm from 'node:vm';
import { createRequire } from 'node:module';
import { fileURLToPath } from 'node:url';

const require = createRequire(import.meta.url);
const acorn = require('acorn');

const DECLARACOES_COM_NOMES = /^(ANON_LEXICO_\w+|ANON_MUNICIPIOS_\w+|ANON_GREGOS_\w+|ANON_GREGAS_\w+|ANON_LISTA_BRANCA_FIXA|ANON_INST_PADRAO)$/;
// Nomes fictícios de exemplo (nunca de gente de verdade) que o código do Graphé usa.
export const TRAVA_PERMITIDOS = new Set([]);

const SEGREDOS = [
  ['ID de arquivo do Drive', /(?<![A-Za-z0-9_-])(1[A-Za-z0-9_-]{32}|1[A-Za-z0-9_-]{43})(?![A-Za-z0-9_-])/g, true],
  ['identificador longo', /(?<![A-Za-z0-9_-])[A-Za-z0-9_-]{25,}(?![A-Za-z0-9_-])/g, true],
  ['URL', /https?:\/\/[A-Za-z0-9-]+(?:\.[A-Za-z0-9-]+)+[^\s'"`]*/g, false],
  ['endereço do Google', /(?:docs|drive|script|sheets)\.google\.com|googleapis\.com|googleusercontent\.com|\.run\.app\b|cloudfunctions\.net/gi, false],
  ['ID de implantação', /AKfyc[A-Za-z0-9_-]{10,}/g, true],
  ['token OAuth', /ya29\.[A-Za-z0-9_-]{10,}/g, true],
  ['chave de API', /(?:sk-ant-|sk-proj-|AIza|ghp_|gho_|github_pat_|xox[abpors]-)[A-Za-z0-9_-]{8,}/g, true],
  ['chave privada', /-----BEGIN [A-Z ]*PRIVATE KEY-----/g, true],
  ['segredo', /\b(?:api[_-]?key|secret|senha|password|token)\s*[:=]\s*['"][^'"\s]{8,}/gi, true],
  ['e-mail', /[A-Za-z0-9._%+-]+@[A-Za-z0-9-]+(?:\.[A-Za-z0-9-]+)*\.[A-Za-z]{2,}/g, false],
  ['prompt', /\bprompts?\b|\bvoc[êe] [ée] (?:um|uma)\b|\bsystem\s*prompt\b|\bmax_tokens\b|\brole\s*:\s*['"](?:system|user|assistant)/gi, false],
  ['modelo de IA', /\b(?:claude|anthropic|openai|chatgpt|gemini|gpt-\d)/gi, false],
];
const APPS_SCRIPT = /^(SpreadsheetApp|DriveApp|Drive|UrlFetchApp|ScriptApp|GmailApp|DocumentApp|CacheService|PropertiesService|Session|LockService|HtmlService|CardService|CalendarApp|FormApp|SlidesApp|ContentService|MailApp)$/;
const IDENT_PROIBIDO = /gemini|claude|openai|anthropic|prompt/i;

const LIGA = new Set(['de', 'da', 'do', 'das', 'dos', 'e', 'di', 'del', 'van', 'von', 'DE', 'DA', 'DO', 'DAS', 'DOS', 'E']);
const LUGAR_ANTES = /\b(rua|r|avenida|av|travessa|tv|alameda|pra[çc]a|rodovia|estrada|bairro|jardim|vila|residencial|conjunto|fazenda|s[íi]tio|ch[áa]cara|cidade|munic[íi]pio|comarca|escola|col[ée]gio|hospital|edif[íi]cio|f[óo]rum|lei|nossa|senhora|santa|s[ãa]o|santo|instituto|funda[çc][ãa]o|universidade)\.?\s*$/i;
const semAcento = (s) => s.normalize('NFD').replace(/[̀-ͯ]/g, '');
const corta = (s) => (s.length > 8 ? s.slice(0, 8) + '…' : s);

/** Os léxicos do próprio motor (o motor roda sozinho num contexto limpo). */
function lexicos(motor) {
  const ctx = vm.createContext({});
  vm.runInContext(motor, ctx, { filename: 'motor.js', timeout: 20000 });
  const pal = (s) => String(s || '').toUpperCase().split(/\s+/).filter(Boolean).map(semAcento);
  const pre = new Set([].concat(pal(ctx.ANON_LEXICO_IBGE_PRENOMES), pal(ctx.ANON_LEXICO_PRENOMES), pal(ctx.ANON_LEXICO_NOMES)));
  const sob = new Set([].concat(pal(ctx.ANON_LEXICO_IBGE_SOBRENOMES), pal(ctx.ANON_LEXICO_SOBRENOMES)));
  if (pre.size < 1000 || sob.size < 1000) throw new Error('léxicos do motor não encontrados (prenomes ' + pre.size + ', sobrenomes ' + sob.size + ')');
  return { pre, sob };
}

/** Sequências de palavras com cara de nome de pessoa num texto. */
function nomesNoTexto(texto, lex) {
  const achados = [];
  const re = /[A-ZÀ-Ý][A-Za-zÀ-ÿ'’]+(?:\s+(?:(?:d[aeo]s?|e|di|del|van|von|D[AEO]S?|E)\s+)?[A-ZÀ-Ý][A-Za-zÀ-ÿ'’]+){1,5}/g;
  let m;
  while ((m = re.exec(texto))) {
    const pal = m[0].split(/\s+/).filter((p) => !LIGA.has(p));
    if (pal.length < 2) continue;
    const prim = semAcento(pal[0]).toUpperCase(), ult = semAcento(pal[pal.length - 1]).toUpperCase();
    if (!lex.pre.has(prim) || !(lex.sob.has(ult) || lex.pre.has(ult))) continue;
    if (LUGAR_ANTES.test(texto.slice(Math.max(0, m.index - 30), m.index))) continue;
    if (TRAVA_PERMITIDOS.has(m[0])) continue;
    achados.push(m[0]);
  }
  return achados;
}

/** Os achados da trava no texto do motor: ["linha N: categoria: trecho", ...]. */
export function travar(motor) {
  const out = [], vistos = new Set();
  const anota = (linha, cat, trecho) => { const k = cat + '|' + trecho; if (!vistos.has(k)) { vistos.add(k); out.push('linha ' + linha + ': ' + cat + ': ' + trecho); } };
  let ast;
  try { ast = acorn.parse(motor, { ecmaVersion: 'latest', sourceType: 'script', locations: true }); }
  catch (e) { return ['o motor não compila: ' + e.message]; }
  let lex;
  try { lex = lexicos(motor); } catch (e) { return ['o motor não carrega sozinho: ' + e.message]; }

  // As faixas das declarações que podem ter nomes.
  const livres = [];
  for (const n of ast.body) if (n.type === 'VariableDeclaration') for (const d of n.declarations) {
    if (d.id.type === 'Identifier' && DECLARACOES_COM_NOMES.test(d.id.name) && d.init) livres.push([d.init.start, d.init.end]);
  }
  const livre = (no) => livres.some(([a, b]) => no.start >= a && no.end <= b);

  const pilha = [ast];
  while (pilha.length) {
    const x = pilha.pop();
    for (const k in x) {
      const v = x[k];
      if (Array.isArray(v)) { for (const e of v) if (e && typeof e.type === 'string') pilha.push(e); }
      else if (v && typeof v.type === 'string' && k !== 'loc') pilha.push(v);
    }
    let texto = null, ehRegex = false;
    if (x.type === 'Literal' && typeof x.value === 'string') texto = x.value;
    else if (x.type === 'Literal' && x.regex) { texto = x.regex.pattern.replace(/\\(.)/g, '$1'); ehRegex = true; }
    else if (x.type === 'TemplateElement') texto = x.value.cooked || x.value.raw;
    else if (x.type === 'Identifier') {
      if (APPS_SCRIPT.test(x.name)) anota(x.loc.start.line, 'código do Apps Script', x.name);
      else if (IDENT_PROIBIDO.test(x.name)) anota(x.loc.start.line, 'código de IA', x.name);
      continue;
    } else continue;
    const linha = x.loc.start.line, ehLista = livre(x);
    for (const [cat, re, segredo] of SEGREDOS) {
      if (ehRegex && (cat === 'URL' || cat === 'e-mail' || cat === 'identificador longo')) continue;
      if (ehLista && (cat === 'prompt' || cat === 'modelo de IA')) continue;   // "CLAUDE" no léxico é prenome
      re.lastIndex = 0;
      let m;
      while ((m = re.exec(texto))) {
        const t = m[1] || m[0];
        if (cat === 'identificador longo' && !(/[A-Z]/.test(t) && /[a-z]/.test(t) && /\d/.test(t))) continue;
        if (cat === 'ID de arquivo do Drive' && !(/[A-Z]/.test(t) && /[a-z]/.test(t))) continue;
        anota(linha, cat, segredo ? corta(t) : t);
      }
    }
    if (!ehLista) for (const nm of nomesNoTexto(texto, lex)) anota(linha, 'nome de pessoa', nm);
  }
  return out;
}

if (process.argv[1] && fileURLToPath(import.meta.url) === fs.realpathSync(process.argv[1])) {
  const arq = process.argv[2] || 'motor/motor.js';
  const achados = travar(fs.readFileSync(arq, 'utf8'));
  if (achados.length) { console.error('PAROU: ' + achados.length + ' achado(s) em ' + arq + ':\n  ' + achados.join('\n  ')); process.exit(1); }
  console.log('trava: nada encontrado em ' + arq);
}
