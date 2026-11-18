// Os comandos do plugin (`plugin:anonimizacao|<comando>`), pelo contrato de módulo do Graphé Desktop (MODULOS.md).
// Para cada um o Tauri gera a permissão `allow-<comando com hífens>`; os conjuntos `default` (a janela do Incognitus) e
// `api` (o que o hospedeiro chama da janela principal dele) estão em permissions/default.toml.
const COMANDOS: &[&str] = &[
    // geral
    "info", "info_app", "registrar", "revelar", "ajustes_ler", "ajustes_gravar", "acordado",
    // a fila: a cópia temporária do PDF, o pdftotext e o OCR de página; o texto de um .txt ou .docx
    "preparar_arquivo", "extrair_texto", "ocr_pagina", "limpar_arquivo", "texto_documento",
    // a saída: o PDF anonimizado, o .txt e o mapa
    "gravar_saida", "pasta_padrao", "listar_pdfs", "expandir_caminhos",
    // reidentificação
    "ler_texto", "docx_partes", "gravar_reidentificado", "gravar_texto",
    // a minuta do 🧠 no editor: o .docx e a abertura dele no editor de texto do computador
    "gravar_docx", "abrir_documento", "abrir_modelo", "mesa_ler", "mesa_gravar",
    // o motor e a atualização assinada dele
    "motor_ativo", "motor_checar", "motor_baixar", "motor_instalar", "motor_voltar",
    // o modelo da anonimização rigorosa (opcional)
    "ner_estado", "ner_baixar", "ner_ler",
    // a busca nos modelos da pasta: o BERT de frases, a lista da pasta e o índice
    "busca_estado", "busca_baixar", "busca_ler", "modelos_listar", "indice_ler", "indice_gravar",
    // teste de ponta a ponta (só no app avulso, com SKIA_E2E)
    "e2e_plano", "e2e_gravar", "e2e_fim",
    // o 🧠 Melhor Próximo Ato: a chave da Anthropic, o login do Google, o prompt e o envio
    "ia_situacao", "ia_chave_guardar", "ia_chave_apagar", "ia_google_entrar", "ia_google_cancelar", "ia_google_sair",
    "ia_prompt_ler", "ia_enviar", "ia_modelos", "ia_pasta_listar", "ia_gdocs_enviar", "abrir_feedback",
    // o 👁: o PDF numa janela do próprio app
    "visor_abrir",
];

fn main() {
    tauri_plugin::Builder::new(COMANDOS).build();
}
