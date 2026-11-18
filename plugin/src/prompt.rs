//! As fontes do 🧠 Melhor Próximo Ato — o prompt e, desde 16/11/2026, o Guia de Formatação, o RPP, o Livro de Modelos e
//! cada modelo que a IA pede: quem usa o Incognitus escolhe as suas, e o app não traz nenhuma. Cada uma pode ser
//!   · um arquivo .txt, .md ou .docx do computador;
//!   · o link de um Google Docs (o caminho principal): lido pela API do Docs com o login do Google de quem usa, então
//!     só abre o Doc que essa conta já pode abrir; os links do texto vêm junto, entre < >;
//!   · o link de uma planilha do Google (o Livro de Modelos, como o do Graphé: nome, link e etiquetas de cada modelo),
//!     lida pela API do Sheets, só a aba do link (#gid) ou, sem ela, a que tem "modelo" no nome, ou a primeira;
//!   · o link de um arquivo do OneDrive ou do SharePoint compartilhado com "qualquer pessoa com o link" (o login da
//!     Microsoft ainda não está no app).
//! O texto é lido de novo a cada 🧠, para valer sempre a versão atual, e nunca é gravado pelo app.
use base64::Engine;
use serde::Serialize;
use serde_json::Value;
use std::path::{Path, PathBuf};
use std::time::Duration;

/// O prompt maior que isto é recusado (um prompt de verdade tem dezenas de KB).
pub const MAX_BYTES: usize = 2_000_000;

#[derive(Debug, PartialEq)]
pub enum Fonte {
    Arquivo(PathBuf),
    GoogleDocs(String),
    /// o id da planilha e, se o link tiver, a aba (gid)
    GoogleSheets(String, Option<String>),
    Microsoft(String),
    /// um arquivo do Google Drive que não é Google Docs (o .docx, o .txt da pasta de modelos), pelo id
    GoogleDrive(String),
}

#[derive(Serialize, Debug)]
pub struct Lido {
    pub texto: String,
    /// "arquivo", "google", "planilha" ou "microsoft"
    pub tipo: &'static str,
    /// o nome do arquivo ou o título do Doc
    pub titulo: String,
    pub caracteres: usize,
}

