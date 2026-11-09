//! A versão nova do app, como no Comentora: pergunta pela release mais nova do repositório público e, se for mais nova
//! que esta, baixa o instalador desta plataforma (o de nome fixo) para a pasta Downloads e o abre. Só leitura; a mesma
//! opção dos Ajustes que liga a checagem das regras liga esta.
//! Primeiro pelo portal (consciencia.eco.br/incognitus/versao, que lê o GitHub do lado da Cloudflare): a rede do MPMT
//! bloqueia o github.com. Se o portal falhar, direto no GitHub.
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::time::Duration;

pub const URL_PADRAO: &str = "https://consciencia.eco.br/incognitus/versao";
pub const URL_GITHUB: &str = "https://api.github.com/repos/claudiogonzaga/incognitus/releases/latest";
#[cfg(target_os = "macos")]
const INSTALADOR: &str = "Incognitus-macOS.dmg";
#[cfg(target_os = "windows")]
const INSTALADOR: &str = "Incognitus-Windows.msi";
#[cfg(not(any(target_os = "macos", target_os = "windows")))]
const INSTALADOR: &str = "";
#[cfg(target_os = "macos")]
const SO: &str = "mac";
#[cfg(target_os = "windows")]
const SO: &str = "windows";
#[cfg(not(any(target_os = "macos", target_os = "windows")))]
const SO: &str = "";

/// O formato do portal: { versao, notas, pagina, instaladores: { mac: { url, tamanho }, windows: { … } } }.
#[derive(Deserialize)]
struct Portal {
    versao: String,
    #[serde(default)]
    notas: String,
    #[serde(default)]
    pagina: String,
    #[serde(default)]
    instaladores: HashMap<String, Instalador>,
}

#[derive(Deserialize)]
struct Instalador {
    url: String,
    #[serde(default)]
    tamanho: u64,
}

#[derive(Deserialize)]
struct Release {
    tag_name: String,
    #[serde(default)]
    body: String,
    #[serde(default)]
    html_url: String,
    #[serde(default)]
    draft: bool,
    #[serde(default)]
    prerelease: bool,
    #[serde(default)]
    assets: Vec<Asset>,
}

#[derive(Deserialize)]
struct Asset {
    name: String,
    browser_download_url: String,
    #[serde(default)]
    size: u64,
}

#[derive(Serialize, Clone)]
pub struct NovaVersao {
    pub versao: String,
    pub notas: String,
    pub url: String,
    pub tamanho: u64,
    pub pagina: String,
}

fn numeros(v: &str) -> Vec<u64> {
    v.trim().trim_start_matches(['v', 'V']).split(|c: char| !c.is_ascii_digit()).filter(|s| !s.is_empty()).map(|s| s.parse().unwrap_or(0)).collect()
}

/// "v0.3.1" é mais nova que "0.3.0"? (número a número; "0.10" > "0.9")
pub fn maior(a: &str, b: &str) -> bool {
    numeros(a) > numeros(b)
}

fn cliente(prazo: u64) -> Result<reqwest::Client, String> {
    reqwest::Client::builder()
        .user_agent(concat!("Incognitus/", env!("CARGO_PKG_VERSION")))
        .timeout(Duration::from_secs(prazo))
        .build()
        .map_err(|e| e.to_string())
}

fn servidor(url: &str) -> &'static str {
    if url.contains("github.com") { "o GitHub" } else { "o servidor de atualizações" }
}

/// A release mais nova, se for mais nova que `atual` e tiver o instalador desta plataforma. Aceita o formato do portal e
/// o da API do GitHub (que o teste de ponta a ponta usa).
pub async fn checar(url: &str, atual: &str) -> Result<Option<NovaVersao>, String> {
    let quem = servidor(url);
    let r = cliente(30)?
        .get(url)
        .header("Accept", "application/vnd.github+json, application/json")
        .send()
        .await
        .map_err(|e| format!("sem conexão com {quem} ({e})"))?;
    if !r.status().is_success() {
        return Err(format!("{quem} respondeu {}", r.status().as_u16()));
    }
    let corpo = r.bytes().await.map_err(|e| e.to_string())?;
    let v: serde_json::Value = serde_json::from_slice(&corpo).map_err(|e| format!("resposta ilegível de {quem}: {e}"))?;
    interpretar(v, atual, quem)
}

