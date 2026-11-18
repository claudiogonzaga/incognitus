//! A pasta de modelos do 🧠 (17/11/2026, pedido do usuário: "o programa deve ser capaz de buscar em uma pasta de modelos
//! — no Google Drive, no SharePoint, no OneDrive ou mesmo uma pasta do computador"). A pasta pode ser:
//!   · uma pasta do computador, com as subpastas: .docx, .txt, .md e os .gdoc que o Google Drive para computador põe no
//!     lugar de cada Google Docs (o .gdoc só tem o id do Doc, que se lê pela API do Docs, com o login do Google). As
//!     pastas do OneDrive e do SharePoint sincronizadas pelo app do OneDrive também são pastas do computador;
//!   · o link de uma pasta do Google Drive: listada pela API do Drive, com o login do Google e a permissão de ler as
//!     pastas do Drive (pedida à parte, só por quem usa a pasta por link);
//!   · o link de uma pasta do OneDrive ou do SharePoint compartilhada com "qualquer pessoa com o link": listada pela API
//!     de compartilhamentos da Microsoft, sem login. Se a Microsoft pedir login, o caminho é sincronizar a pasta.
//! Cada modelo volta com o `caminho` que o leitor de fontes (prompt.rs) sabe abrir: o arquivo, o link do Google Docs, o
//! do arquivo do Drive ou o endereço do arquivo na API da Microsoft.
use base64::Engine;
use serde::Serialize;
use serde_json::Value;
use std::path::{Path, PathBuf};
use std::time::Duration;

/// Até onde a busca desce e quantos modelos lê (uma pasta de modelos de verdade tem centenas, não dezenas de milhares).
const MAX_NIVEL: usize = 6;
const MAX_ARQUIVOS: usize = 5000;
const MAX_PASTAS: usize = 400;

pub const ERRO_SEM_DRIVE: &str = "Para ler a pasta do Google Drive pelo link, o Incognitus precisa da permissão de ler as pastas do \
Drive: clique em \"Permitir pastas do Drive\", nas Configurações do 🧠.";
const ERRO_MS_LOGIN: &str = "A Microsoft pediu login para listar essa pasta. Compartilhe-a com \"qualquer pessoa com o link\" — ou \
sincronize-a no computador com o app do OneDrive (vale para o SharePoint: Sincronizar, ou Adicionar atalho a Meus arquivos) e \
escolha a pasta sincronizada.";

#[derive(Debug, PartialEq)]
pub enum Pasta {
    Local(PathBuf),
    /// o id da pasta e, nos links antigos, a chave de recurso (resourcekey)
    Drive { id: String, chave: Option<String> },
    OneDrive(String),
    SharePoint(String),
}

#[derive(Serialize, Debug, Clone, PartialEq)]
pub struct ArquivoModelo {
    /// o que o leitor de fontes abre: o caminho do arquivo ou o link
    pub caminho: String,
    pub nome: String,
    /// a subpasta, a partir da pasta escolhida ("" na raiz)
    pub rel: String,
    pub tam: u64,
    pub mtime: u64,
}

fn id_valido(id: &str) -> bool {
    id.len() >= 10 && id.len() <= 100 && id.chars().all(|c| c.is_ascii_alphanumeric() || c == '-' || c == '_')
}