/// De onde vem o prompt, pelo que a pessoa colou ou escolheu.
pub fn classificar(entrada: &str) -> Result<Fonte, String> {
    let e = entrada.trim();
    if e.is_empty() {
        return Err("Escolha um arquivo .txt ou .md, ou cole o link de um Google Docs.".into());
    }
    if e.starts_with("http://") || e.starts_with("https://") {
        let u = reqwest::Url::parse(e).map_err(|_| "Esse link não abre: confira se copiou inteiro.".to_string())?;
        let host = u.host_str().unwrap_or("").to_ascii_lowercase();
        if host == "docs.google.com" {
            let partes: Vec<&str> = u.path_segments().map(|s| s.collect()).unwrap_or_default();
            if let Some(i) = partes.iter().position(|p| *p == "spreadsheets") {
                let resto = &partes[i + 1..];
                let id = resto.iter().position(|p| *p == "d").and_then(|j| resto.get(j + 1)).map(|s| s.to_string());
                let gid = u
                    .fragment()
                    .into_iter()
                    .chain(u.query())
                    .flat_map(|q| q.split('&'))
                    .find_map(|kv| kv.strip_prefix("gid=").map(|g| g.to_string()))
                    .filter(|g| !g.is_empty() && g.chars().all(|c| c.is_ascii_digit()));
                return match id.filter(|id| id.len() >= 20 && id.chars().all(|c| c.is_ascii_alphanumeric() || c == '-' || c == '_')) {
                    Some(id) => Ok(Fonte::GoogleSheets(id, gid)),
                    None => Err("Não achei a planilha nesse link: copie o endereço da planilha aberta (docs.google.com/spreadsheets/d/…).".into()),
                };
            }
            let i = partes.iter().position(|p| *p == "document");
            let id = i.and_then(|i| {
                // /document/d/ID/edit ou /document/u/0/d/ID/edit
                let resto = &partes[i + 1..];
                resto.iter().position(|p| *p == "d").and_then(|j| resto.get(j + 1)).map(|s| s.to_string())
            });
            return match id.filter(|id| id.len() >= 20 && id.chars().all(|c| c.is_ascii_alphanumeric() || c == '-' || c == '_')) {
                Some(id) => Ok(Fonte::GoogleDocs(id)),
                None if partes.first() == Some(&"presentation") => {
                    Err("Esse link é de uma apresentação; o texto tem de estar num Google Docs (ou, o Livro de Modelos, numa planilha).".into())
                }
                None => Err("Não achei o Doc nesse link: copie o endereço do Google Docs aberto (docs.google.com/document/d/…).".into()),
            };
        }
        if host == "drive.google.com" {
            // o .docx ou o .txt guardado no Drive (/file/d/ID/…): baixado pela API do Drive, com a permissão das pastas
            let partes: Vec<&str> = u.path_segments().map(|s| s.collect()).unwrap_or_default();
            let id = partes.iter().position(|p| *p == "d").and_then(|j| partes.get(j + 1)).map(|s| s.to_string());
            if let Some(id) = id.filter(|id| id.len() >= 20 && id.chars().all(|c| c.is_ascii_alphanumeric() || c == '-' || c == '_')) {
                return Ok(Fonte::GoogleDrive(id));
            }
            return Err("Esse link é de uma pasta ou de um arquivo do Drive que não sei abrir. Abra o arquivo como Google Docs e copie o link dele.".into());
        }
        if host == "1drv.ms" || host == "onedrive.live.com" || host == "api.onedrive.com" || host.ends_with(".sharepoint.com") {
            return Ok(Fonte::Microsoft(e.to_string()));
        }
        return Err("Só leio links do Google Docs, das planilhas do Google, do OneDrive e do SharePoint.".into());
    }
    let p = PathBuf::from(e);
    match p.extension().map(|x| x.to_string_lossy().to_ascii_lowercase()).as_deref() {
        Some("txt") | Some("md") | Some("markdown") | Some("docx") => Ok(Fonte::Arquivo(p)),
        // o atalho do Google Drive para computador: só o id do Google Docs, que se lê pela API do Docs
        Some("gdoc") => {
            let t = std::fs::read_to_string(&p).map_err(|_| format!("Não consegui abrir o atalho do Google Docs ({}).", nome_do_arquivo(&p)))?;
            crate::pastas::id_do_gdoc(&t).map(Fonte::GoogleDocs).ok_or_else(|| format!("O atalho {} não traz o Google Docs.", nome_do_arquivo(&p)))
        }
        _ => Err("O arquivo tem de ser .txt, .md ou .docx.".into()),
    }
}

fn nome_do_arquivo(p: &Path) -> String {
    p.file_name().map(|n| n.to_string_lossy().to_string()).unwrap_or_default()
}

pub fn ler_arquivo(p: &Path) -> Result<Lido, String> {
    let meta = std::fs::metadata(p).map_err(|_| format!("Não achei o arquivo ({}).", nome_do_arquivo(p)))?;
    let docx = p.extension().map_or(false, |x| x.to_string_lossy().eq_ignore_ascii_case("docx"));
    if meta.len() as usize > MAX_BYTES * if docx { 5 } else { 1 } {
        return Err(format!("O arquivo é grande demais ({}).", nome_do_arquivo(p)));
    }
    if docx {
        let t = crate::reid::docx_texto(p).map_err(|_| format!("Não consegui ler o .docx ({}).", nome_do_arquivo(p)))?;
        return lido(t, "arquivo", nome_do_arquivo(p));
    }
    let b = std::fs::read(p).map_err(|e| format!("Não consegui ler o arquivo: {e}"))?;
    lido(crate::reid::decodificar(&b), "arquivo", nome_do_arquivo(p))
}

pub(crate) fn lido(texto: String, tipo: &'static str, titulo: String) -> Result<Lido, String> {
    let texto = texto.trim_start_matches('\u{feff}').replace("\r\n", "\n");
    if texto.trim().chars().count() < 20 {
        return Err("O texto está vazio (ou quase): confira o arquivo ou o Doc.".into());
    }
    let caracteres = texto.chars().count();
    Ok(Lido { texto, tipo, titulo, caracteres })
}

