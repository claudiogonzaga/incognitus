//! O motor de anonimização em uso: o embutido no app (src/motor) ou um baixado do repositório público skia-motor.
//! Um motor baixado só entra se (1) a assinatura Ed25519 do manifesto confere com a chave pública embutida no app,
//! (2) o sha256 do motor.js é o do manifesto, (3) a API é a que este app fala e (4) os casos de teste embutidos passam
//! — o teste roda no worker do app, entre o `baixar` e o `instalar`. O anterior fica guardado para voltar.
//! Pastas (em dados do app): motor/atual, motor/anterior, motor/pendente.
use base64::Engine;
use ed25519_dalek::{Signature, VerifyingKey};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::path::{Path, PathBuf};

const CHAVE_PUBLICA: &str = include_str!("../../motor/chave_publica.txt");
const MANIFESTO_EMBUTIDO: &str = include_str!("../../src/motor/manifesto.json");
pub const URL_PADRAO: &str = "https://github.com/claudiogonzaga/skia-motor/releases/latest/download/";
pub const API: u32 = 2;

#[derive(Serialize, Deserialize, Clone, Debug)]
pub struct Manifesto {
    pub nome: String,
    pub versao: String,
    pub api: u32,
    pub data: String,
    pub sha256: String,
    #[serde(flatten)]
    pub resto: serde_json::Map<String, serde_json::Value>,
}

#[derive(Serialize)]
pub struct MotorAtivo {
    pub origem: String,
    pub manifesto: Manifesto,
    /// O texto do motor baixado (o embutido o front lê de src/motor/motor.js).
    pub js: Option<String>,
    pub anterior: Option<Manifesto>,
}

#[derive(Serialize)]
pub struct Baixado {
    pub manifesto: Manifesto,
    pub js: String,
}

pub fn sha256_hex(b: &[u8]) -> String {
    Sha256::digest(b).iter().map(|x| format!("{x:02x}")).collect()
}

fn verificar_assinatura(manif: &[u8], sig_b64: &str) -> Result<(), String> {
    let b64 = base64::engine::general_purpose::STANDARD;
    let chave: [u8; 32] = b64.decode(CHAVE_PUBLICA.trim()).map_err(|_| "chave pública inválida")?.try_into().map_err(|_| "chave pública com tamanho errado")?;
    let sig: [u8; 64] = b64.decode(sig_b64.trim()).map_err(|_| "assinatura ilegível")?.try_into().map_err(|_| "assinatura com tamanho errado")?;
    let vk = VerifyingKey::from_bytes(&chave).map_err(|_| "chave pública inválida")?;
    vk.verify_strict(manif, &Signature::from_bytes(&sig)).map_err(|_| "a assinatura do manifesto NÃO confere".to_string())
}

/// Lê e confere uma pasta de motor (manifesto assinado + motor.js com o sha256 dele).
fn ler_pasta(dir: &Path) -> Result<(Manifesto, String), String> {
    let manif = std::fs::read(dir.join("manifesto.json")).map_err(|e| format!("manifesto: {e}"))?;
    let sig = std::fs::read_to_string(dir.join("manifesto.json.sig")).map_err(|e| format!("assinatura: {e}"))?;
    verificar_assinatura(&manif, &sig)?;
    let m: Manifesto = serde_json::from_slice(&manif).map_err(|e| format!("manifesto ilegível: {e}"))?;
    let js = std::fs::read(dir.join("motor.js")).map_err(|e| format!("motor.js: {e}"))?;
    if sha256_hex(&js) != m.sha256 {
        return Err("o sha256 do motor.js não é o do manifesto".into());
    }
    if m.api != API {
        return Err(format!("motor da API {} (este app fala a {API})", m.api));
    }
    Ok((m, String::from_utf8(js).map_err(|_| "motor.js não é UTF-8")?))
}

fn versao_num(v: &str) -> Vec<u64> {
    v.split(|c: char| !c.is_ascii_digit()).filter(|s| !s.is_empty()).map(|s| s.parse().unwrap_or(0)).collect()
}