/// Pura: lê a resposta (portal ou API do GitHub) e diz se há versão nova com o instalador deste sistema.
fn interpretar(v: serde_json::Value, atual: &str, quem: &str) -> Result<Option<NovaVersao>, String> {
    if v.get("tag_name").is_none() {
        let p: Portal = serde_json::from_value(v).map_err(|e| format!("resposta ilegível de {quem}: {e}"))?;
        if p.versao.is_empty() || !maior(&p.versao, atual) {
            return Ok(None);
        }
        let i = p.instaladores.get(SO).ok_or("a versão nova ainda não tem o instalador deste sistema")?;
        return Ok(Some(NovaVersao {
            versao: p.versao.trim_start_matches(['v', 'V']).to_string(),
            notas: p.notas.chars().take(800).collect(),
            url: i.url.clone(),
            tamanho: i.tamanho,
            pagina: p.pagina,
        }));
    }
    let rel: Release = serde_json::from_value(v).map_err(|e| format!("resposta ilegível de {quem}: {e}"))?;
    if rel.draft || rel.prerelease || !maior(&rel.tag_name, atual) {
        return Ok(None);
    }
    let a = rel.assets.iter().find(|a| a.name == INSTALADOR).ok_or("a versão nova ainda não tem o instalador deste sistema")?;
    Ok(Some(NovaVersao {
        versao: rel.tag_name.trim_start_matches(['v', 'V']).to_string(),
        notas: rel.body.chars().take(800).collect(),
        url: a.browser_download_url.clone(),
        tamanho: a.size,
        pagina: rel.html_url,
    }))
}

/// O padrão: primeiro o portal; se ele falhar (fora do ar, sem rede para ele), direto no GitHub.
pub async fn checar_padrao(atual: &str) -> Result<Option<NovaVersao>, String> {
    match checar(URL_PADRAO, atual).await {
        Ok(r) => Ok(r),
        Err(e1) => checar(URL_GITHUB, atual).await.map_err(|e2| format!("{e1}; direto: {e2}")),
    }
}

/// Baixa o instalador para `pasta` (Downloads), com o número da versão no nome; grava em .parcial e renomeia no fim.
pub async fn baixar(url: &str, versao: &str, pasta: &Path) -> Result<PathBuf, String> {
    let ext = Path::new(INSTALADOR).extension().and_then(|e| e.to_str()).unwrap_or("bin");
    let destino = pasta.join(format!("Incognitus-{versao}.{ext}"));
    let parcial = pasta.join(format!(".Incognitus-{versao}.{ext}.parcial"));
    let quem = servidor(url);
    let r = cliente(900)?.get(url).send().await.map_err(|e| format!("sem conexão com {quem} ({e})"))?;
    if !r.status().is_success() {
        return Err(format!("{quem} respondeu {}", r.status().as_u16()));
    }
    let dados = r.bytes().await.map_err(|e| format!("o download parou: {e}"))?;
    std::fs::write(&parcial, &dados).map_err(|e| format!("não consegui gravar em {}: {e}", pasta.display()))?;
    std::fs::rename(&parcial, &destino).map_err(|e| e.to_string())?;
    Ok(destino)
}

/// Abre o instalador: no Mac, o .dmg (arrastar o Incognitus para Aplicativos); no Windows, o .msi, que troca a versão.
pub fn abrir(caminho: &Path) -> Result<(), String> {
    #[cfg(target_os = "macos")]
    let r = std::process::Command::new("open").arg(caminho).spawn();
    #[cfg(target_os = "windows")]
    let r = std::process::Command::new("msiexec").arg("/i").arg(caminho).spawn();
    #[cfg(not(any(target_os = "macos", target_os = "windows")))]
    let r = std::process::Command::new("xdg-open").arg(caminho).spawn();
    r.map(|_| ()).map_err(|e| format!("não consegui abrir o instalador: {e}"))
}

#[cfg(test)]
mod testes {
    use super::*;

    #[test]
    fn le_o_formato_do_portal_e_o_do_github() {
        let portal = serde_json::json!({ "versao": "99.0.0", "notas": "n", "pagina": "https://exemplo.invalid/incognitus/",
            "instaladores": { "mac": { "url": "https://exemplo.invalid/incognitus/mac", "tamanho": 10 },
                              "windows": { "url": "https://exemplo.invalid/incognitus/windows", "tamanho": 20 } } });
        let n = interpretar(portal.clone(), "0.4.0", "o portal").unwrap().expect("versão nova");
        assert_eq!(n.versao, "99.0.0");
        assert!(n.url.ends_with(if cfg!(target_os = "windows") { "/windows" } else { "/mac" }));
        assert!(interpretar(portal, "99.0.0", "o portal").unwrap().is_none());
        let velha = serde_json::json!({ "versao": "0.3.0", "instaladores": {} });
        assert!(interpretar(velha, "0.4.0", "o portal").unwrap().is_none());
        let gh = serde_json::json!({ "tag_name": "v99.0.0", "body": "b", "html_url": "h", "draft": false, "prerelease": false,
            "assets": [ { "name": INSTALADOR, "browser_download_url": "https://exemplo.invalid/x", "size": 5 } ] });
        assert_eq!(interpretar(gh, "0.4.0", "o GitHub").unwrap().expect("nova").url, "https://exemplo.invalid/x");
    }

    #[test]
    fn ordem_das_versoes_do_app() {
        assert!(maior("v0.3.1", "0.3.0"));
        assert!(maior("0.10.0", "0.9.9"));
        assert!(maior("v1.0.0", "0.99.0"));
        assert!(!maior("v0.3.0", "0.3.0"));
        assert!(!maior("0.2.9", "0.3.0"));
    }
}