// ── Google Docs ─────────────────────────────────────────────────────────────────────────────────────────────────────

/// O endereço da API do Docs (ou o de um Google de mentira, nas provas).
pub fn url_docs(id: &str) -> String {
    let base = crate::google::modo_prova().unwrap_or_else(|| "https://docs.googleapis.com".to_string());
    format!("{}/v1/documents/{id}?includeTabsContent=true", base.trim_end_matches('/'))
}

/// O texto de um documento da API do Docs (documents.get com includeTabsContent): as abas na ordem, os títulos com
/// "#", os itens de lista com "- " e as tabelas com " | " entre as células.
pub fn texto_do_doc(v: &Value) -> String {
    let mut out = String::new();
    fn conteudo(c: &Value, out: &mut String) {
        for el in c.as_array().into_iter().flatten() {
            if let Some(p) = el.get("paragraph") {
                let mut linha = String::new();
                for e in p.get("elements").and_then(Value::as_array).into_iter().flatten() {
                    if let Some(t) = e.pointer("/textRun/content").and_then(Value::as_str) {
                        // o link vem junto, entre < >, quando o texto não é o próprio endereço
                        match e.pointer("/textRun/textStyle/link/url").and_then(Value::as_str) {
                            Some(url) if !t.contains(url) => {
                                let corpo = t.trim_end_matches('\n');
                                linha.push_str(corpo);
                                linha.push_str(&format!(" <{url}>"));
                                linha.push_str(&t[corpo.len()..]);
                            }
                            _ => linha.push_str(t),
                        }
                    } else if let Some(r) = e.pointer("/richLink/richLinkProperties") {
                        // o "chip" de um arquivo do Drive: o título e o endereço
                        let titulo = r.get("title").and_then(Value::as_str).unwrap_or("");
                        let uri = r.get("uri").and_then(Value::as_str).unwrap_or("");
                        linha.push_str(&format!("{titulo} <{uri}>"));
                    }
                }
                let estilo = p.pointer("/paragraphStyle/namedStyleType").and_then(Value::as_str).unwrap_or("");
                let prefixo = match estilo {
                    "TITLE" | "HEADING_1" => "# ",
                    "HEADING_2" => "## ",
                    "HEADING_3" => "### ",
                    "HEADING_4" | "HEADING_5" | "HEADING_6" => "#### ",
                    _ if p.get("bullet").is_some() => "- ",
                    _ => "",
                };
                let corpo = linha.trim_end_matches('\n');
                if !corpo.trim().is_empty() {
                    out.push_str(prefixo);
                }
                out.push_str(corpo);
                out.push('\n');
            } else if let Some(t) = el.get("table") {
                for linha in t.get("tableRows").and_then(Value::as_array).into_iter().flatten() {
                    let celulas: Vec<String> = linha
                        .get("tableCells")
                        .and_then(Value::as_array)
                        .into_iter()
                        .flatten()
                        .map(|c| {
                            let mut s = String::new();
                            conteudo(c.get("content").unwrap_or(&Value::Null), &mut s);
                            s.trim().replace('\n', " ")
                        })
                        .collect();
                    out.push_str(&celulas.join(" | "));
                    out.push('\n');
                }
            } else if let Some(t) = el.get("tableOfContents") {
                conteudo(t.get("content").unwrap_or(&Value::Null), out);
            }
        }
    }
    fn abas(lista: &Value, out: &mut String) {
        for aba in lista.as_array().into_iter().flatten() {
            conteudo(aba.pointer("/documentTab/body/content").unwrap_or(&Value::Null), out);
            abas(aba.get("childTabs").unwrap_or(&Value::Null), out);
        }
    }
    if v.get("tabs").map_or(false, |t| t.as_array().map_or(false, |a| !a.is_empty())) {
        abas(&v["tabs"], &mut out);
    } else {
        conteudo(v.pointer("/body/content").unwrap_or(&Value::Null), &mut out);
    }
    out
}