/// `a` é mais novo que `b`? (versão; na mesma versão, a data — o mesmo ANON_VERSAO pode ser republicado)
fn mais_novo(a: &Manifesto, b: &Manifesto) -> bool {
    let (va, vb) = (versao_num(&a.versao), versao_num(&b.versao));
    if va != vb {
        return va > vb;
    }
    a.sha256 != b.sha256 && a.data > b.data
}

pub fn embutido() -> Manifesto {
    serde_json::from_str(MANIFESTO_EMBUTIDO).expect("manifesto embutido")
}

/// O motor em uso: o baixado (atual), se conferir e não for mais velho que o embutido; senão, o embutido.
pub fn ativo(dir: &Path) -> MotorAtivo {
    let emb = embutido();
    let anterior = ler_pasta(&dir.join("anterior")).ok().map(|(m, _)| m);
    match ler_pasta(&dir.join("atual")) {
        Ok((m, js)) if !mais_novo(&emb, &m) => MotorAtivo { origem: "baixado".into(), manifesto: m, js: Some(js), anterior },
        _ => MotorAtivo { origem: "embutido".into(), manifesto: emb, js: None, anterior },
    }
}

fn cliente() -> Result<reqwest::Client, String> {
    reqwest::Client::builder()
        .user_agent(concat!("Skia/", env!("CARGO_PKG_VERSION")))
        .timeout(std::time::Duration::from_secs(60))
        .build()
        .map_err(|e| e.to_string())
}

async fn buscar(c: &reqwest::Client, url: &str) -> Result<Vec<u8>, String> {
    let r = c.get(url).send().await.map_err(|e| format!("sem conexão com o repositório das regras ({e})"))?;
    if !r.status().is_success() {
        return Err(format!("o repositório das regras respondeu {}", r.status().as_u16()));
    }
    Ok(r.bytes().await.map_err(|e| e.to_string())?.to_vec())
}

/// O manifesto publicado, se for mais novo que o motor em uso, com assinatura conferida. `recusado`: o sha256 de um
/// motor de que o usuário voltou (a checagem automática não o oferece de novo).
pub async fn checar(base: &str, em_uso: &Manifesto, recusado: Option<String>) -> Result<Option<Manifesto>, String> {
    let c = cliente()?;
    let manif = buscar(&c, &format!("{base}manifesto.json")).await?;
    let sig = buscar(&c, &format!("{base}manifesto.json.sig")).await?;
    verificar_assinatura(&manif, &String::from_utf8_lossy(&sig))?;
    let m: Manifesto = serde_json::from_slice(&manif).map_err(|e| format!("manifesto ilegível: {e}"))?;
    if m.api != API {
        return Err(format!("as regras publicadas pedem um Skiá mais novo (API {})", m.api));
    }
    if Some(&m.sha256) == recusado.as_ref() || !mais_novo(&m, em_uso) {
        return Ok(None);
    }
    Ok(Some(m))
}

/// Baixa manifesto, assinatura e motor.js para motor/pendente e confere tudo. O teste dos casos é no front.
pub async fn baixar(base: &str, dir: &Path) -> Result<Baixado, String> {
    let c = cliente()?;
    let manif = buscar(&c, &format!("{base}manifesto.json")).await?;
    let sig = buscar(&c, &format!("{base}manifesto.json.sig")).await?;
    verificar_assinatura(&manif, &String::from_utf8_lossy(&sig))?;
    let m: Manifesto = serde_json::from_slice(&manif).map_err(|e| format!("manifesto ilegível: {e}"))?;
    let js = buscar(&c, &format!("{base}motor.js")).await?;
    if sha256_hex(&js) != m.sha256 {
        return Err("o motor.js baixado não tem o sha256 do manifesto (descartado)".into());
    }
    let pend = dir.join("pendente");
    let _ = std::fs::remove_dir_all(&pend);
    std::fs::create_dir_all(&pend).map_err(|e| e.to_string())?;
    std::fs::write(pend.join("manifesto.json"), &manif).map_err(|e| e.to_string())?;
    std::fs::write(pend.join("manifesto.json.sig"), &sig).map_err(|e| e.to_string())?;
    std::fs::write(pend.join("motor.js"), &js).map_err(|e| e.to_string())?;
    let (m2, js2) = ler_pasta(&pend)?;
    if m2.sha256 != m.sha256 {
        return Err("o motor gravado não é o baixado".into());
    }
    Ok(Baixado { manifesto: m2, js: js2 })
}

