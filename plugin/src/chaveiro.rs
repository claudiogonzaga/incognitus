//! O chaveiro do sistema: o Keychain no Mac e o Gerenciador de Credenciais no Windows. Guarda a chave da Anthropic de
//! quem usa o app e a renovação do login do Google (google.rs). Nada disso vai a arquivo, a log ou ao JavaScript da
//! janela: quem lê é só o Rust, na hora de chamar a API. (Vem do Graphé Desktop, src-tauri/src/chaveiro.rs.)
//! O serviço é o identificador do app avulso, com ".modulo" no fim quando hospedado (MODULOS.md do Graphé Desktop).

pub const CONTA_ANTHROPIC: &str = "anthropic";

fn entrada(servico: &str, conta: &str) -> Result<keyring::Entry, String> {
    keyring::Entry::new(servico, conta).map_err(|e| format!("não consegui abrir o chaveiro do sistema: {e}"))
}

pub fn guardar(servico: &str, conta: &str, valor: &str) -> Result<(), String> {
    entrada(servico, conta)?.set_password(valor).map_err(|e| format!("não consegui guardar no chaveiro do sistema: {e}"))
}

/// O valor guardado, ou None se não há.
pub fn ler(servico: &str, conta: &str) -> Result<Option<String>, String> {
    match entrada(servico, conta)?.get_password() {
        Ok(v) => Ok(Some(v)),
        Err(keyring::Error::NoEntry) => Ok(None),
        Err(e) => Err(format!("não consegui ler o chaveiro do sistema: {e}")),
    }
}

/// Apaga o valor; não ter o que apagar não é erro.
pub fn apagar(servico: &str, conta: &str) -> Result<(), String> {
    match entrada(servico, conta)?.delete_credential() {
        Ok(()) | Err(keyring::Error::NoEntry) => Ok(()),
        Err(e) => Err(format!("não consegui apagar do chaveiro do sistema: {e}")),
    }
}

/// A chave colada, limpa e conferida: começa por "sk-ant-" e não tem espaço no meio.
pub fn conferir_chave_anthropic(texto: &str) -> Result<String, String> {
    let chave = texto.trim();
    if !chave.starts_with("sk-ant-") || chave.len() < 24 || chave.chars().any(char::is_whitespace) {
        return Err("Isso não parece uma chave da Anthropic: ela começa com sk-ant- e não tem espaços. Copie de novo no Console.".into());
    }
    Ok(chave.to_string())
}

/// O fim da chave guardada ("…a1b2"), para quem usa saber qual é, sem mostrar a chave.
pub fn final_da_chave(chave: &str) -> String {
    let n = chave.chars().count();
    format!("…{}", chave.chars().skip(n.saturating_sub(4)).collect::<String>())
}

#[cfg(test)]
mod testes {
    use super::*;

    #[test]
    fn confere_a_chave_colada() {
        let boa = format!("  sk-ant-api03-{}\n", "x".repeat(40));
        assert_eq!(conferir_chave_anthropic(&boa).unwrap(), boa.trim());
        // As chaves de prova são montadas em pedaços, para a trava do repositório não as tomar por chaves de verdade.
        assert!(conferir_chave_anthropic(&["sk", "proj", "abcdefghijklmnopqrstuvwxyz"].join("-")).is_err());
        assert!(conferir_chave_anthropic("sk-ant-api03 com espaço no meio da chave").is_err());
        assert!(conferir_chave_anthropic("sk-ant-curta").is_err());
        assert_eq!(final_da_chave(&boa.trim()), "…xxxx");
    }

    /// O chaveiro de verdade, numa entrada de prova que é apagada no fim. Não roda sozinho (mexe no chaveiro de quem
    /// testa): cargo test -- --ignored chaveiro_de_verdade
    #[test]
    #[ignore]
    fn chaveiro_de_verdade() {
        let servico = "br.eco.consciencia.incognitus.prova";
        apagar(servico, "prova").unwrap();
        assert_eq!(ler(servico, "prova").unwrap(), None);
        guardar(servico, "prova", "valor de prova").unwrap();
        assert_eq!(ler(servico, "prova").unwrap().as_deref(), Some("valor de prova"));
        apagar(servico, "prova").unwrap();
        assert_eq!(ler(servico, "prova").unwrap(), None);
    }
}