/// De onde vem a pasta, pelo que a pessoa escolheu ou colou.
pub fn classificar(entrada: &str) -> Result<Pasta, String> {
    let e = entrada.trim();
    if e.is_empty() {
        return Err("Escolha a pasta dos modelos, ou cole o link de uma pasta do Google Drive, do OneDrive ou do SharePoint.".into());
    }
    if !(e.starts_with("http://") || e.starts_with("https://")) {
        return Ok(Pasta::Local(PathBuf::from(e)));
    }
    let u = reqwest::Url::parse(e).map_err(|_| "Esse link não abre: confira se copiou inteiro.".to_string())?;
    let host = u.host_str().unwrap_or("").to_ascii_lowercase();
    let q = |k: &str| u.query_pairs().find(|(n, _)| n == k).map(|(_, v)| v.to_string()).filter(|v| !v.is_empty());
    if host == "drive.google.com" {
        let partes: Vec<&str> = u.path_segments().map(|s| s.collect()).unwrap_or_default();
        let id = partes.iter().position(|p| *p == "folders").and_then(|i| partes.get(i + 1)).map(|s| s.to_string()).or_else(|| q("id"));
        return match id.filter(|id| id_valido(id)) {
            Some(id) => Ok(Pasta::Drive { id, chave: q("resourcekey") }),
            None => Err("Esse link do Drive não é de uma pasta: abra a pasta dos modelos no Drive e copie o link dela (drive.google.com/drive/folders/…).".into()),
        };
    }
    if host == "1drv.ms" || host == "onedrive.live.com" {
        return Ok(Pasta::OneDrive(e.to_string()));
    }
    if host.ends_with(".sharepoint.com") {
        return Ok(Pasta::SharePoint(e.to_string()));
    }
    Err("Só leio pastas do computador e links de pastas do Google Drive, do OneDrive e do SharePoint.".into())
}

fn e_modelo(nome: &str) -> bool {
    let n = nome.to_ascii_lowercase();
    [".docx", ".txt", ".md", ".gdoc"].iter().any(|x| n.ends_with(x))
}

// ── a pasta do computador ───────────────────────────────────────────────────────────────────────────────────────────

/// Os modelos da pasta e das subpastas: .docx, .txt, .md e .gdoc; sem os ocultos e os "~$" que o Word deixa abertos.
pub fn listar_local(raiz: &Path) -> Result<Vec<ArquivoModelo>, String> {
    if !raiz.is_dir() {
        return Err("Não achei a pasta dos modelos (foi movida ou apagada?).".into());
    }
    let mut out = Vec::new();
    let mut pilha = vec![(raiz.to_path_buf(), 0usize)];
    while let Some((dir, nivel)) = pilha.pop() {
        let Ok(rd) = std::fs::read_dir(&dir) else { continue };
        for e in rd.flatten() {
            let p = e.path();
            let nome = e.file_name().to_string_lossy().to_string();
            if nome.starts_with('.') || nome.starts_with("~$") {
                continue;
            }
            let Ok(meta) = e.metadata() else { continue };
            if meta.is_dir() {
                if nivel < MAX_NIVEL {
                    pilha.push((p, nivel + 1));
                }
                continue;
            }
            if !e_modelo(&nome) {
                continue;
            }
            let rel = p.parent().and_then(|d| d.strip_prefix(raiz).ok()).map(|r| r.to_string_lossy().to_string()).unwrap_or_default();
            let mtime = meta.modified().ok().and_then(|t| t.duration_since(std::time::UNIX_EPOCH).ok()).map(|d| d.as_secs()).unwrap_or(0);
            out.push(ArquivoModelo { caminho: p.to_string_lossy().to_string(), nome, rel, tam: meta.len(), mtime });
            if out.len() >= MAX_ARQUIVOS {
                return Ok(out);
            }
        }
    }
    out.sort_by(|a, b| (&a.rel, &a.nome).cmp(&(&b.rel, &b.nome)));
    Ok(out)
}

/// O id do Google Docs de um .gdoc (o atalho do Google Drive para computador: um JSON com "doc_id"; os mais antigos
/// trazem só o "url").
pub fn id_do_gdoc(texto: &str) -> Option<String> {
    let v: Value = serde_json::from_str(texto).ok()?;
    if let Some(id) = v.get("doc_id").and_then(Value::as_str).filter(|id| id_valido(id)) {
        return Some(id.to_string());
    }
    let url = v.get("url").and_then(Value::as_str)?;
    let i = url.find("/d/")? + 3;
    let id: String = url[i..].chars().take_while(|c| c.is_ascii_alphanumeric() || *c == '-' || *c == '_').collect();
    id_valido(&id).then_some(id)
}

// ── o Google Drive ──────────────────────────────────────────────────────────────────────────────────────────────────

pub const DRIVE_BASE: &str = "https://www.googleapis.com";
const PASTA_MIME: &str = "application/vnd.google-apps.folder";
const DOCS_MIME: &str = "application/vnd.google-apps.document";
const ATALHO_MIME: &str = "application/vnd.google-apps.shortcut";

