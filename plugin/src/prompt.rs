//! O prompt do 🧠 Melhor Próximo Ato: quem usa o Incognitus escolhe o próprio, e o app não traz nenhum. Pode ser
//!   · um arquivo .txt ou .md do computador;
//!   · o link de um Google Docs (o caminho principal): lido pela API do Docs com o login do Google de quem usa, então
//!     só abre o Doc que essa conta já pode abrir;
//!   · o link de um arquivo do OneDrive ou do SharePoint compartilhado com "qualquer pessoa com o link" (o login da
//!     Microsoft ainda não está no app).
//! O texto é lido de novo a cada 🧠, para valer sempre a versão atual do Doc, e nunca é gravado pelo app.
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
    Microsoft(String),
}

#[derive(Serialize, Debug)]
pub struct Lido {
    pub texto: String,
    /// "arquivo", "google" ou "microsoft"
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
            let i = partes.iter().position(|p| *p == "document");
            let id = i.and_then(|i| {
                // /document/d/ID/edit ou /document/u/0/d/ID/edit
                let resto = &partes[i + 1..];
                resto.iter().position(|p| *p == "d").and_then(|j| resto.get(j + 1)).map(|s| s.to_string())
            });
            return match id.filter(|id| id.len() >= 20 && id.chars().all(|c| c.is_ascii_alphanumeric() || c == '-' || c == '_')) {
                Some(id) => Ok(Fonte::GoogleDocs(id)),
                None if partes.first() == Some(&"spreadsheets") || partes.first() == Some(&"presentation") => {
                    Err("Esse link é de uma planilha ou apresentação; o prompt tem de estar num Google Docs.".into())
                }
                None => Err("Não achei o Doc nesse link: copie o endereço do Google Docs aberto (docs.google.com/document/d/…).".into()),
            };
        }
        if host == "drive.google.com" {
            return Err("Esse link é de um arquivo do Drive, não de um Google Docs. Abra o arquivo como Google Docs e copie o link dele.".into());
        }
        if host == "1drv.ms" || host == "onedrive.live.com" || host.ends_with(".sharepoint.com") {
            return Ok(Fonte::Microsoft(e.to_string()));
        }
        return Err("Só leio links do Google Docs, do OneDrive e do SharePoint.".into());
    }
    let p = PathBuf::from(e);
    match p.extension().map(|x| x.to_string_lossy().to_ascii_lowercase()).as_deref() {
        Some("txt") | Some("md") | Some("markdown") => Ok(Fonte::Arquivo(p)),
        _ => Err("O prompt em arquivo tem de ser .txt ou .md.".into()),
    }
}

fn nome_do_arquivo(p: &Path) -> String {
    p.file_name().map(|n| n.to_string_lossy().to_string()).unwrap_or_default()
}

pub fn ler_arquivo(p: &Path) -> Result<Lido, String> {
    let meta = std::fs::metadata(p).map_err(|_| format!("Não achei o arquivo do prompt ({}).", nome_do_arquivo(p)))?;
    if meta.len() as usize > MAX_BYTES {
        return Err("O arquivo do prompt é grande demais (mais de 2 MB).".into());
    }
    let b = std::fs::read(p).map_err(|e| format!("Não consegui ler o arquivo do prompt: {e}"))?;
    lido(crate::reid::decodificar(&b), "arquivo", nome_do_arquivo(p))
}

fn lido(texto: String, tipo: &'static str, titulo: String) -> Result<Lido, String> {
    let texto = texto.trim_start_matches('\u{feff}').replace("\r\n", "\n");
    if texto.trim().chars().count() < 20 {
        return Err("O prompt está vazio (ou quase): confira o arquivo ou o Doc.".into());
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
                        linha.push_str(t);
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

// ── OneDrive e SharePoint ───────────────────────────────────────────────────────────────────────────────────────────

/// O endereço que baixa o arquivo de um link compartilhado: no OneDrive pessoal, a API de compartilhamentos
/// ("u!" + o link em base64url); no SharePoint, o próprio link com download=1.
pub fn url_microsoft(link: &str) -> String {
    let u = reqwest::Url::parse(link).ok();
    let host = u.as_ref().and_then(|u| u.host_str()).unwrap_or("").to_ascii_lowercase();
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
        return Err("O arquivo do link é grande demais para um prompt.".into());
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
        assert!(classificar(&format!("https://docs.google.com/spreadsheets/d/{id}/edit")).unwrap_err().contains("planilha"));
        assert!(classificar(&format!("https://drive.google.com/file/d/{id}/view")).unwrap_err().contains("Drive"));
        assert!(matches!(classificar("https://1drv.ms/w/s!AbcDef").unwrap(), Fonte::Microsoft(_)));
        assert!(matches!(classificar("https://empresa-my.sharepoint.com/:w:/g/personal/x/EabC?e=1").unwrap(), Fonte::Microsoft(_)));
        assert!(classificar("https://exemplo.invalid/prompt.md").is_err());
        assert_eq!(classificar("/tmp/Prompt MPA.MD").unwrap(), Fonte::Arquivo(PathBuf::from("/tmp/Prompt MPA.MD")));
        assert!(classificar("/tmp/prompt.docx").is_err());
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
    fn o_link_do_onedrive_e_do_sharepoint_vira_download() {
        let u = url_microsoft("https://1drv.ms/t/s!AbcD");
        assert!(u.starts_with("https://api.onedrive.com/v1.0/shares/u!aHR0cHM6Ly8xZHJ2Lm1zL3QvcyFBYmNE/root/content"), "{u}");
        let s = url_microsoft("https://x-my.sharepoint.com/:t:/g/personal/p/Eab?e=Zz&download=0");
        assert!(s.ends_with("e=Zz&download=1"), "{s}");
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