pub async fn ler_google(http: &reqwest::Client, id: &str, acesso: &str, conta: &str) -> Result<Lido, String> {
    let r = http
        .get(url_docs(id))
        .bearer_auth(acesso)
        .timeout(Duration::from_secs(60))
        .send()
        .await
        .map_err(|e| format!("Sem conexão com o Google ({}).", if e.is_timeout() { "tempo esgotado".to_string() } else { e.to_string() }))?;
    let codigo = r.status().as_u16();
    let texto = r.text().await.map_err(|e| format!("Sem conexão com o Google ({e})."))?;
    match codigo {
        200 => {}
        401 => return Err(crate::google::VENCIDO.into()),
        403 => return Err(format!("A conta {conta} não pode abrir esse Google Docs. Peça acesso a quem o compartilhou, ou entre com outra conta.")),
        404 => return Err("Não achei esse Google Docs: o link está certo? (Ou ele foi apagado.)".into()),
        c => return Err(format!("O Google recusou a leitura do Doc (código {c}).")),
    }
    let v: Value = serde_json::from_str(&texto).map_err(|_| "O Google devolveu o Doc num formato que não entendi.".to_string())?;
    let titulo = v.get("title").and_then(Value::as_str).unwrap_or("Google Docs").to_string();
    lido(texto_do_doc(&v), "google", titulo)
}

// ── planilha do Google (o Livro de Modelos) ─────────────────────────────────────────────────────────────────────────

pub const PERMISSAO_NOVA: &str = "Para ler planilhas, o Incognitus precisa de uma permissão nova do Google: nas Configurações, clique em Sair e depois em Entrar com o Google.";

fn base_sheets() -> String {
    crate::google::modo_prova().unwrap_or_else(|| "https://sheets.googleapis.com".to_string()).trim_end_matches('/').to_string()
}

/// O endereço que lista as abas da planilha (sem os dados).
pub fn url_planilha_abas(id: &str) -> String {
    format!("{}/v4/spreadsheets/{id}?fields=properties.title,sheets.properties(sheetId,title,index)", base_sheets())
}

/// O endereço que traz os valores de uma aba, com os links de cada célula.
pub fn url_planilha_dados(id: &str, aba: &str) -> String {
    let mut u = reqwest::Url::parse(&format!("{}/v4/spreadsheets/{id}", base_sheets())).unwrap();
    u.query_pairs_mut()
        .append_pair("includeGridData", "true")
        .append_pair("ranges", &format!("'{}'", aba.replace('\'', "''")))
        .append_pair("fields", "sheets.data.rowData.values(formattedValue,hyperlink,textFormatRuns.format.link.uri)");
    u.to_string()
}

/// A aba que vale: a do link (gid); sem ela, a primeira que tem "modelo" no nome; senão, a primeira.
pub fn escolher_aba(v: &Value, gid: Option<&str>) -> Option<String> {
    let mut abas: Vec<(i64, i64, String)> = v
        .get("sheets")
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
        .filter_map(|s| {
            let p = s.get("properties")?;
            Some((p.get("index").and_then(Value::as_i64).unwrap_or(0), p.get("sheetId").and_then(Value::as_i64).unwrap_or(-1), p.get("title")?.as_str()?.to_string()))
        })
        .collect();
    abas.sort();
    if let Some(g) = gid.and_then(|g| g.parse::<i64>().ok()) {
        if let Some(a) = abas.iter().find(|a| a.1 == g) {
            return Some(a.2.clone());
        }
    }
    let sem_acento = |t: &str| t.to_lowercase().replace(['ó', 'ò', 'ô', 'õ'], "o");
    abas.iter().find(|a| sem_acento(&a.2).contains("modelo")).or_else(|| abas.first()).map(|a| a.2.clone())
}

/// O texto de uma aba: uma linha por linha da planilha, as células com " | " e o link da célula entre < >.
pub fn texto_da_planilha(v: &Value) -> String {
    let mut out = String::new();
    let linhas = v.pointer("/sheets/0/data/0/rowData").and_then(Value::as_array);
    for l in linhas.into_iter().flatten() {
        let mut celulas: Vec<String> = l
            .get("values")
            .and_then(Value::as_array)
            .into_iter()
            .flatten()
            .map(|c| {
                let t = c.get("formattedValue").and_then(Value::as_str).unwrap_or("").replace(['\n', '\r'], " ").trim().to_string();
                let link = c.get("hyperlink").and_then(Value::as_str).map(str::to_string).or_else(|| {
                    c.get("textFormatRuns")
                        .and_then(Value::as_array)
                        .into_iter()
                        .flatten()
                        .find_map(|r| r.pointer("/format/link/uri").and_then(Value::as_str).map(str::to_string))
                });
                match link {
                    Some(u) if !t.contains(&u) => if t.is_empty() { format!("<{u}>") } else { format!("{t} <{u}>") },
                    _ => t,
                }
            })
            .collect();
        while celulas.last().map_or(false, |c| c.is_empty()) {
            celulas.pop();
        }
        if !celulas.is_empty() {
            out.push_str(&celulas.join(" | "));
            out.push('\n');
        }
    }
    out
}