/// "2026-11-17T10:20:30.123Z" → segundos desde 1970 (0 se não der para ler).
pub fn segundos_rfc3339(s: &str) -> u64 {
    let b = s.as_bytes();
    let n = |i: usize, k: usize| -> Option<i64> { std::str::from_utf8(b.get(i..i + k)?).ok()?.parse().ok() };
    let (Some(a), Some(m), Some(d), Some(h), Some(mi), Some(se)) = (n(0, 4), n(5, 2), n(8, 2), n(11, 2), n(14, 2), n(17, 2)) else {
        return 0;
    };
    // os dias desde 1970-01-01 (o algoritmo de Howard Hinnant, days_from_civil)
    let y = if m <= 2 { a - 1 } else { a };
    let era = y.div_euclid(400);
    let yoe = y - era * 400;
    let mp = (m + 9) % 12;
    let doy = (153 * mp + 2) / 5 + d - 1;
    let doe = yoe * 365 + yoe / 4 - yoe / 100 + doy;
    let dias = era * 146097 + doe - 719468;
    (dias * 86400 + h * 3600 + mi * 60 + se).max(0) as u64
}

/// Um arquivo do Drive que serve de modelo: o Google Docs (lido pela API do Docs) ou o .docx/.txt/.md (baixado pela do
/// Drive). Pasta, planilha, PDF e o resto ficam de fora.
fn modelo_do_drive(id: &str, nome: &str, mime: &str, rel: &str, tam: u64, mtime: u64) -> Option<ArquivoModelo> {
    let caminho = if mime == DOCS_MIME {
        format!("https://docs.google.com/document/d/{id}/edit")
    } else if mime.contains("wordprocessingml") || mime == "text/plain" || mime == "text/markdown" || e_modelo(nome) {
        if nome.to_ascii_lowercase().ends_with(".gdoc") {
            return None;
        }
        format!("https://drive.google.com/file/d/{id}/view")
    } else {
        return None;
    };
    Some(ArquivoModelo { caminho, nome: nome.to_string(), rel: rel.to_string(), tam, mtime })
}

/// O que veio numa página da lista do Drive: os modelos, as subpastas (id, chave, nome) e a próxima página.
pub fn itens_do_drive(v: &Value, rel: &str) -> (Vec<ArquivoModelo>, Vec<(String, Option<String>, String)>, Option<String>) {
    let mut modelos = Vec::new();
    let mut pastas = Vec::new();
    for f in v.get("files").and_then(Value::as_array).into_iter().flatten() {
        let s = |k: &str| f.get(k).and_then(Value::as_str).unwrap_or("").to_string();
        let (mut id, nome, mut mime, mut chave) = (s("id"), s("name"), s("mimeType"), f.get("resourceKey").and_then(Value::as_str).map(str::to_string));
        if mime == ATALHO_MIME {
            let alvo = f.get("shortcutDetails");
            id = alvo.and_then(|a| a.get("targetId")).and_then(Value::as_str).unwrap_or("").to_string();
            mime = alvo.and_then(|a| a.get("targetMimeType")).and_then(Value::as_str).unwrap_or("").to_string();
            chave = alvo.and_then(|a| a.get("targetResourceKey")).and_then(Value::as_str).map(str::to_string);
        }
        if !id_valido(&id) || nome.starts_with("~$") {
            continue;
        }
        if mime == PASTA_MIME {
            pastas.push((id, chave, nome));
            continue;
        }
        let tam = f.get("size").and_then(|x| x.as_str().and_then(|t| t.parse().ok()).or_else(|| x.as_u64())).unwrap_or(0);
        let mtime = segundos_rfc3339(f.get("modifiedTime").and_then(Value::as_str).unwrap_or(""));
        if let Some(m) = modelo_do_drive(&id, &nome, &mime, rel, tam, mtime) {
            modelos.push(m);
        }
    }
    let prox = v.get("nextPageToken").and_then(Value::as_str).map(str::to_string);
    (modelos, pastas, prox)
}

