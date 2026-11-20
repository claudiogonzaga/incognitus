// Incognitus — o CONTEXTO (19/11/2026, pedido do usuário: "cargo, município ficam em escuro também; comarca também;
// nome de empresa, fazenda e apelido (datas não)" e "add Alerta de contexto").
//
// O motor troca os identificadores diretos (nome, CPF, endereço, telefone). Mas "o único tabelião do município X de 8 mil
// habitantes" identifica a pessoa sem nome nenhum. Aqui, depois do motor:
//   1. ACHAR: o que no texto é município ou comarca, cargo, empresa ou estabelecimento, fazenda (e sítio, gleba,
//      assentamento), apelido e o número do processo (os quatro últimos dígitos dele são o código da comarca). Cada um
//      vira um token do mapa do caso («MUNICIPIO_01», «CARGO_01», «EMPRESA_01», «FAZENDA_01», «APELIDO_01»,
//      «PROCESSO_01»), e o porteiro troca todas as ocorrências. Na reidentificação, voltam como qualquer outro token.
//   2. ALERTAR: o que ainda sobra de contexto que pode identificar alguém e que regra nenhuma troca com segurança ("o
//      único…", "filho do prefeito", "dono do posto", o tamanho da cidade, o bairro, a rodovia). Só aponta; quem decide
//      é quem usa.
// Tudo por regra (expressões regulares), no computador. As datas ficam em claro, de propósito.
(function (raiz) {
'use strict';

const MAI = 'A-ZÀ-ÖØ-Þ', LET = 'A-Za-zÀ-ÖØ-öø-ÿ';
// uma palavra de nome próprio: começa em maiúscula (ou é toda em maiúsculas), com letras e apóstrofo
const P = '[' + MAI + '][' + LET + "'’]+";
const LIGA = '(?:d[aeo]s?|D[AEO]S?|e|E)';
// um nome próprio de até n palavras, com as ligações ("Santo Antônio do Leverger"); não atravessa quebra de linha
const nome = (n) => P + '(?:[ ]' + '(?:' + LIGA + '[ ])?' + P + '){0,' + (n - 1) + '}';
const NOME_NUM = (n, liga) => '(?:' + P + '|\\d{1,4})(?:[ ](?:' + (liga || LIGA) + '[ ])?(?:' + P + '|\\d{1,4}|[IVX]{1,4}))' + '{0,' + (n - 1) + '}';

const chave = (s) => String(s == null ? '' : s).normalize('NFD').replace(/[̀-ͯ]/g, '').toUpperCase().replace(/[^A-Z0-9]+/g, ' ').trim();
const conj = (s) => new Set(s.split(/\s+/).filter(Boolean));

const UFS = 'AC|AL|AP|AM|BA|CE|DF|ES|GO|MA|MT|MS|MG|PA|PB|PR|PE|PI|RJ|RN|RS|RO|RR|SC|SP|SE|TO';
// onde um nome de lugar acaba (ou o que nunca é o nome dele)
const PARA = conj('ESTADO VARA JUIZO COMARCA MUNICIPIO CIDADE DISTRITO PODER TRIBUNAL PROMOTORIA PROMOTORIAS MINISTERIO PROCESSO AUTOS CLASSE NUM CEP FONE ' +
  'RUA AVENIDA BAIRRO CENTRO JUSTICA POLICIA VEREADORES ORIGEM DIREITO FAMILIA ENTRANCIA EXECUCAO EXECUCOES INFANCIA FAZENDA REGISTRO NOTAS PROTESTO ' +
  'TITULOS DELEGACIA FORUM PREFEITURA CAMARA SECRETARIA CIVIL CRIMINAL CIVEL PUBLICO PUBLICA FEDERAL ESTADUAL MUNICIPAL JUDICIARIO JUDICIARIA NATURAL ' +
  'RESIDENTE NASCIDO NASCIDA DOMICILIADO DOMICILIADA DATA DIA HORA ONDE QUE PARA COM POR FOI ERA SER ESTA ESTAVA NESTA NESTE DESTA DESTE REPUBLICA BRASIL ' +
  'UNICA ESPECIALIZADA GERAL REGIONAL CENTRAL ZONA RURAL URBANA DEFENSORIA PROCURADORIA CARTORIO OFICIO TERMO AUDIENCIA SENTENCA DECISAO DESPACHO ' +
  // rótulos de formulário ("MUNICÍPIO DE NASCIMENTO E UF", "Município de Residência") e órgãos que vêm antes do lugar
  'NASCIMENTO UNIDADE FEDERACAO UF RESIDENCIA OCORRENCIA FATO FATOS DESTINO EMISSAO LOTACAO DOMICILIO NATURALIDADE ENDERECO EXPEDICAO ATUACAO ' +
  'MEDICINA LEGAL GERENCIA DIRETORIA COORDENADORIA NUCLEO POLO SECAO SUBSECAO CIRCUNSCRICAO BATALHAO COMPANHIA PELOTAO DESTACAMENTO CADEIA ' +
  'PENITENCIARIA PRESIDIO HOSPITAL SR SRA DR DRA EXMO EXMA MM ' + UFS.replace(/\|/g, ' '));
// à esquerda de "X/MT", o nome começa depois da preposição ("… EM X/MT", "… PARA X - MT")
const CORTE = conj('EM NA NO NAS NOS PARA ATE AO AOS A COM POR E OU QUE');

const SIGLAS = conj('SSP SESP PJC PM PC OAB CRM CREA CRC CRO CRF CRP COREN DETRAN MP MPE MPF TJ TCE TRE TRT TRF DPE PGE PGJ SEMA SEFAZ SEJUS SEDUC SES INDEA CBM PRF ' +
  'PF DPF INSS IBAMA INCRA UF RG CPF CNH GAECO DHPP DERF DEA CISC POLITEC SINFRA INTERMAT JUCEMAT CIRETRAN UFMT UNEMAT IFMT SENAI SESI SEBRAE CAEX CAOP ' +
  'SEDEC SETAS SEAF SINJ STJ STF CNJ CNMP PJE SIMP SEEU BO APF IP TCO NF CTPS PIS SUS UPA PSF CRAS CREAS CAPS ' +
  'RMS HC RHC RESP ARESP AGRG AGINT ARE AI MS ADI ADPF EDCL CC RCL PET INQ AP APN RVCR HDE SL STA RG EXT');
// a palavra que ocupa o lugar de um nome mas é papel no processo ou termo comum
const PAPEIS = conj('REQUERIDA REQUERIDO REQUERENTE AUTORA AUTOR RE REU EXECUTADA EXECUTADO EXEQUENTE INVESTIGADA INVESTIGADO DENUNCIADA DENUNCIADO VITIMA ' +
  'RECORRENTE RECORRIDA RECORRIDO APELANTE APELADA APELADO CONTRATADA CONTRATANTE LICITANTE VENCEDORA INDICIADO INDICIADA ACUSADO ACUSADA ' +
  'TESTEMUNHA DECLARANTE OFENDIDO OFENDIDA REPRESENTADO REPRESENTADA NOTICIANTE NOTICIADO NOTICIADA FLAGRANTEADO CONDUZIDO CUSTODIADO ' +
  'SUPRACITADA REFERIDA MENCIONADA CITADA');
const GENERICOS = conj('SAUDE GASOLINA COMBUSTIVEL COMBUSTIVEIS FISCAL POLICIAL ATENDIMENTO MUNICIPAL ESTADUAL FEDERAL PUBLICO PUBLICA JUDICIAL CENTRAL REGIONAL ' +
  'GERAL NACIONAL ELETRONICO ELETRONICA TERAPEUTICA TERAPEUTICO INDIGENA RURAL VIZINHA VIZINHO TELEFONICA DIRETA CATOLICA EVANGELICA REGISTRO NOTAS OFICIO ' +
  'DISTRIBUIDOR ELEITORAL MILITAR CIVIL PENAL PRISIONAL INFANTIL BASICO BASICA LOCAL ONDE QUE PARA COM POR FOI ERA ESTAVA');

// num depoimento todo em maiúsculas, "BAR DO TONHÃO QUANDO CHEGOU" não é o nome inteiro: o nome acaba na palavra comum
const COMUNS = conj('QUANDO ONDE QUE PARA COM POR FOI ERA ESTAVA CHEGOU NO NA NOS NAS EM SE NAO MAS COMO ATE APOS DEPOIS ANTES SEM SOBRE ENTRE SEU SUA SEUS SUAS ' +
  'ELE ELA ELES ELAS TINHA HAVIA DISSE VIU ESTA ESTE ESSA ESSE AQUELE AQUELA AO AOS PELA PELO UM UMA OU JA TAMBEM AINDA PORQUE POIS LOGO ENTAO ' +
  'SER TER FAZ FEZ VAI IA SAO TEM TEVE FICA FICOU SAIU ENTROU PEDIU FALOU OUVIU QUAL QUEM CUJO CUJA MEU MINHA ISSO ISTO AQUI ALI LA');
const ehComum = (w) => /^[A-ZÀ-Þ]+$/.test(w) && COMUNS.has(chave(w));

/** Tira do começo e do fim o que não é do nome (a ligação solta, a palavra de parada) e devolve as palavras. */
function aparar(texto, opts) {
  let ws = String(texto || '').trim().split(/ +/);
  // o nome acaba na primeira palavra de parada (salvo se `direita`: aí vale o que vem DEPOIS da última)
  if (opts && opts.direita) {
    let k = -1;
    ws.forEach((w, n) => { if (PARA.has(chave(w)) || SIGLAS.has(chave(w)) || CORTE.has(chave(w))) k = n; });
    ws = ws.slice(k + 1);
  } else {
    const k = ws.findIndex((w, n) => n > 0 && (PARA.has(chave(w)) || ehComum(w)));
    if (k > 0) ws = ws.slice(0, k);
  }
  const liga = (w) => /^(?:d[aeo]s?|e)$/i.test(w);
  while (ws.length && liga(ws[0])) ws.shift();
  while (ws.length && liga(ws[ws.length - 1])) ws.pop();
  return ws;
}

const TIPOS = {
  MUNICIPIO: 'município ou comarca', CARGO: 'cargo', EMPRESA: 'empresa ou estabelecimento', FAZENDA: 'fazenda, sítio ou gleba',
  APELIDO: 'apelido', PROCESSO: 'número do processo',
  JUIZ: 'juiz', PROMOTOR: 'promotor de justiça', DELEGADO: 'delegado', ADVOGADO: 'advogado', DEFENSOR: 'defensor', POLICIAL: 'policial', PERITO: 'perito',
  SERVIDOR: 'servidor ou outra autoridade',
};

// ── 1. ACHAR ──────────────────────────────────────────────────────────────────────────────────────────────────────

const CABECAS_MUN = 'munic[ií]pio|comarca|cidade|distrito|prefeitura(?: municipal)?|c[âa]mara(?: municipal)?(?: de vereadores)?|f[óo]rum(?: da comarca)?|' +
  'delegacia(?: de pol[ií]cia)?(?: judici[áa]ria)?(?: civil| municipal)?|promotorias?(?: de justi[çc]a)?(?: c[ií]vel| criminal)?(?: da comarca)?|' +
  'vara [úu]nica(?: da comarca)?|natural|naturalidade:?|nascid[oa]|residente(?: e domiciliad[oa])?(?: n[oa] cidade)?';
// a cabeça vale em qualquer caixa ("COMARCA DE", "Comarca de"); o nome que vem depois, só com maiúscula (em dois passos)
const RE_MUN_CABECA = new RegExp('(?<![' + LET + '])(?:' + CABECAS_MUN + ')\\s+(?:de|do|da|em)\\s+', 'gi');
const depois = (fonte) => { const re = new RegExp(fonte, 'y'); return (t, i) => { re.lastIndex = i; const m = re.exec(t); return m ? m[0] : ''; }; };
const NOME6 = depois(nome(6)), NOME3 = depois(nome(3)), NOMENUM6 = depois('["“]?(?:' + NOME_NUM(6) + ')');
// "X/MT", "X - MT", "X-MT". O "-SE" colado é verbo ("INTIME-SE", "CUMPRA-SE"), não Sergipe: para SE, só com barra ou com espaços
const RE_MUN_UF = new RegExp('(' + nome(6) + ')(?:\\s?\\/\\s?|\\s[\\-–]\\s)(?:' + UFS + ')(?![' + LET + '])|(' + nome(6) + ')[\\-–](?:' + UFS.replace('|SE', '') + ')(?![' + LET + '])', 'g');
const RE_CNJ = /(?<![\d.\-])(?:\d{7}-\d{2}\.\d{4}\.\d\.\d{2}\.\d{4}|\d{9}(?:19|20)\d{2}\d{7})(?![\d.\-])/g;

const CARGOS = [
  'vice-?prefeit[oa]s?(?: municipal)?', 'prefeit[oa]s?(?: municipal)?', 'vereador(?:a|es|as)?', 'tabeli(?:ão|ã|ães|ões|oa)(?: substitut[oa])?',
  'registrador(?:a|es)?', 'not[áa]ri[oa]s?', 'cartor[áa]ri[oa]s?', 'oficial(?:a)? d[eo] (?:registro|cart[óo]rio)', 'escrevente juramentad[oa]',
  'conselheir[oa]s? tutelar(?:es)?', 'secretári[oa]s? (?:municipal|estadual|adjunt[oa])(?: d[aeo]s? [' + LET + ']{4,})?', 'secretári[oa]s? d[aeo] (?:sa[úu]de|educa[çc][ãa]o|obras|finan[çc]as|fazenda|administra[çc][ãa]o|agricultura|assist[êe]ncia social|meio ambiente|governo|planejamento|esportes?|cultura|turismo|transportes?|infraestrutura)',
  'deputad[oa]s?(?: estadual| federal)?', 'senador(?:a)?', 'governador(?:a)?', 'vice-?governador(?:a)?',
  'presidente d[aeo] (?:c[âa]mara(?: municipal)?(?: de vereadores)?|sindicato|associa[çc][ãa]o|cooperativa|partido|conselho|clube|col[ôo]nia)',
  'diretor(?:a)?(?: geral| administrativ[oa]| financeir[oa]| cl[ií]nic[oa])? d[aeo] (?:escola|col[ée]gio|hospital|pres[ií]dio|cadeia|penitenci[áa]ria|creche|unidade|posto|ag[êe]ncia|cooperativa|empresa)',
  'gerente(?: geral)? d[aeo] (?:banco|ag[êe]ncia|cooperativa|loja|posto|fazenda|empresa)',
  'pastor(?:a|es)?', 'padre', 'p[áa]roco', 'bispo', 'mission[áa]ri[oa]', 'cacique',
  'procurador(?:a)?(?:-geral)? d[oe] munic[ií]pio', 'controlador(?:a)? (?:geral|intern[oa])', 'pregoeir[oa]', 'tesoureir[oa]', 'chefe de gabinete', 'comandante',
];
const RE_CARGO = new RegExp('(?<![' + LET + '«_-])(?:' + CARGOS.join('|') + ')(?![' + LET + '»_])', 'gi');

const SUFIXO_PJ = '(?:LTDA\\.?|Ltda\\.?|S\\/A|S\\.A\\.?|EIRELI|Eireli)(?:\\s?[-–]\\s?(?:ME|EPP))?';
const RE_PJ_SUFIXO = new RegExp('(' + NOME_NUM(8, '(?:d[aeo]s?|D[AEO]S?|e|E|&)') + ')[ ,]{1,2}(' + SUFIXO_PJ + '|[-–]\\s?(?:ME|EPP|MEI)(?![' + LET + ']))', 'g');
const RE_PJ_CABECA = new RegExp('(?<![' + LET + '])(?:empresa|firma|raz[ãa]o social:?|nome fantasia:?|estabelecimento(?: comercial)?(?: denominad[oa])?|denominad[oa])\\s+', 'gi');
const LOCAIS = 'Bar|Lanchonete|Restaurante|Mercado|Supermercado|Mercearia|Padaria|Farm[áa]cia|Drogaria|Auto Posto|Posto|Oficina|Borracharia|Loja|Hotel|Pousada|' +
  'Motel|Boate|Conveni[êe]ncia|Distribuidora|Madeireira|Serraria|Frigor[ií]fico|Cerealista|Armaz[ée]m|Transportadora|Construtora|Imobili[áa]ria|' +
  'Cooperativa|Sindicato|Associa[çc][ãa]o|Igreja|Escola(?: Estadual| Municipal)?|Col[ée]gio|Creche|Hospital|Cl[ií]nica|Laborat[óo]rio|Academia|' +
  'Sal[ãa]o|Barbearia|Clube|Garimpo|Agropecu[áa]ria|Cart[óo]rio';
const maiusculas = (re) => re.toUpperCase();   // as listas não têm \\d nem \\s: só letras, classes e grupos
const RE_LOCAL = new RegExp('(?<![' + LET + '«])((?:' + LOCAIS + '|' + maiusculas(LOCAIS) + ')[ ](?:' + LIGA + '[ ])?' + NOME_NUM(4) + ')', 'g');
const TERRAS = 'Fazenda|Faz\\.|S[ií]tio|Ch[áa]cara|Est[âa]ncia|Rancho|Gleba|Projeto de Assentamento|Assentamento|P\\.A\\.|Retiro|Haras|Granja|Agrovila|Aldeia|Comunidade|Linha|Col[ôo]nia';
const RE_TERRA = new RegExp('(?<![' + LET + '«])((?:' + TERRAS + '|' + maiusculas(TERRAS) + ')[ ](?:' + LIGA + '[ ])?' + NOME_NUM(5) + ')', 'g');
const ANTES_APELIDO = '(?:vulg[oa]|alcunha(?: de)?|apelido(?: de)?|apelidad[oa](?: de| como)?|codinome|conhecid[oa](?: popularmente| vulgarmente)? (?:como|por)|chamad[oa] de)';
const RE_APELIDO_ASPAS = new RegExp('(?<![' + LET + '])' + ANTES_APELIDO + '[ :]{1,3}["“”\'‘’]([^"“”\'‘’«»\\n]{2,30})["“”\'‘’]', 'gi');
const RE_APELIDO_SOLTO = new RegExp('(?<![' + LET + '])' + ANTES_APELIDO + '[ :]{1,3}', 'gi');

const semToken = (s) => s.indexOf('«') < 0 && s.indexOf('»') < 0;

/** O que há de contexto num texto já anonimizado. [{ tipo, valor }], sem repetir (o mais comprido primeiro). */
function achar(texto) {
  const t = String(texto || ''), out = new Map();
  const por = (tipo, valor) => {
    const v = String(valor || '').replace(/\s+/g, ' ').replace(/^[\s"“”'‘’,.;:()\-–]+|[\s"“”'‘’,.;:()\-–]+$/g, '');
    const k = chave(v);
    if (!v || !semToken(v) || k.replace(/ /g, '').length < 3 || /^X+$/.test(k.replace(/ /g, ''))) return;
    if (!out.has(tipo + ':' + k)) out.set(tipo + ':' + k, { tipo, valor: v });
  };
  let m;
  const cada = (re, fn) => { re.lastIndex = 0; while ((m = re.exec(t))) { if (!m[0]) { re.lastIndex++; continue; } fn(m); } };

  // o município e a comarca: pelo que vem antes ("Comarca de X", "natural de X") e pela sigla do estado ("X/MT", "X - MT")
  const municipio = (ws) => {
    if (!ws.length || ws.length > 6) return;
    const k0 = chave(ws[0]);
    if (PARA.has(k0) || PAPEIS.has(k0) || GENERICOS.has(k0) || SIGLAS.has(k0) || /^(?:SUL|NORTE|LESTE|OESTE)$/.test(k0)) return;   // "do\nSul/MS" partido na linha
    if (ws.length === 1 && (ws[0].length < 3 || (/^[A-ZÀ-Þ]+$/.test(ws[0]) && !/[AEIOUÀ-Ü]/.test(ws[0])))) return;
    if (/^X+$/i.test(ws.join('')) || /^MATO GROSSO(?: DO SUL)?$/.test(chave(ws.join(' ')))) return;   // o campo em branco ("XXX") e o estado
    por('MUNICIPIO', ws.join(' '));
  };
  cada(RE_MUN_CABECA, (x) => municipio(aparar(NOME6(t, x.index + x[0].length))));
  cada(RE_MUN_UF, (x) => municipio(aparar(x[1] || x[2], { direita: true })));
  // o número do processo: os quatro últimos dígitos são o código da comarca, e o número inteiro abre a consulta pública
  cada(RE_CNJ, (x) => { if (x[0].replace(/\D/g, '').length === 20) por('PROCESSO', x[0]); });

  cada(RE_CARGO, (x) => por('CARGO', x[0].toLowerCase()));

  const semPapel = (ws) => { while (ws.length && (PAPEIS.has(chave(ws[0])) || /^(?:A|O|AS|OS|DA|DO|DE|NA|NO|EM|PELA|PELO|QUE|COM|E|CONTRA|EMPRESA|FIRMA)$/i.test(ws[0]))) ws.shift(); return ws; };
  cada(RE_PJ_SUFIXO, (x) => {
    let ws = x[1].trim().split(/ +/);
    const u = ws.map(ehComum).lastIndexOf(true);   // "… QUE A AGRO FICTÍCIA LTDA": o nome começa depois da última palavra comum
    if (u >= 0) ws = ws.slice(u + 1);
    ws = semPapel(ws);
    if (ws.length) por('EMPRESA', ws.join(' ') + ' ' + x[2].trim());
  });
  cada(RE_PJ_CABECA, (x) => {
    const ws = semPapel(aparar(NOMENUM6(t, x.index + x[0].length).replace(/^["“]/, '')));
    if (ws.length && !GENERICOS.has(chave(ws[0])) && !PARA.has(chave(ws[0]))) por('EMPRESA', ws.join(' '));
  });
  const comCabeca = (tipo) => (x) => {
    const ws = x[1].trim().split(/ +/), nCab = /^(?:auto posto|projeto de assentamento|escola (?:estadual|municipal))\b/i.test(x[1]) ? (/^projeto/i.test(x[1]) ? 3 : 2) : 1;
    let resto = ws.slice(nCab);
    const k = resto.findIndex((w, n) => n > 0 && (GENERICOS.has(chave(w)) || PARA.has(chave(w)) || ehComum(w)) && !/^(?:d[aeo]s?|e)$/i.test(w));
    if (k > 0) resto = resto.slice(0, k);
    while (resto.length && /^(?:d[aeo]s?|e)$/i.test(resto[resto.length - 1])) resto.pop();
    const sig = resto.filter((w) => !/^(?:d[aeo]s?|e)$/i.test(w));
    if (!sig.length || GENERICOS.has(chave(sig[0])) || PARA.has(chave(sig[0])) || PAPEIS.has(chave(sig[0]))) return;
    por(tipo, ws.slice(0, nCab).concat(resto).join(' '));
  };
  cada(RE_LOCAL, comCabeca('EMPRESA'));
  cada(RE_TERRA, comCabeca('FAZENDA'));

  cada(RE_APELIDO_ASPAS, (x) => { if (!PAPEIS.has(chave(x[1]))) por('APELIDO', x[1]); });
  cada(RE_APELIDO_SOLTO, (x) => { const ws = aparar(NOME3(t, x.index + x[0].length)); if (ws.length && !PAPEIS.has(chave(ws[0])) && !PARA.has(chave(ws[0]))) por('APELIDO', ws.join(' ')); });

  return [...out.values()].sort((a, b) => b.valor.length - a.valor.length);
}

/** Põe no mapa do caso o que `achar` encontrou. `recusar(valor)`: o que quem usa ensinou que não é dado pessoal.
 *  Devolve os tokens novos: [{ token, tipo, valor }]. */
function registrar(mapa, achados, recusar) {
  const tokens = mapa.tokens || (mapa.tokens = {}), tem = new Set(), novos = [], n = {};
  for (const t of Object.keys(tokens)) for (const v of [tokens[t].valor].concat(tokens[t].variantes || [])) tem.add(chave(v));
  for (const a of achados) {
    const k = chave(a.valor);
    if (tem.has(k) || (recusar && recusar(a.valor))) continue;
    let token;
    do { n[a.tipo] = (n[a.tipo] || 0) + 1; token = '«' + a.tipo + '_' + String(n[a.tipo]).padStart(2, '0') + '»'; } while (tokens[token]);
    tokens[token] = { valor: a.valor, tipo: a.tipo };
    if (a.variantes && a.variantes.length) tokens[token].variantes = a.variantes.slice();
    tem.add(k);
    novos.push({ token, tipo: a.tipo, valor: a.valor, origem: a.origem });
  }
  return novos;
}

// ── 1b. AS AUTORIDADES (19/11/2026, pedido do usuário: "tira o nome do juiz, promotor, delegado e adv também; tira
// tudo, mais prático; juiz1, promotor1, etc.") ──────────────────────────────────────────────────────────────────────
// O motor deixa em claro o nome de autoridade, servidor e advogado e anota a chave de cada um em `mapa.emClaro` (as
// palavras do nome, sem acento, sem "de/da"). Aqui cada chave é procurada no texto, o papel sai do que vem antes ou
// depois do nome ("Juiz de Direito", "OAB", "Delegada de Polícia") e o nome vira um token do papel: «JUIZ_01»,
// «PROMOTOR_01», «DELEGADO_01», «ADVOGADO_01», «DEFENSOR_01», «POLICIAL_01», «PERITO_01», «SERVIDOR_01» (o resto).
// Fica em claro o ministro, o desembargador e o relator: é nome de julgado citado (decisão de 09/11/2026).
const PAPEIS_AUT = [
  ['FORA', 'ministr[oa]|min\\.|desembargador[a]?|des[a]?\\.|relator[a]?|rel\\.'],
  ['JUIZ', 'ju[ií]z[a]?|magistrad[oa]|MM\\.?'],
  ['PROMOTOR', 'promotor[a]?|procurador[a]?\\s+de\\s+justi[çc]a'],
  ['DELEGADO', 'delegad[oa]'],
  ['DEFENSOR', 'defensor[a]?'],
  ['ADVOGADO', 'advogad[oa]|OAB|adv\\.'],
  ['PERITO', 'perit[oa]|m[ée]dic[oa]\\s+legista'],
  ['POLICIAL', 'policia(?:l|is)|investigador[a]?|(?:SD|CB|SGT|SUBTEN|TEN|CAP|MAJ|CEL)\\.?\\s*(?:PM|BM)|PM|soldado|sargento|cabo|tenente|capit[ãa]o|major|coronel'],
].map(([tipo, alt]) => [tipo, new RegExp('(?<![' + LET + '])(?:' + alt + ')(?![' + LET + '])', 'gi')]);

/** O texto sem acento e em maiúsculas, com uma letra por letra (as posições valem no original). */
const dobra = (cru) => cru.normalize('NFC').split('').map((c) => c.normalize('NFD').replace(/[\u0300-\u036f]/g, '').toUpperCase().charAt(0) || c).join('');

/** O papel de quem tem o nome entre `i` e `f`: o cargo mais perto, antes (na mesma linha) ou depois (até a linha de baixo). */
function papelDe(t, i, f) {
  // só a frase do nome: o cargo da frase vizinha é de outra pessoa ("Rel.", "Des.", "adv." não fecham frase)
  const antes = t.slice(Math.max(0, i - 60), i).split('\n').pop().split(/[a-zà-ÿ]{4,}[.;!?]\s/).pop();
  const depois = t.slice(f, f + 110).split('\n').slice(0, 2).join('\n').split(/[.;!?](?:\s|$)/)[0];
  let melhor = null, dist = 1e9;
  for (const [tipo, re] of PAPEIS_AUT) {
    let m;
    re.lastIndex = 0;
    while ((m = re.exec(antes))) { const d = antes.length - (m.index + m[0].length); if (d < dist) { dist = d; melhor = tipo; } }
    re.lastIndex = 0;
    if ((m = re.exec(depois)) && m.index < dist) { dist = m.index; melhor = tipo; }
  }
  return melhor;
}

/** Os nomes que o motor deixou em claro, cada um com o seu papel: [{ tipo, valor, variantes, origem }] (`origem`: a
 *  chave do motor). O que só aparece como julgador de precedente (ministro, desembargador, relator) não entra. */
function autoridades(texto, mapa) {
  const t = String(texto || '').normalize('NFC'), chaves = ((mapa && mapa.emClaro) || []).concat((mapa && mapa.autoridadesIa) || []), out = [];
  if (!t || !chaves.length) return out;
  const dob = dobra(t), vistas = new Set();
  for (const k of chaves) {
    const ps = String(k || '').toUpperCase().split(' ').filter(Boolean);
    if (ps.length < 2 || vistas.has(k)) continue;
    vistas.add(k);
    const re = new RegExp('(?<![A-Z0-9«_])' + ps.map((p) => p.replace(/[^A-Z0-9]/g, '')).join('\\s+(?:(?:D[AEO]S?|E|DI|DEL|VON|VAN)\\s+)?') + '(?![A-Z0-9»_])', 'g');
    const formas = new Map(), votos = {};
    let m;
    while ((m = re.exec(dob))) {
      const forma = t.slice(m.index, m.index + m[0].length).replace(/\s+/g, ' '), kf = chave(forma);
      if (!formas.has(kf) || (forma !== forma.toUpperCase() && formas.get(kf) === formas.get(kf).toUpperCase())) formas.set(kf, forma);
      const p = papelDe(t, m.index, m.index + m[0].length) || 'SERVIDOR';
      votos[p] = (votos[p] || 0) + 1;
    }
    if (!formas.size) continue;
    const ordem = Object.keys(votos).filter((p) => p !== 'SERVIDOR').sort((a, b) => votos[b] - votos[a]);
    const tipo = ordem[0] || 'SERVIDOR';
    if (tipo === 'FORA') continue;
    const lista = [...formas.values()].sort((a, b) => (a === a.toUpperCase()) - (b === b.toUpperCase()) || b.length - a.length);
    out.push({ tipo, valor: lista[0], variantes: lista.slice(1), origem: k });
  }
  return out;
}

// ── 2. ALERTAR ────────────────────────────────────────────────────────────────────────────────────────────────────

const L = '[^\\s,.;:()«»]+';
const LT = '[^\\s,.;:()]+';   // também os tokens ("o único «CARGO_01» do município")
const PROFISSOES = 'm[ée]dic[oa]|dentista|advogad[oa]|professor(?:a)?|enfermeir[oa]|farmac[êe]utic[oa]|veterin[áa]ri[oa]|contador(?:a)?|engenheir[oa]|agr[ôo]nom[oa]|' +
  'radialista|jornalista|locutor(?:a)?|empres[áa]ri[oa]|fazendeir[oa]|pecuarista|comerciante|taxista|mototaxista|barbeir[oa]|cabeleireir[oa]|delegad[oa]|' +
  'escriv[ãa]o?|investigador(?:a)?|oficial de justi[çc]a|«CARGO_\\d+»';
const NEGOCIOS = 'bar|mercado|supermercado|mercearia|posto|loja|fazenda|s[ií]tio|ch[áa]cara|empresa|oficina|padaria|farm[áa]cia|lanchonete|restaurante|hotel|pousada|' +
  'com[ée]rcio|estabelecimento|borracharia|madeireira|garimpo|frigor[ií]fico|transportadora|«EMPRESA_\\d+»|«FAZENDA_\\d+»';
const REGRAS = [
  { id: 'unico', titulo: 'Alguém descrito como único', porque: 'dizer que a pessoa é "a única" em algo aponta para ela, mesmo sem o nome',
    re: new RegExp('(?<![' + LET + '])(?:[oa]s?|d[oa]s?|n[oa]s?|pel[oa]s?|é|era|como|sendo)\\s+únic[oa]s?\\s+' + LT + '(?:\\s+' + LT + '){0,5}', 'gi') },
  { id: 'porte', titulo: 'Tamanho do lugar', porque: 'num lugar pequeno, pouca informação já basta para saber de quem se fala',
    re: /(?<!\d)\d[\d.]*\s*(?:mil\s+)?habitantes\b|(?<![A-Za-zÀ-ÿ])(?:cidade|munic[ií]pio|comarca|vila|distrito|comunidade)\s+(?:pequen[oa]|de pequeno porte|do interior)(?![A-Za-zÀ-ÿ])|(?<![A-Za-zÀ-ÿ])pequen[oa]\s+(?:cidade|munic[ií]pio|comunidade|vila)(?![A-Za-zÀ-ÿ])/gi },
  { id: 'parente', titulo: 'Parentesco com alguém conhecido', porque: 'o parente de uma autoridade ou de um comerciante é fácil de achar',
    re: new RegExp('(?<![' + LET + '])(?:filh[oa]|espos[oa]|marido|mulher|companheir[oa]|irm[ãa]o?|sobrinh[oa]|net[oa]|genro|nora|cunhad[oa]|sogr[oa]|pai|mãe|ti[oa]|prim[oa]|ex-(?:marido|mulher|espos[oa]|companheir[oa]))' +
      '\\s+d[oa]s?\\s+(?:«CARGO_\\d+»|prefeit[oa]|vereador(?:a)?|ju[ií]z(?:a)?|promotor(?:a)?|delegad[oa]|deputad[oa]|secret[áa]ri[oa]|pastor|padre|m[ée]dic[oa]|don[oa]|propriet[áa]ri[oa])(?:\\s+' + L + '){0,4}', 'gi') },
  { id: 'dono', titulo: 'Dono de um negócio', porque: 'o dono de um comércio ou de uma fazenda do lugar é conhecido de todos',
    re: new RegExp('(?<![' + LET + '])(?:don[oa]|propriet[áa]ri[oa]|s[óo]ci[oa](?:-propriet[áa]ri[oa])?|arrendat[áa]ri[oa])\\s+(?:d[aeo]s?|de (?:um|uma))\\s+(?:' + NEGOCIOS + ')(?:\\s+' + L + '){0,4}', 'gi') },
  { id: 'profissao', titulo: 'Profissão ou cargo ligado ao lugar', porque: 'em cidade pequena pode haver um só',
    re: new RegExp('(?<![' + LET + '«_])(?:' + PROFISSOES + ')(?:\\s+' + L + '){0,3}?\\s+(?:d[aeo]|em|n[oa])\\s+(?:munic[ií]pio de |cidade de |comarca de )?(?:«MUNICIPIO_\\d+»|cidade|munic[ií]pio|regi[ãa]o|localidade|comunidade)(?![' + LET + '])', 'gi') },
  { id: 'lugar', titulo: 'Lugar em claro', porque: 'o bairro, o distrito ou a rodovia ajudam a descobrir a cidade',
    re: new RegExp('(?<![' + LET + '])(?:bairro|distrito|povoado|vila|comunidade|assentamento|gleba|rodovia|estrada)\\s+(?:d[aeo]s?\\s+)?[' + MAI + ']' + L + '(?:\\s+(?:d[aeo]s?\\s+)?[' + MAI + '0-9]' + L + '){0,3}|(?<![' + LET + '])(?:BR|MT|GO|MS|PA|RO|TO)[- ]?\\d{3}(?!\\d)', 'g') },
  { id: 'processo', titulo: 'Número do processo em claro', porque: 'com ele qualquer pessoa consulta o processo e vê as partes; os quatro últimos dígitos dizem a comarca',
    re: RE_CNJ },
];

/** O que ainda pode identificar alguém num texto já anonimizado. [{ regra, titulo, porque, trecho, antes, depois, i }].
 *  `mapa` (opcional): para avisar das autoridades que o motor deixa com o nome em claro. No máximo `max` por regra. */
function alertas(texto, mapa, max) {
  const t = String(texto || ''), out = [], lim = max || 12;
  for (const r of REGRAS) {
    const vistos = new Set();
    let m, n = 0;
    r.re.lastIndex = 0;
    while ((m = r.re.exec(t)) && n < lim) {
      if (!m[0]) { r.re.lastIndex++; continue; }
      if (r.id === 'processo' && m[0].replace(/\D/g, '').length !== 20) continue;
      const trecho = m[0].replace(/\s+/g, ' ').trim(), k = chave(trecho);
      if (vistos.has(k)) continue;
      vistos.add(k); n++;
      out.push({ regra: r.id, titulo: r.titulo, porque: r.porque, trecho, i: m.index,
        antes: t.slice(Math.max(0, m.index - 60), m.index).replace(/\s+/g, ' '), depois: t.slice(m.index + m[0].length, m.index + m[0].length + 60).replace(/\s+/g, ' ') });
    }
  }
  const claros = Math.max(0, (mapa && Array.isArray(mapa.emClaro) ? mapa.emClaro.length : 0) - (mapa && Array.isArray(mapa.emClaroTarjados) ? mapa.emClaroTarjados.length : 0));
  if (claros) out.push({ regra: 'autoridade', titulo: 'Autoridades com o nome em claro', porque: 'o nome do juiz, do promotor, do delegado ou do advogado permite descobrir a comarca',
    trecho: claros + (claros === 1 ? ' nome ficou' : ' nomes ficaram') + ' em claro (autoridade, servidor ou advogado: julgador de precedente citado, ou nome que você ensinou a deixar)', antes: '', depois: '', i: -1 });
  return out;
}

const api = { achar, registrar, autoridades, alertas, TIPOS, chave };
if (typeof module !== 'undefined' && module.exports) module.exports = api;
else raiz.Contexto = api;
})(typeof window !== 'undefined' ? window : globalThis);