async fn get_google(http: &reqwest::Client, url: &str, acesso: &str, conta: &str, oque: &str) -> Result<Value, String> {
    let r = http
        .get(url)
        .bearer_auth(acesso)
        .timeout(Duration::from_secs(60))
        .send()
        .await
        .map_err(|e| format!("Sem conexão com o Google ({}).", if e.is_timeout() { "tempo esgotado".to_string() } else { e.to_string() }))?;
    let codigo = r.status().as_u16();
    let texto = r.text().await.map_err(|e| format!("Sem conexão com o Google ({e})."))?;
    match codigo {
        200 => {}
        401 => return Err(crate::google::VENCIDO.into()),
        403 if texto.contains("SCOPE_INSUFFICIENT") || texto.to_lowercase().contains("insufficient authentication scopes") => {
            return Err(PERMISSAO_NOVA.into())
        }
        403 if texto.contains("SERVICE_DISABLED") || texto.contains("has not been used in project") => {
            return Err(format!("A leitura de {oque} ainda não está ativada no projeto do Google do Incognitus."))
        }
        403 => return Err(format!("A conta {conta} não pode abrir {oque}. Peça acesso a quem a compartilhou, ou entre com outra conta.")),
        404 => return Err(format!("Não achei {oque}: o link está certo? (Ou ela foi apagada.)")),
        c => return Err(format!("O Google recusou a leitura de {oque} (código {c}).")),
    }
    serde_json::from_str(&texto).map_err(|_| format!("O Google devolveu {oque} num formato que não entendi."))
}

pub async fn ler_planilha(http: &reqwest::Client, id: &str, gid: Option<&str>, acesso: &str, conta: &str) -> Result<Lido, String> {
    let meta = get_google(http, &url_planilha_abas(id), acesso, conta, "essa planilha").await?;
    let titulo = meta.pointer("/properties/title").and_then(Value::as_str).unwrap_or("Planilha").to_string();
    let aba = escolher_aba(&meta, gid).ok_or("A planilha não tem nenhuma aba.")?;
    let dados = get_google(http, &url_planilha_dados(id, &aba), acesso, conta, "essa planilha").await?;
    lido(texto_da_planilha(&dados), "planilha", format!("{titulo} › {aba}"))
}

// ── OneDrive e SharePoint ───────────────────────────────────────────────────────────────────────────────────────────

/// O endereço que baixa o arquivo de um link compartilhado: no OneDrive pessoal, a API de compartilhamentos
/// ("u!" + o link em base64url); no SharePoint, o próprio link com download=1.
pub fn url_microsoft(link: &str) -> String {
    let u = reqwest::Url::parse(link).ok();
    let host = u.as_ref().and_then(|u| u.host_str()).unwrap_or("").to_ascii_lowercase();
    // o arquivo de dentro de uma pasta compartilhada já vem com o endereço da API (pastas.rs): vai como está
    if host == "api.onedrive.com" || link.contains("/_api/v2.0/shares/") {
        return link.to_string();
    }
    if host.ends_with(".sharepoint.com") {
        let mut u = u.unwrap();
        let resto: Vec<(String, String)> = u.query_pairs().into_owned().filter(|(k, _)| k != "download").collect();
        u.query_pairs_mut().clear().extend_pairs(resto).append_pair("download", "1");
        return u.to_string();
    }
    let token = base64::engine::general_purpose::URL_SAFE_NO_PAD.encode(link.as_bytes());
    format!("https://api.onedrive.com/v1.0/shares/u!{token}/root/content")
}