fn url_lista_drive(base: &str, pasta: &str, pagina: Option<&str>) -> String {
    let mut u = reqwest::Url::parse(&format!("{base}/drive/v3/files")).expect("o endereço do Drive é fixo");
    u.query_pairs_mut()
        .append_pair("q", &format!("'{pasta}' in parents and trashed = false"))
        .append_pair(
            "fields",
            "nextPageToken,files(id,name,mimeType,modifiedTime,size,resourceKey,shortcutDetails(targetId,targetMimeType,targetResourceKey))",
        )
        .append_pair("pageSize", "1000")
        .append_pair("supportsAllDrives", "true")
        .append_pair("includeItemsFromAllDrives", "true");
    if let Some(p) = pagina {
        u.query_pairs_mut().append_pair("pageToken", p);
    }
    u.to_string()
}

/// Um GET na API do Google, com as respostas de erro em português (a permissão do Drive à parte).
pub async fn get_google_drive(http: &reqwest::Client, url: &str, acesso: &str, chaves: Option<&str>) -> Result<reqwest::Response, String> {
    let mut req = http.get(url).bearer_auth(acesso).timeout(Duration::from_secs(60));
    if let Some(c) = chaves {
        req = req.header("X-Goog-Drive-Resource-Keys", c);
    }
    let r = req.send().await.map_err(|e| format!("Sem conexão com o Google Drive ({e})."))?;
    let codigo = r.status().as_u16();
    if codigo == 200 {
        return Ok(r);
    }
    let texto = r.text().await.unwrap_or_default();
    let baixo = texto.to_lowercase();
    Err(match codigo {
        401 => crate::google::VENCIDO.into(),
        403 if texto.contains("SCOPE_INSUFFICIENT") || baixo.contains("insufficient") => ERRO_SEM_DRIVE.into(),
        403 if texto.contains("SERVICE_DISABLED") || baixo.contains("has not been used in project") || baixo.contains("is disabled") => {
            "A leitura de pastas do Google Drive ainda não está ativada no projeto do Google do Incognitus.".into()
        }
        403 => "A sua conta do Google não pode abrir essa pasta (ou esse arquivo) do Drive. Peça acesso a quem a compartilhou.".into(),
        404 => "Não achei essa pasta (ou esse arquivo) no Google Drive: o link está certo, e a sua conta pode abri-lo?".into(),
        c => format!("O Google Drive recusou a leitura (código {c})."),
    })
}

/// Os modelos de uma pasta do Drive e das subpastas (até MAX_NIVEL de fundo).
pub async fn listar_drive(http: &reqwest::Client, base: &str, id: &str, chave: Option<&str>, acesso: &str) -> Result<Vec<ArquivoModelo>, String> {
    let mut out = Vec::new();
    let mut fila = vec![(id.to_string(), chave.map(str::to_string), String::new(), 0usize)];
    let mut vistas = 0usize;
    while let Some((pasta, chave, rel, nivel)) = fila.pop() {
        vistas += 1;
        if vistas > MAX_PASTAS {
            break;
        }
        let chaves = chave.as_ref().map(|c| format!("{pasta}/{c}"));
        let mut pagina: Option<String> = None;
        loop {
            let r = get_google_drive(http, &url_lista_drive(base, &pasta, pagina.as_deref()), acesso, chaves.as_deref()).await?;
            let v: Value = serde_json::from_str(&r.text().await.map_err(|e| format!("Sem conexão com o Google Drive ({e})."))?)
                .map_err(|_| "O Google Drive devolveu a lista num formato que não entendi.".to_string())?;
            let (modelos, subpastas, prox) = itens_do_drive(&v, &rel);
            out.extend(modelos);
            if nivel < MAX_NIVEL {
                for (sid, sch, snome) in subpastas {
                    let srel = if rel.is_empty() { snome } else { format!("{rel}/{snome}") };
                    fila.push((sid, sch, srel, nivel + 1));
                }
            }
            if out.len() >= MAX_ARQUIVOS {
                return Ok(out);
            }
            match prox {
                Some(p) => pagina = Some(p),
                None => break,
            }
        }
    }
    out.sort_by(|a, b| (&a.rel, &a.nome).cmp(&(&b.rel, &b.nome)));
    Ok(out)
}

