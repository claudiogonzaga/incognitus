// Σκιά · Skiá — o worker do motor. O app monta este worker de um Blob com três partes: o motor.js (o embutido ou o
// baixado e conferido), o casos.js e este arquivo. O motor roda aqui, fora da linha da interface: autos de centenas de
// páginas não travam a janela. Cada pedido: { id, tipo, ... } → { id, ok, r } ou { id, ok: false, erro }.
self.onmessage = function (ev) {
  var m = ev.data || {}, r;
  try {
    switch (m.tipo) {
      case 'versao':
        r = { versao: SkiaMotor.versao, api: SkiaMotor.api };
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
        r = SkiaMotor.anonimizar(m.texto, { meta: m.meta, incluirMapa: !!m.comMapa });
        if (m.comMapa) r.mapaJson = JSON.stringify(r.mapa, null, 1);
        delete r.mapa;
        delete r.corpo;
        if (!m.comTexto) delete r.texto;
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