/// O texto do que veio: .docx (começa por PK) vira texto; o resto, texto em UTF-8.
pub fn texto_dos_bytes(b: &[u8]) -> Result<String, String> {
    if b.starts_with(b"PK") {
        let dir = std::env::temp_dir().join(format!("incognitus-prompt-{}", std::process::id()));
        std::fs::create_dir_all(&dir).map_err(|e| e.to_string())?;
        let p = dir.join("prompt.docx");
        std::fs::write(&p, b).map_err(|e| e.to_string())?;
        let t = crate::reid::docx_texto(&p);
        let _ = std::fs::remove_dir_all(&dir);
        return t.map_err(|_| "O arquivo do link não é um .docx que eu consiga ler.".to_string());
    }
    let t = crate::reid::decodificar(b);
    let cabeca: String = t.chars().take(400).collect::<String>().to_ascii_lowercase();
    if cabeca.contains("<!doctype html") || cabeca.contains("<html") {
        return Err("O link abriu uma página de login, não o arquivo. Compartilhe-o com \"qualquer pessoa com o link\", ou baixe-o e escolha o arquivo.".into());
    }
    Ok(t)
}

pub async fn ler_microsoft(http: &reqwest::Client, link: &str) -> Result<Lido, String> {
    let r = http
        .get(url_microsoft(link))
        .timeout(Duration::from_secs(60))
        .send()
        .await
        .map_err(|e| format!("Sem conexão com a Microsoft ({e})."))?;
    let codigo = r.status().as_u16();
    if codigo == 401 || codigo == 403 {
        return Err("A Microsoft pediu login para esse link. Compartilhe-o com \"qualquer pessoa com o link\", ou baixe o arquivo e escolha-o.".into());
    }
    if codigo == 404 {
        return Err("Não achei o arquivo desse link do OneDrive/SharePoint.".into());
    }
    if codigo != 200 {
        return Err(format!("A Microsoft recusou o download (código {codigo})."));
    }
    let b = r.bytes().await.map_err(|e| format!("Sem conexão com a Microsoft ({e})."))?;
    if b.len() > MAX_BYTES * 5 {
        return Err("O arquivo do link é grande demais.".into());
    }
    lido(texto_dos_bytes(&b)?, "microsoft", "OneDrive/SharePoint".into())
}

#[cfg(test)]
mod testes {
    use super::*;
    use serde_json::json;

    #[test]
    fn reconhece_de_onde_vem_o_prompt() {
        let id = "1AbCdEfGhIjKlMnOpQrStUvWxYz0123456789_-xy";
        assert_eq!(classificar(&format!("https://docs.google.com/document/d/{id}/edit?tab=t.0")).unwrap(), Fonte::GoogleDocs(id.into()));
        assert_eq!(classificar(&format!(" https://docs.google.com/document/u/1/d/{id}/edit ")).unwrap(), Fonte::GoogleDocs(id.into()));
        assert_eq!(classificar(&format!("https://docs.google.com/spreadsheets/d/{id}/edit")).unwrap(), Fonte::GoogleSheets(id.into(), None));
        assert_eq!(classificar(&format!("https://docs.google.com/spreadsheets/d/{id}/edit#gid=12345")).unwrap(), Fonte::GoogleSheets(id.into(), Some("12345".into())));
        assert_eq!(classificar(&format!("https://docs.google.com/spreadsheets/u/0/d/{id}/edit?gid=7#gid=7")).unwrap(), Fonte::GoogleSheets(id.into(), Some("7".into())));
        assert!(classificar(&format!("https://docs.google.com/presentation/d/{id}/edit")).unwrap_err().contains("apresentação"));
        // o arquivo do Drive (o .docx ou o .txt da pasta de modelos) se baixa pela API do Drive; a pasta, não é fonte
        assert_eq!(classificar(&format!("https://drive.google.com/file/d/{id}/view")).unwrap(), Fonte::GoogleDrive(id.to_string()));
        assert!(classificar("https://drive.google.com/drive/folders/1AbCdEfGhIjKlMnOpQrStUvWx").unwrap_err().contains("Drive"));
        assert!(matches!(classificar("https://1drv.ms/w/s!AbcDef").unwrap(), Fonte::Microsoft(_)));
        assert!(matches!(classificar("https://empresa-my.sharepoint.com/:w:/g/personal/x/EabC?e=1").unwrap(), Fonte::Microsoft(_)));
        assert!(classificar("https://exemplo.invalid/prompt.md").is_err());
        assert_eq!(classificar("/tmp/Prompt MPA.MD").unwrap(), Fonte::Arquivo(PathBuf::from("/tmp/Prompt MPA.MD")));
        assert_eq!(classificar("/tmp/Guia.docx").unwrap(), Fonte::Arquivo(PathBuf::from("/tmp/Guia.docx")));
        assert!(classificar("/tmp/prompt.pdf").is_err());
        assert!(classificar("  ").is_err());
    }