/// Os bytes de um arquivo do Drive (o .docx, o .txt) e o nome dele.
pub async fn baixar_drive(http: &reqwest::Client, base: &str, id: &str, acesso: &str) -> Result<(Vec<u8>, String, String), String> {
    let meta: Value = {
        let r = get_google_drive(http, &format!("{base}/drive/v3/files/{id}?fields=name,mimeType,size&supportsAllDrives=true"), acesso, None).await?;
        serde_json::from_str(&r.text().await.map_err(|e| format!("Sem conexão com o Google Drive ({e})."))?)
            .map_err(|_| "O Google Drive devolveu o arquivo num formato que não entendi.".to_string())?
    };
    let nome = meta.get("name").and_then(Value::as_str).unwrap_or("arquivo do Drive").to_string();
    let mime = meta.get("mimeType").and_then(Value::as_str).unwrap_or("").to_string();
    if mime == DOCS_MIME {
        return Ok((vec![], nome, mime));
    }
    let r = get_google_drive(http, &format!("{base}/drive/v3/files/{id}?alt=media&supportsAllDrives=true"), acesso, None).await?;
    let b = r.bytes().await.map_err(|e| format!("Sem conexão com o Google Drive ({e})."))?;
    Ok((b.to_vec(), nome, mime))
}

// ── o OneDrive e o SharePoint ───────────────────────────────────────────────────────────────────────────────────────

/// A raiz da API de compartilhamentos para o link: a do OneDrive pessoal ou a do SharePoint do próprio link.
pub fn base_microsoft(link: &str) -> String {
    let token = base64::engine::general_purpose::URL_SAFE_NO_PAD.encode(link.trim().as_bytes());
    let host = reqwest::Url::parse(link.trim()).ok().and_then(|u| u.host_str().map(str::to_ascii_lowercase)).unwrap_or_default();
    if host.ends_with(".sharepoint.com") {
        format!("https://{host}/_api/v2.0/shares/u!{token}")
    } else {
        format!("https://api.onedrive.com/v1.0/shares/u!{token}")
    }
}

/// O que veio numa página da lista da Microsoft: os modelos, as subpastas (id, nome) e a próxima página.
pub fn itens_da_microsoft(v: &Value, base: &str, rel: &str) -> (Vec<ArquivoModelo>, Vec<(String, String)>, Option<String>) {
    let mut modelos = Vec::new();
    let mut pastas = Vec::new();
    for f in v.get("value").and_then(Value::as_array).into_iter().flatten() {
        let id = f.get("id").and_then(Value::as_str).unwrap_or("").to_string();
        let nome = f.get("name").and_then(Value::as_str).unwrap_or("").to_string();
        if id.is_empty() || id.len() > 200 || nome.starts_with('.') || nome.starts_with("~$") {
            continue;
        }
        if f.get("folder").is_some() {
            pastas.push((id, nome));
            continue;
        }
        if f.get("file").is_none() || !e_modelo(&nome) || nome.to_ascii_lowercase().ends_with(".gdoc") {
            continue;
        }
        let idc: String = id.chars().map(|c| if c.is_ascii_alphanumeric() || "!-_.".contains(c) { c.to_string() } else { format!("%{:02X}", c as u32) }).collect();
        modelos.push(ArquivoModelo {
            caminho: format!("{base}/items/{idc}/content"),
            nome,
            rel: rel.to_string(),
            tam: f.get("size").and_then(Value::as_u64).unwrap_or(0),
            mtime: segundos_rfc3339(f.get("lastModifiedDateTime").and_then(Value::as_str).unwrap_or("")),
        });
    }
    let prox = v.get("@odata.nextLink").and_then(Value::as_str).map(str::to_string);
    (modelos, pastas, prox)
}

