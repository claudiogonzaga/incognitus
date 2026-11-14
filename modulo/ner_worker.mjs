// Incognitus — um leitor da anonimização rigorosa: roda o modelo de NER (onnxruntime-web, WebAssembly, 1 thread) sobre
// as páginas que a janela manda. A janela abre alguns destes em paralelo. Pedidos: { id, tipo: 'iniciar', modelo,
// tokenizer, rotulos } e { id, tipo: 'reconhecer', texto } → { id, ok, r } ou { id, ok: false, erro }.
import * as ort from './vendor/onnxruntime-web/ort.wasm.bundle.min.mjs';
import './ner.js';

let reconhecedor = null;
const passo = (id, t) => self.postMessage({ id, passo: t });

self.onmessage = async (ev) => {
  const m = ev.data || {};
  try {
    let r;
    if (m.tipo === 'iniciar') {
      ort.env.wasm.numThreads = self.crossOriginIsolated ? Math.min(4, navigator.hardwareConcurrency || 2) : 1;
      ort.env.wasm.proxy = false;
      // o .wasm vem pelo próprio app (sem depender do tipo MIME que o WebView dá ao arquivo)
      const w = await fetch(new URL('./vendor/onnxruntime-web/ort-wasm-simd-threaded.wasm', import.meta.url));
      if (!w.ok) throw new Error('o motor do modelo (wasm) não abriu: ' + w.status);
      ort.env.wasm.wasmBinary = await w.arrayBuffer();
      passo(m.id, 'wasm ' + ort.env.wasm.wasmBinary.byteLength);
      const rot = {};
      m.rotulos.forEach((x, i) => { rot[i] = x; });
      // sem a arena e o padrão de memória do onnxruntime: vários leitores dividem o mesmo processo do WebView, e ele cai
      // quando a memória passa do limite
      reconhecedor = await self.SkiaNer.criar(ort, new Uint8Array(m.modelo), m.tokenizer, rot,
        { graphOptimizationLevel: 'all', enableCpuMemArena: false, enableMemPattern: false });
      m.modelo = null;
      passo(m.id, 'sessão criada');
      r = { pronto: true, threads: ort.env.wasm.numThreads };
    } else if (m.tipo === 'reconhecer') {
      if (!reconhecedor) throw new Error('o leitor não foi iniciado');
      r = await reconhecedor.reconhecer(String(m.texto || ''));
    } else {
      throw new Error('pedido desconhecido: ' + m.tipo);
    }
    self.postMessage({ id: m.id, ok: true, r });
  } catch (e) {
    self.postMessage({ id: m.id, ok: false, erro: String((e && e.message) || e) });
  }
};
