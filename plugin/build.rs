// Os comandos do plugin (`plugin:anonimizacao|<comando>`), pelo contrato de módulo do Graphé Desktop (MODULOS.md).
// Para cada um o Tauri gera a permissão `allow-<comando com hífens>`; os conjuntos `default` (a janela do Incognitus) e
// `api` (o que o hospedeiro chama da janela principal dele) estão em permissions/default.toml.
const COMANDOS: &[&str] = &[
    // geral
    "info", "info_app", "registrar", "revelar", "ajustes_ler", "ajustes_gravar",
    // a fila: a cópia temporária do PDF, o pdftotext e o OCR de página
    "preparar_arquivo", "extrair_texto", "ocr_pagina", "limpar_arquivo",
    // a saída: o PDF anonimizado, o .txt e o mapa
    "gravar_saida", "pasta_padrao", "listar_pdfs", "expandir_caminhos",
    // reidentificação
    "ler_texto", "docx_partes", "gravar_reidentificado", "gravar_texto",
    // o motor e a atualização assinada dele
    "motor_ativo", "motor_checar", "motor_baixar", "motor_instalar", "motor_voltar",
    // teste de ponta a ponta (só no app avulso, com SKIA_E2E)
    "e2e_plano", "e2e_gravar", "e2e_fim",
];

fn main() {
    tauri_plugin::Builder::new(COMANDOS).build();
}
