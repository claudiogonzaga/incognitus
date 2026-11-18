// Incognitus — o leitor da busca nos modelos da pasta: roda o BERT de frases (multilingual-e5-small, onnxruntime-web,
// WebAssembly) sobre os textos que a janela manda e devolve os vetores. Pedidos: { id, tipo: 'iniciar', modelo,
// tokenizer } e { id, tipo: 'vetores', textos: [..], qual: 'query' | 'passage' } → { id, ok, r: [Float32Array…] }.
import * as ort from './vendor/onnxruntime-web/ort.wasm.bundle.min.mjs';
import './busca.js';

let embutidor = null;

self.onmessage = async (ev) => {
  const m = ev.data || {};
  try {
    let r;
    if (m.tipo === 'iniciar') {
      ort.env.wasm.numThreads = self.crossOriginIsolated ? Math.min(4, navigator.hardwareConcurrency || 2) : 1;
      ort.env.wasm.proxy = false;
      const w = await fetch(new URL('./vendor/onnxruntime-web/ort-wasm-simd-threaded.wasm', import.meta.url));
      if (!w.ok) throw new Error('o motor do modelo (wasm) não abriu: ' + w.status);
      ort.env.wasm.wasmBinary = await w.arrayBuffer();
      embutidor = await self.SkiaBusca.criar(ort, new Uint8Array(m.modelo), m.tokenizer,
        { graphOptimizationLevel: 'all', enableCpuMemArena: false, enableMemPattern: false });
      m.modelo = null;
      r = { pronto: true };
    } else if (m.tipo === 'vetores') {
      if (!embutidor) throw new Error('o leitor da busca não foi iniciado');
      r = [];
      for (const t of m.textos || []) r.push(await embutidor.vetor(String(t || ''), m.qual));
    } else {
      throw new Error('pedido desconhecido: ' + m.tipo);
    }
    self.postMessage({ id: m.id, ok: true, r });
  } catch (e) {
    self.postMessage({ id: m.id, ok: false, erro: String((e && e.message) || e) });
  }
};