async fn get_microsoft(http: &reqwest::Client, url: &str) -> Result<Value, String> {
    let r = http
        .get(url)
        .header("accept", "application/json")
        .timeout(Duration::from_secs(60))
        .send()
        .await
        .map_err(|e| format!("Sem conexão com a Microsoft ({e})."))?;
    let codigo = r.status().as_u16();
    if codigo == 401 || codigo == 403 {
        return Err(ERRO_MS_LOGIN.into());
    }
    if codigo == 404 {
        return Err("Não achei a pasta desse link do OneDrive/SharePoint (ou ela não está compartilhada com quem tem o link).".into());
    }
    if codigo != 200 {
        return Err(format!("A Microsoft recusou a lista da pasta (código {codigo})."));
    }
    let t = r.text().await.map_err(|e| format!("Sem conexão com a Microsoft ({e})."))?;
    serde_json::from_str(&t).map_err(|_| ERRO_MS_LOGIN.to_string())
}

/// Os modelos de uma pasta do OneDrive ou do SharePoint compartilhada com "qualquer pessoa com o link".
pub async fn listar_microsoft(http: &reqwest::Client, link: &str, base_teste: Option<&str>) -> Result<Vec<ArquivoModelo>, String> {
    let base = base_teste.map(str::to_string).unwrap_or_else(|| base_microsoft(link));
    let raiz = if base.contains("/_api/v2.0/") { format!("{base}/driveItem/children") } else { format!("{base}/root/children") };
    let mut out = Vec::new();
    let mut fila = vec![(raiz, String::new(), 0usize)];
    let mut vistas = 0usize;
    while let Some((url, rel, nivel)) = fila.pop() {
        vistas += 1;
        if vistas > MAX_PASTAS {
            break;
        }
        let mut pagina = Some(url);
        while let Some(u) = pagina.take() {
            let v = get_microsoft(http, &u).await?;
            let (modelos, subpastas, prox) = itens_da_microsoft(&v, &base, &rel);
            out.extend(modelos);
            if nivel < MAX_NIVEL {
                for (sid, snome) in subpastas {
                    let srel = if rel.is_empty() { snome } else { format!("{rel}/{snome}") };
                    fila.push((format!("{base}/items/{sid}/children"), srel, nivel + 1));
                }
            }
            if out.len() >= MAX_ARQUIVOS {
                return Ok(out);
            }
            pagina = prox;
        }
    }
    out.sort_by(|a, b| (&a.rel, &a.nome).cmp(&(&b.rel, &b.nome)));
    Ok(out)
}

#[cfg(test)]
mod testes {
    use super::*;
    use serde_json::json;

    #[test]
    fn reconhece_de_onde_vem_a_pasta() {
        assert_eq!(classificar("/Users/x/Modelos").unwrap(), Pasta::Local(PathBuf::from("/Users/x/Modelos")));
        assert_eq!(
            classificar("https://drive.google.com/drive/folders/0By6AbcDefGhiJkl?resourcekey=0-abc&usp=sharing").unwrap(),
            Pasta::Drive { id: "0By6AbcDefGhiJkl".into(), chave: Some("0-abc".into()) }
        );
        assert_eq!(
            classificar("https://drive.google.com/drive/u/0/folders/1AbCdEfGhIjKlMnOpQrStUvWxYz").unwrap(),
            Pasta::Drive { id: "1AbCdEfGhIjKlMnOpQrStUvWxYz".into(), chave: None }
        );
        assert!(classificar("https://drive.google.com/file/d/1AbCdEfGhIjKlMnOpQrStUvWxYz/view").is_err());
        assert!(matches!(classificar("https://1drv.ms/f/s!AbcDef").unwrap(), Pasta::OneDrive(_)));
        assert!(matches!(classificar("https://empresa.sharepoint.com/:f:/s/Juridico/EabC?e=1").unwrap(), Pasta::SharePoint(_)));
        assert!(classificar("https://exemplo.com/pasta").is_err() && classificar("  ").is_err());
    }

    #[test]
    fn le_a_data_do_google_e_da_microsoft() {
        assert_eq!(segundos_rfc3339("1970-01-01T00:00:00Z"), 0);
        assert_eq!(segundos_rfc3339("2026-11-17T10:20:30.123Z"), 1_794_910_830);
        assert_eq!(segundos_rfc3339("2000-02-29T12:00:00Z"), 951_825_600);
        assert_eq!(segundos_rfc3339("lixo"), 0);
    }