/// O pendente (já testado pelo front) vira o atual; o atual vira o anterior.
pub fn instalar(dir: &Path, sha256: &str) -> Result<MotorAtivo, String> {
    let pend = dir.join("pendente");
    let (m, _) = ler_pasta(&pend)?;
    if m.sha256 != sha256 {
        return Err("o motor pendente não é o que foi testado".into());
    }
    let (atual, anterior) = (dir.join("atual"), dir.join("anterior"));
    if atual.exists() {
        let _ = std::fs::remove_dir_all(&anterior);
        std::fs::rename(&atual, &anterior).map_err(|e| e.to_string())?;
    }
    std::fs::rename(&pend, &atual).map_err(|e| e.to_string())?;
    let _ = std::fs::remove_file(dir.join("recusado.txt"));
    Ok(ativo(dir))
}

/// Volta ao anterior (e o de agora vira o anterior, para poder ir e vir); sem anterior, volta ao embutido.
pub fn voltar(dir: &Path) -> Result<MotorAtivo, String> {
    let (atual, anterior, troca) = (dir.join("atual"), dir.join("anterior"), dir.join("troca"));
    let saindo = ler_pasta(&atual).ok().map(|(m, _)| m.sha256);
    if ler_pasta(&anterior).is_ok() {
        let _ = std::fs::remove_dir_all(&troca);
        if atual.exists() {
            std::fs::rename(&atual, &troca).map_err(|e| e.to_string())?;
        }
        std::fs::rename(&anterior, &atual).map_err(|e| e.to_string())?;
        if troca.exists() {
            std::fs::rename(&troca, &anterior).map_err(|e| e.to_string())?;
        }
    } else if atual.exists() {
        std::fs::remove_dir_all(&atual).map_err(|e| e.to_string())?;
    } else {
        return Err("não há outro motor para voltar".into());
    }
    if let Some(s) = saindo {
        let _ = std::fs::write(dir.join("recusado.txt"), s);
    }
    Ok(ativo(dir))
}

pub fn recusado(dir: &Path) -> Option<String> {
    std::fs::read_to_string(dir.join("recusado.txt")).ok().map(|s| s.trim().to_string())
}

pub fn pasta(dados: &Path) -> PathBuf {
    dados.join("motor")
}

#[cfg(test)]
mod testes {
    use super::*;

    #[test]
    fn embutido_assinado_confere() {
        let manif = include_bytes!("../../src/motor/manifesto.json");
        let sig = include_str!("../../src/motor/manifesto.json.sig");
        verificar_assinatura(manif, sig).expect("assinatura do embutido");
        let mut adulterado = manif.to_vec();
        adulterado[10] ^= 1;
        assert!(verificar_assinatura(&adulterado, sig).is_err());
    }

    #[test]
    fn ordem_das_versoes() {
        let m = |v: &str, d: &str, s: &str| Manifesto { nome: "x".into(), versao: v.into(), api: 1, data: d.into(), sha256: s.into(), resto: Default::default() };
        assert!(mais_novo(&m("1.10.0", "2026-01-01", "a"), &m("1.9.0", "2026-12-01", "b")));
        assert!(!mais_novo(&m("1.9.0", "2026-12-01", "a"), &m("1.9.0", "2026-12-01", "a")));
        assert!(mais_novo(&m("1.9.0", "2026-12-02", "a"), &m("1.9.0", "2026-12-01", "b")));
    }
}