    #[test]
    fn o_texto_do_google_docs_com_abas_titulos_listas_e_tabela() {
        let par = |t: &str, estilo: &str| json!({"paragraph": {"elements": [{"textRun": {"content": t}}], "paragraphStyle": {"namedStyleType": estilo}}});
        let doc = json!({
            "title": "Prompt MPA",
            "tabs": [
                {"documentTab": {"body": {"content": [
                    {"sectionBreak": {}},
                    par("Papel\n", "HEADING_1"),
                    {"paragraph": {"elements": [{"textRun": {"content": "Você é "}}, {"textRun": {"content": "o assessor.\n"}}], "paragraphStyle": {"namedStyleType": "NORMAL_TEXT"}}},
                    {"paragraph": {"elements": [{"textRun": {"content": "leia os autos\n"}}], "bullet": {"listId": "x"}, "paragraphStyle": {"namedStyleType": "NORMAL_TEXT"}}},
                    {"table": {"tableRows": [{"tableCells": [{"content": [par("Peça\n", "NORMAL_TEXT")]}, {"content": [par("Quando\n", "NORMAL_TEXT")]}]}]}}
                ]}},
                 "childTabs": [{"documentTab": {"body": {"content": [par("Aba filha\n", "HEADING_2")]}}}]}
            ]
        });
        assert_eq!(texto_do_doc(&doc), "# Papel\nVocê é o assessor.\n- leia os autos\nPeça | Quando\n## Aba filha\n");
        // sem abas (resposta antiga da API)
        assert_eq!(texto_do_doc(&json!({"body": {"content": [par("Só o corpo\n", "NORMAL_TEXT")]}})), "Só o corpo\n");
    }

    #[test]
    fn os_links_do_google_docs_vem_no_texto() {
        let doc = json!({"body": {"content": [
            {"paragraph": {"elements": [
                {"textRun": {"content": "Denúncia — furto"}},
                {"textRun": {"content": " (modelo)\n", "textStyle": {"link": {"url": "https://docs.google.com/document/d/abc/edit"}}}}
            ]}},
            {"paragraph": {"elements": [{"richLink": {"richLinkProperties": {"title": "Arquivamento", "uri": "https://docs.google.com/document/d/def/edit"}}}, {"textRun": {"content": "\n"}}]}},
            {"paragraph": {"elements": [{"textRun": {"content": "https://exemplo.invalid/x\n", "textStyle": {"link": {"url": "https://exemplo.invalid/x"}}}}]}}
        ]}});
        assert_eq!(texto_do_doc(&doc), "Denúncia — furto (modelo) <https://docs.google.com/document/d/abc/edit>\nArquivamento <https://docs.google.com/document/d/def/edit>\nhttps://exemplo.invalid/x\n");
    }