    #[test]
    fn o_gdoc_do_google_drive_para_computador() {
        assert_eq!(id_do_gdoc(r#"{"": "WARNING!", "doc_id": "1AbCdEfGhIjKlMnOpQrStUv", "email": "x@y", "resource_key": ""}"#).as_deref(), Some("1AbCdEfGhIjKlMnOpQrStUv"));
        assert_eq!(id_do_gdoc(r#"{"url": "https://docs.google.com/document/d/1ZyXwVuTsRqPoNmLkJiH/edit?usp=drivesdk"}"#).as_deref(), Some("1ZyXwVuTsRqPoNmLkJiH"));
        assert_eq!(id_do_gdoc("não é json"), None);
    }

    #[test]
    fn a_pasta_do_computador_com_subpastas_e_gdoc() {
        let d = std::env::temp_dir().join(format!("incognitus-pasta-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&d);
        std::fs::create_dir_all(d.join("Recursos")).unwrap();
        std::fs::write(d.join("Denúncia.txt"), "x").unwrap();
        std::fs::write(d.join("Recursos").join("Contrarrazões.gdoc"), "{}").unwrap();
        std::fs::write(d.join("~$aberto.docx"), "x").unwrap();
        std::fs::write(d.join("foto.png"), "x").unwrap();
        let l = listar_local(&d).unwrap();
        let _ = std::fs::remove_dir_all(&d);
        let nomes: Vec<(String, String)> = l.iter().map(|a| (a.rel.clone(), a.nome.clone())).collect();
        assert_eq!(nomes, vec![("".into(), "Denúncia.txt".into()), ("Recursos".into(), "Contrarrazões.gdoc".into())]);
    }

    #[test]
    fn a_lista_do_drive_com_docs_docx_atalho_e_subpasta() {
        let v = json!({"nextPageToken": "p2", "files": [
            {"id": "docAAAAAAAAAA", "name": "Denúncia - tráfico", "mimeType": DOCS_MIME, "modifiedTime": "2026-11-17T10:20:30Z"},
            {"id": "docxBBBBBBBBB", "name": "Arquivamento.docx", "mimeType": "application/vnd.openxmlformats-officedocument.wordprocessingml.document", "size": "1234"},
            {"id": "pastaCCCCCCCC", "name": "Recursos", "mimeType": PASTA_MIME, "resourceKey": "0-k"},
            {"id": "atalhoDDDDDDD", "name": "Atalho", "mimeType": ATALHO_MIME, "shortcutDetails": {"targetId": "alvoEEEEEEEEE", "targetMimeType": DOCS_MIME}},
            {"id": "planilhaFFFFF", "name": "Livro", "mimeType": "application/vnd.google-apps.spreadsheet"},
            {"id": "pdfGGGGGGGGGG", "name": "autos.pdf", "mimeType": "application/pdf"}]});
        let (m, p, prox) = itens_do_drive(&v, "Criminal");
        assert_eq!(prox.as_deref(), Some("p2"));
        assert_eq!(p, vec![("pastaCCCCCCCC".into(), Some("0-k".into()), "Recursos".into())]);
        let c: Vec<&str> = m.iter().map(|a| a.caminho.as_str()).collect();
        assert_eq!(c, vec![
            "https://docs.google.com/document/d/docAAAAAAAAAA/edit",
            "https://drive.google.com/file/d/docxBBBBBBBBB/view",
            "https://docs.google.com/document/d/alvoEEEEEEEEE/edit",
        ]);
        assert_eq!((m[0].mtime, m[1].tam, m[0].rel.as_str()), (1_794_910_830, 1234, "Criminal"));
        let u = url_lista_drive(DRIVE_BASE, "pastaCCCCCCCC", Some("p2"));
        assert!(u.contains("q=%27pastaCCCCCCCC%27+in+parents+and+trashed+%3D+false") && u.contains("pageToken=p2") && u.contains("supportsAllDrives=true"), "{u}");
    }

    #[test]
    fn a_lista_do_onedrive_e_do_sharepoint() {
        assert!(base_microsoft("https://1drv.ms/f/s!Abc").starts_with("https://api.onedrive.com/v1.0/shares/u!aHR0cHM6Ly8xZHJ2Lm1zL2YvcyFBYmM"));
        assert!(base_microsoft("https://empresa.sharepoint.com/:f:/s/J/Eab?e=1").starts_with("https://empresa.sharepoint.com/_api/v2.0/shares/u!"));
        let v = json!({"@odata.nextLink": "https://x/proxima", "value": [
            {"id": "A1!5", "name": "Denúncia.docx", "size": 10, "file": {}, "lastModifiedDateTime": "2026-11-17T10:20:30Z"},
            {"id": "B2", "name": "Recursos", "folder": {"childCount": 3}},
            {"id": "C3", "name": "foto.png", "file": {}},
            {"id": "D4", "name": "~$aberto.docx", "file": {}}]});
        let (m, p, prox) = itens_da_microsoft(&v, "https://api.onedrive.com/v1.0/shares/u!T", "");
        assert_eq!(prox.as_deref(), Some("https://x/proxima"));
        assert_eq!(p, vec![("B2".to_string(), "Recursos".to_string())]);
        assert_eq!(m.len(), 1);
        assert_eq!(m[0].caminho, "https://api.onedrive.com/v1.0/shares/u!T/items/A1!5/content");
        assert_eq!((m[0].tam, m[0].mtime), (10, 1_794_910_830));
    }

    /// A pasta do Drive de verdade, contra um Google de mentira: a raiz, a subpasta (com a chave de recurso) e a segunda
    /// página.
    #[tokio::test]
    async fn lista_a_pasta_do_drive_descendo_nas_subpastas() {
        use std::io::{Read, Write};
        let l = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
        let base = format!("http://{}", l.local_addr().unwrap());
        let pedidos = std::sync::Arc::new(std::sync::Mutex::new(Vec::<String>::new()));
        let guardados = pedidos.clone();
        std::thread::spawn(move || {
            for s in l.incoming() {
                let mut s = s.unwrap();
                let mut b = [0u8; 16384];
                let n = s.read(&mut b).unwrap();
                let cab = String::from_utf8_lossy(&b[..n]).to_string();
                let linha = cab.lines().next().unwrap_or("").to_string();
                guardados.lock().unwrap().push(cab.clone());
                let corpo = if linha.contains("pageToken=p2") {
                    json!({"files": [{"id": "docZZZZZZZZZ", "name": "Segunda página", "mimeType": DOCS_MIME}]})
                } else if linha.contains("raizRRRRRRRR") {
                    json!({"nextPageToken": "p2", "files": [
                        {"id": "docAAAAAAAAAA", "name": "Denúncia", "mimeType": DOCS_MIME},
                        {"id": "subSSSSSSSSSS", "name": "Recursos", "mimeType": PASTA_MIME, "resourceKey": "0-k"}]})
                } else {
                    json!({"files": [{"id": "docBBBBBBBBBB", "name": "Contrarrazões", "mimeType": DOCS_MIME}]})
                }
                .to_string();
                let _ = s.write_all(format!("HTTP/1.1 200 OK\r\ncontent-type: application/json\r\ncontent-length: {}\r\nconnection: close\r\n\r\n{corpo}", corpo.len()).as_bytes());
            }
        });
        let cli = reqwest::Client::new();
        let l = listar_drive(&cli, &base, "raizRRRRRRRR", None, "acesso-de-prova").await.unwrap();
        let nomes: Vec<(String, String)> = l.iter().map(|a| (a.rel.clone(), a.nome.clone())).collect();
        assert_eq!(nomes, vec![("".into(), "Denúncia".into()), ("".into(), "Segunda página".into()), ("Recursos".into(), "Contrarrazões".into())]);
        let p = pedidos.lock().unwrap();
        assert!(p.iter().all(|c| c.to_lowercase().contains("authorization: bearer acesso-de-prova")));
        assert!(p.iter().any(|c| c.contains("subSSSSSSSSSS") && c.to_lowercase().contains("x-goog-drive-resource-keys: subssssssssss/0-k")));
    }
}
