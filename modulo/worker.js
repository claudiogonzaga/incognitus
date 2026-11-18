// Incognitus — o worker do motor. O app monta este worker de um Blob com três partes: o motor.js (o embutido ou o
// baixado e conferido), o casos.js e este arquivo. O motor roda aqui, fora da linha da interface: autos de centenas de
// páginas não travam a janela. Cada pedido: { id, tipo, ... } → { id, ok, r } ou { id, ok: false, erro }.
// 18/11/2026 (pedido do usuário: "ao invés de colocar os nomes em grego, a gente vai colocar o status processual dele…
// réu 01, réu 02, vítima 1"): o motor já sabe as duas formas — a propriedade GRAPHE_ANON_NOMES_GREGOS desligada dá
// «REU_01», «VITIMA_01», «TESTEMUNHA_01», «PES_01». Vale só nas anonimizações (a suíte de casos segue no padrão do motor).
function semGregos(m, f) {
  SKIA_AMBIENTE.propriedades.GRAPHE_ANON_NOMES_GREGOS = m.gregos ? '1' : '0';
  try { return f(); } finally { delete SKIA_AMBIENTE.propriedades.GRAPHE_ANON_NOMES_GREGOS; }
}
self.onmessage = function (ev) {
  var m = ev.data || {}, r;
  try {
    switch (m.tipo) {
      case 'versao':
        r = { versao: SkiaMotor.versao, api: SkiaMotor.api, caso: typeof SkiaMotor.anonimizarCaso === 'function' };
        break;
      case 'configurar':
        r = SkiaMotor.configurar(m.cfg);
        break;
      case 'ler':
        r = SkiaMotor.lerPdftotext(m.saida);
        break;
      case 'ocr':
        r = SkiaMotor.aplicarOcr(m.texto, m.fracas, m.lidos);
        break;
      case 'anonimizar':
        r = semGregos(m, function () { return SkiaMotor.anonimizar(m.texto, { meta: m.meta, incluirMapa: true, extras: m.extras, rigoroso: !!m.rigoroso }); });
        r.mapaJson = JSON.stringify(r.mapa, null, 1);
        delete r.mapa;
        delete r.corpo;
        if (!m.comTexto) delete r.texto;
        break;
      case 'caso':
        // vários documentos do mesmo caso (ou um só): a saída repartida, um mapa, a assinatura para juntar
        r = semGregos(m, function () { return SkiaMotor.anonimizarCaso(m.docs, { incluirMapa: true, extras: m.extras, rigoroso: !!m.rigoroso }); });
        r.mapaJson = JSON.stringify(r.mapa, null, 1);
        delete r.mapa;
        if (!m.comTexto) r.documentos.forEach(function (d) { delete d.texto; });
        break;
      case 'reidentificar':
        r = SkiaMotor.reidentificar(m.texto, m.mapa, m.titulo);
        break;
      case 'trechos':
        r = SkiaMotor.trechos(m.textos, m.mapa);
        break;
      case 'sobras':
        r = SkiaMotor.sobras(m.texto, m.titulo, m.mapa);
        break;
      case 'aplicarMapa':
        // os nomes e os identificadores do mapa do caso que aparecerem num texto de quem usa (o prompt, o Guia, o RPP, um
        // modelo do Livro) viram os tokens do caso, como o Graphé faz com o RPP (as funções são as do próprio motor)
        if (typeof _anonAplicarNomes_ !== 'function' || typeof _anonAplicarIdentificadores_ !== 'function') { r = { texto: m.texto, aplicado: false }; break; }
        var mp = typeof m.mapa === 'string' ? JSON.parse(m.mapa) : m.mapa;
        // só os NOMES do caso (como o Graphé faz com o RPP): os identificadores da casa (conta do fundo, CNPJ) ficam
        r = { texto: _anonAplicarNomes_(String(m.texto || ''), mp), aplicado: true };
        break;
      case 'dicionario':
        r = SkiaMotor.dicionario(m.mapa);
        break;
      case 'testar':
        r = SkiaCasos.rodar(SkiaMotor, m.pacote);
        break;
      default:
        throw new Error('pedido desconhecido: ' + m.tipo);
    }
    self.postMessage({ id: m.id, ok: true, r: r });
  } catch (e) {
    self.postMessage({ id: m.id, ok: false, erro: String((e && e.message) || e) });
  }
};
