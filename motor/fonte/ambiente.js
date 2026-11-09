// ── Ambiente do Incognitus ────────────────────────────────────────────────────────────────────────────────────────────────
// O que no Graphé vem da planilha e do Apps Script vem daqui, sem rede e sem disco:
//   · VARIAVEIS_GLOBAIS: vazia — os contatos institucionais são o ANON_INST_PADRAO, que os Ajustes do Incognitus trocam
//     (SkiaMotor.configurar, no fim deste arquivo);
//   · equipe: vazia;
//   · propriedades do script: nenhuma (sem lista branca aprendida, nomes gregos ligados — o padrão do Graphé);
//   · Logger não grava nada; Utilities só formata datas (no fuso que o motor pedir).
var SKIA_AMBIENTE = { propriedades: {}, equipe: [] };
var VariaveisGlobais = {};
function getScriptProperties_() {
  return { getProperty: function (k) { var v = SKIA_AMBIENTE.propriedades[k]; return v == null ? null : String(v); } };
}
function _lerDadosEquipe_() { return (SKIA_AMBIENTE.equipe || []).slice(); }
var Logger = { log: function () {} };
var Utilities = {
  formatDate: function (d, tz, fmt) {
    var p = {};
    try {
      new Intl.DateTimeFormat('en-GB', { timeZone: tz, year: 'numeric', month: '2-digit', day: '2-digit', hour: '2-digit',
        minute: '2-digit', second: '2-digit', hourCycle: 'h23' }).formatToParts(d).forEach(function (x) { p[x.type] = x.value; });
    } catch (e) {
      var z = function (n) { return (n < 10 ? '0' : '') + n; };
      p = { year: String(d.getFullYear()), month: z(d.getMonth() + 1), day: z(d.getDate()), hour: z(d.getHours()),
            minute: z(d.getMinutes()), second: z(d.getSeconds()) };
    }
    return String(fmt).replace(/yyyy|MM|M|dd|d|HH|H|mm|ss/g, function (t) {
      switch (t) {
        case 'yyyy': return p.year;
        case 'MM': return p.month;
        case 'M': return String(parseInt(p.month, 10));
        case 'dd': return p.day;
        case 'd': return String(parseInt(p.day, 10));
        case 'HH': return p.hour;
        case 'H': return String(parseInt(p.hour, 10));
        case 'mm': return p.minute;
        default: return p.second;
      }
    });
  }
};
