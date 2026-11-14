//! O modelo da anonimização RIGOROSA (opcional): um BERT de reconhecimento de entidades em português, baixado na primeira
//! vez que o usuário liga a opção — não vai no instalador. Fica na pasta de dados do app, conferido por sha256; quem roda
//! o modelo é a janela (onnxruntime-web, num Web Worker), que lê os arquivos por `ner_ler`.
//!
//! Modelo: inples-tech/anonimato-ner-ptbr (Apache-2.0; rótulos PESSOA, ORG, LOCAL, ENDERECO; 110 MB, int8), numa
//! revisão fixa. A rede do MPMT derruba conexões de vez em quando: o download tenta de novo e retoma de onde parou.
use serde::Serialize;
use sha2::{Digest, Sha256};
use std::io::Write;
use std::path::{Path, PathBuf};
use std::time::Duration;

pub const NOME: &str = "anonimato-ner-ptbr";
const REPO: &str = "inples-tech/anonimato-ner-ptbr";
const REVISAO: &str = "e55b0a26470a8525387ad9172e1dab24c390e80b";
/// (arquivo, sha256, tamanho em bytes)
pub const ARQUIVOS: &[(&str, &str, u64)] = &[
    ("model.onnx", "c5d83274c636d3f589457d8af6c4966cfb583e82589cf90843a4281bfb4bb4fd", 109_515_171),
    ("tokenizer.json", "bcc8fe4209dffce14189a4319759f834e84c7df0a0bee8bfc6b576d4e2c6d387", 677_790),
];
/// O id2label do config.json do modelo (fixo com a revisão).
pub const ROTULOS: &[&str] = &["O", "B-PESSOA", "I-PESSOA", "B-ORG", "I-ORG", "B-LOCAL", "I-LOCAL", "B-ENDERECO", "I-ENDERECO"];

#[derive(Serialize)]
pub struct EstadoNer {
    pub baixado: bool,
    pub nome: &'static str,
    pub megas: u64,
    pub rotulos: Vec<&'static str>,
}

/// A pasta do modelo: SKIA_NER_DIR (testes) ou <dados>/modelos/<nome>.
pub fn pasta(dados: &Path) -> PathBuf {
    match std::env::var_os("SKIA_NER_DIR") {
        Some(d) => PathBuf::from(d),
        None => dados.join("modelos").join(NOME),
    }
}

fn sha256_arquivo(p: &Path) -> Option<String> {
    let mut f = std::fs::File::open(p).ok()?;
    let mut h = Sha256::new();
    std::io::copy(&mut f, &mut h).ok()?;
    Some(format!("{:x}", h.finalize()))
}

fn arquivo_ok(dir: &Path, nome: &str, sha: &str, tam: u64) -> bool {
    let p = dir.join(nome);
    std::fs::metadata(&p).map(|m| m.len() == tam).unwrap_or(false) && sha256_arquivo(&p).as_deref() == Some(sha)
}

pub fn estado(dados: &Path) -> EstadoNer {
    let dir = pasta(dados);
    EstadoNer {
        baixado: ARQUIVOS.iter().all(|(n, s, t)| arquivo_ok(&dir, n, s, *t)),
        nome: NOME,
        megas: ARQUIVOS.iter().map(|a| a.2).sum::<u64>() / 1_000_000,
        rotulos: ROTULOS.to_vec(),
    }
}

/// Baixa o que falta (com novas tentativas e retomada), confere o sha256 e só então dá o nome final.
pub async fn baixar(dados: &Path) -> Result<EstadoNer, String> {
    let dir = pasta(dados);
    std::fs::create_dir_all(&dir).map_err(|e| format!("não consegui criar {}: {e}", dir.display()))?;
    let cli = reqwest::Client::builder()
        .user_agent(concat!("Incognitus/", env!("CARGO_PKG_VERSION")))
        .connect_timeout(Duration::from_secs(30))
        .timeout(Duration::from_secs(1800))
        .build()
        .map_err(|e| e.to_string())?;
    for (nome, sha, tam) in ARQUIVOS {
        if arquivo_ok(&dir, nome, sha, *tam) {
            continue;
        }
        let url = format!("https://huggingface.co/{REPO}/resolve/{REVISAO}/{nome}");
        let parcial = dir.join(format!(".{nome}.parcial"));
        let mut erro = String::new();
        for tentativa in 0..8u64 {
            if tentativa > 0 {
                std::thread::sleep(Duration::from_millis(1000 * tentativa));
            }
            let ja = std::fs::metadata(&parcial).map(|m| m.len()).unwrap_or(0);
            if ja >= *tam {
                break;
            }
            let mut req = cli.get(&url);
            if ja > 0 {
                req = req.header("Range", format!("bytes={ja}-"));
            }
            let mut r = match req.send().await {
                Ok(r) => r,
                Err(e) => {
                    erro = format!("sem conexão com o Hugging Face ({e})");
                    continue;
                }
            };
            let st = r.status().as_u16();
            if st != 200 && st != 206 {
                erro = format!("o Hugging Face respondeu {st}");
                continue;
            }
            let anexar = st == 206 && ja > 0;
            let mut f = std::fs::OpenOptions::new()
                .create(true)
                .write(true)
                .append(anexar)
                .truncate(!anexar)
                .open(&parcial)
                .map_err(|e| format!("não consegui gravar o modelo: {e}"))?;
            loop {
                match r.chunk().await {
                    Ok(Some(c)) => f.write_all(&c).map_err(|e| format!("não consegui gravar o modelo: {e}"))?,
                    Ok(None) => break,
                    Err(e) => {
                        erro = format!("o download parou: {e}");
                        break;
                    }
                }
            }
        }
        if std::fs::metadata(&parcial).map(|m| m.len()).unwrap_or(0) != *tam {
            return Err(format!("não consegui baixar o modelo ({nome}): {erro}. Tente de novo; ele retoma de onde parou."));
        }
        if sha256_arquivo(&parcial).as_deref() != Some(*sha) {
            let _ = std::fs::remove_file(&parcial);
            return Err(format!("o arquivo {nome} do modelo veio diferente do esperado (sha256): apaguei; tente de novo"));
        }
        std::fs::rename(&parcial, dir.join(nome)).map_err(|e| e.to_string())?;
    }
    Ok(estado(dados))
}

/// Os bytes de um arquivo do modelo (só os da lista).
pub fn ler(dados: &Path, nome: &str) -> Result<Vec<u8>, String> {
    if !ARQUIVOS.iter().any(|a| a.0 == nome) {
        return Err(format!("arquivo desconhecido: {nome}"));
    }
    std::fs::read(pasta(dados).join(nome)).map_err(|e| format!("o modelo não está baixado ({nome}): {e}"))
}

#[cfg(test)]
mod testes {
    use super::*;

    #[test]
    fn so_le_arquivos_da_lista() {
        assert!(ler(Path::new("/tmp"), "../../etc/passwd").is_err());
        assert!(ler(Path::new("/tmp"), "config.json").is_err());
    }

    #[test]
    fn rotulos_com_pessoa_e_endereco() {
        assert_eq!(ROTULOS[1], "B-PESSOA");
        assert_eq!(ROTULOS[8], "I-ENDERECO");
    }
}