    #[test]
    fn a_planilha_do_livro_de_modelos() {
        let meta = json!({"properties": {"title": "MODELOS & PROMPTS"}, "sheets": [
            {"properties": {"sheetId": 0, "title": "Prompts", "index": 0}},
            {"properties": {"sheetId": 99, "title": "LinksModelos", "index": 1}},
            {"properties": {"sheetId": 7, "title": "Outra", "index": 2}}]});
        assert_eq!(escolher_aba(&meta, None).as_deref(), Some("LinksModelos"));
        assert_eq!(escolher_aba(&meta, Some("7")).as_deref(), Some("Outra"));
        assert_eq!(escolher_aba(&meta, Some("123")).as_deref(), Some("LinksModelos"));
        assert_eq!(escolher_aba(&json!({"sheets": [{"properties": {"sheetId": 3, "title": "A", "index": 0}}]}), None).as_deref(), Some("A"));
        let dados = json!({"sheets": [{"data": [{"rowData": [
            {"values": [{"formattedValue": "NOME_MINUTA"}, {"formattedValue": "link"}, {"formattedValue": "ETIQUETAS"}]},
            {"values": [{"formattedValue": "Denúncia — furto"}, {"formattedValue": "abrir", "hyperlink": "https://docs.google.com/document/d/abc/edit"}, {"formattedValue": "furto, 155"}]},
            {"values": [{"formattedValue": "Arquivamento"}, {"formattedValue": "https://docs.google.com/document/d/def/edit", "hyperlink": "https://docs.google.com/document/d/def/edit"}, {}]},
            {"values": [{"formattedValue": "Com link no texto", "textFormatRuns": [{"format": {}}, {"startIndex": 4, "format": {"link": {"uri": "https://docs.google.com/document/d/ghi/edit"}}}]}]},
            {"values": [{}, {}]}
        ]}]}]});
        assert_eq!(texto_da_planilha(&dados), "NOME_MINUTA | link | ETIQUETAS\n\
            Denúncia — furto | abrir <https://docs.google.com/document/d/abc/edit> | furto, 155\n\
            Arquivamento | https://docs.google.com/document/d/def/edit\n\
            Com link no texto <https://docs.google.com/document/d/ghi/edit>\n");
        let u = url_planilha_dados("ID_DE_PROVA_0123456789abc", "Links d'Ouro");
        assert!(u.contains("ranges=%27Links+d%27%27Ouro%27") && u.contains("includeGridData=true"), "{u}");
    }

    #[test]
    fn o_link_do_onedrive_e_do_sharepoint_vira_download() {
        let u = url_microsoft("https://1drv.ms/t/s!AbcD");
        assert!(u.starts_with("https://api.onedrive.com/v1.0/shares/u!aHR0cHM6Ly8xZHJ2Lm1zL3QvcyFBYmNE/root/content"), "{u}");
        let s = url_microsoft("https://x-my.sharepoint.com/:t:/g/personal/p/Eab?e=Zz&download=0");
        assert!(s.ends_with("e=Zz&download=1"), "{s}");
        // o arquivo de dentro da pasta compartilhada (pastas.rs) vai como veio
        let d = "https://api.onedrive.com/v1.0/shares/u!T/items/A1!5/content";
        assert_eq!(url_microsoft(d), d);
        assert!(matches!(classificar(d).unwrap(), Fonte::Microsoft(_)));
        assert_eq!(classificar("https://drive.google.com/file/d/1AbCdEfGhIjKlMnOpQrStUvWx/view").unwrap(), Fonte::GoogleDrive("1AbCdEfGhIjKlMnOpQrStUvWx".into()));
        assert!(texto_dos_bytes(b"<!DOCTYPE html><html><body>Entrar</body></html>").unwrap_err().contains("login"));
        assert_eq!(texto_dos_bytes("Você é o assessor".as_bytes()).unwrap(), "Você é o assessor");
    }

    #[test]
    fn le_o_prompt_em_arquivo() {
        let d = std::env::temp_dir().join(format!("incognitus-prova-prompt-{}", std::process::id()));
        std::fs::create_dir_all(&d).unwrap();
        let p = d.join("prompt.md");
        std::fs::write(&p, "\u{feff}# Papel\r\nVocê é o assessor jurídico fictício.\r\n").unwrap();
        let l = ler_arquivo(&p).unwrap();
        assert_eq!(l.texto, "# Papel\nVocê é o assessor jurídico fictício.\n");
        assert_eq!((l.tipo, l.titulo.as_str()), ("arquivo", "prompt.md"));
        std::fs::write(&p, "curto").unwrap();
        assert!(ler_arquivo(&p).unwrap_err().contains("vazio"));
        assert!(ler_arquivo(&d.join("nao-existe.md")).unwrap_err().contains("Não achei"));
        std::fs::remove_dir_all(&d).unwrap();
    }
}
