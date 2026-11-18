//! A minuta no Google Docs (18/11/2026, pedido do usuário: ao lado de "Abrir no Word / LibreOffice", "poderia ter a opção
//! de abrir no Google Docs também"). O .docx que o editor gravou sobe para o Drive de quem usa e o Google o converte num
//! documento do Google Docs. É o único caminho pelo qual um texto com os nomes reais sai do computador, e só sai no
//! clique de quem usa, depois de uma confirmação, para a conta do Google dela.
//!
//! A permissão é a `drive.file`: o app só enxerga os arquivos que ele mesmo criou. Não lê o resto do Drive.

use serde::Serialize;
use serde_json::Value;
use std::time::Duration;

const DOCX_MIME: &str = "application/vnd.openxmlformats-officedocument.wordprocessingml.document";
const DOCS_MIME: &str = "application/vnd.google-apps.document";
/// O teto do .docx que sobe (o Google converte até 50 MB; uma minuta não chega perto disso).
pub const MAX_BYTES: usize = 20 * 1024 * 1024;

pub const ERRO_SEM_ESCRITA: &str = "Para abrir a minuta no Google Docs, o Incognitus precisa da permissão de criar documentos no seu \
Google Drive. Clique de novo em \"Abrir no Google Docs\" e aceite a permissão no navegador.";

#[derive(Serialize, Debug, PartialEq)]
pub struct NoDocs {
    pub id: String,
    pub link: String,
    /// "criado" (documento novo), "substituido" (o texto daqui trocou o de lá) ou "aberto" (o de lá ficou como estava)
    pub feito: String,
}

/// O id de um arquivo do Drive só tem letras, números, "-" e "_": nada além disso entra num endereço.
pub fn id_valido(id: &str) -> bool {
    (10..=128).contains(&id.len()) && id.bytes().all(|b| b.is_ascii_alphanumeric() || b == b'-' || b == b'_')
}

/// O endereço do documento, montado aqui a partir do id (o que o Google devolve não é usado como endereço).
pub fn link_do(id: &str) -> String {
    format!("https://docs.google.com/document/d/{id}/edit")
}

fn erro_da(codigo: u16, texto: &str) -> String {
    let baixo = texto.to_lowercase();
    match codigo {
        401 => crate::google::VENCIDO.into(),
        403 if texto.contains("SCOPE_INSUFFICIENT") || baixo.contains("insufficient") => ERRO_SEM_ESCRITA.into(),
        403 if texto.contains("SERVICE_DISABLED") || baixo.contains("has not been used in project") || baixo.contains("is disabled") => {
            "O Google Drive ainda não está ativado no projeto do Google do Incognitus.".into()
        }
        403 if baixo.contains("storagequota") || baixo.contains("quota") => "O Google recusou: o seu Drive está sem espaço (ou o limite de uso foi atingido).".into(),
        403 => "A sua conta do Google não pode criar documentos no Drive (a administração da conta pode ter bloqueado).".into(),
        c => format!("O Google Drive recusou o envio da minuta (código {c})."),
    }
}

fn sem_conexao(e: reqwest::Error) -> String {
    format!("Sem conexão com o Google Drive ({e}).")
}

async fn id_da_resposta(r: reqwest::Response) -> Result<String, String> {
    let v: Value = serde_json::from_str(&r.text().await.map_err(sem_conexao)?).map_err(|_| "O Google Drive devolveu uma resposta que não entendi.".to_string())?;
    let id = v.get("id").and_then(Value::as_str).unwrap_or("");
    if !id_valido(id) {
        return Err("O Google Drive não devolveu o documento criado.".into());
    }
    Ok(id.to_string())
}

/// O corpo do envio em duas partes (multipart/related): os dados do arquivo e o .docx.
pub fn corpo_em_partes(limite: &str, nome: &str, docx: &[u8]) -> Vec<u8> {
    let meta = serde_json::json!({ "name": nome, "mimeType": DOCS_MIME }).to_string();
    let mut c = Vec::with_capacity(docx.len() + meta.len() + 300);
    c.extend_from_slice(format!("--{limite}\r\nContent-Type: application/json; charset=UTF-8\r\n\r\n{meta}\r\n--{limite}\r\nContent-Type: {DOCX_MIME}\r\n\r\n").as_bytes());
    c.extend_from_slice(docx);
    c.extend_from_slice(format!("\r\n--{limite}--\r\n").as_bytes());
    c
}

async fn criar(http: &reqwest::Client, base: &str, acesso: &str, nome: &str, docx: &[u8]) -> Result<String, String> {
    // o limite das partes: só precisa não aparecer dentro do .docx (um zip); o relógio e o processo bastam
    let agora = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map(|d| d.as_nanos()).unwrap_or(0);
    let limite = format!("incognitus{:x}x{:x}", agora, std::process::id());
    let r = http
        .post(format!("{base}/upload/drive/v3/files?uploadType=multipart&fields=id"))
        .bearer_auth(acesso)
        .header("Content-Type", format!("multipart/related; boundary={limite}"))
        .timeout(Duration::from_secs(120))
        .body(corpo_em_partes(&limite, nome, docx))
        .send()
        .await
        .map_err(sem_conexao)?;
    let codigo = r.status().as_u16();
    if codigo != 200 {
        return Err(erro_da(codigo, &r.text().await.unwrap_or_default()));
    }
    id_da_resposta(r).await
}

/// O documento que já foi enviado ainda está lá (e fora da lixeira)? `None` quando o Google não o acha mais.
async fn existe(http: &reqwest::Client, base: &str, acesso: &str, id: &str) -> Result<bool, String> {
    let r = http
        .get(format!("{base}/drive/v3/files/{id}?fields=id,trashed"))
        .bearer_auth(acesso)
        .timeout(Duration::from_secs(60))
        .send()
        .await
        .map_err(sem_conexao)?;
    match r.status().as_u16() {
        200 => {
            let v: Value = serde_json::from_str(&r.text().await.map_err(sem_conexao)?).unwrap_or(Value::Null);
            Ok(v.get("trashed").and_then(Value::as_bool) != Some(true))
        }
        404 => Ok(false),
        c => Err(erro_da(c, &r.text().await.unwrap_or_default())),
    }
}

/// Troca o texto do documento que já está lá pelo .docx daqui. `Ok(false)` quando o documento não existe mais.
async fn substituir(http: &reqwest::Client, base: &str, acesso: &str, id: &str, docx: &[u8]) -> Result<bool, String> {
    let r = http
        .patch(format!("{base}/upload/drive/v3/files/{id}?uploadType=media&fields=id"))
        .bearer_auth(acesso)
        .header("Content-Type", DOCX_MIME)
        .timeout(Duration::from_secs(120))
        .body(docx.to_vec())
        .send()
        .await
        .map_err(sem_conexao)?;
    match r.status().as_u16() {
        200 => Ok(true),
        404 => Ok(false),
        c => Err(erro_da(c, &r.text().await.unwrap_or_default())),
    }
}

/// Manda a minuta ao Google Docs. Sem `existente`, cria um documento novo. Com `existente`: se `trocar`, o texto daqui
/// substitui o de lá; senão, o de lá fica como está (só é aberto). Se o documento de antes sumiu (apagado, na lixeira),
/// cria um novo.
pub async fn enviar(http: &reqwest::Client, base: &str, acesso: &str, nome: &str, docx: &[u8], existente: Option<&str>, trocar: bool) -> Result<NoDocs, String> {
    if docx.is_empty() || docx.len() > MAX_BYTES {
        return Err("A minuta está vazia ou é grande demais para o Google Docs.".into());
    }
    if let Some(id) = existente.filter(|i| id_valido(i)) {
        if existe(http, base, acesso, id).await? {
            if !trocar {
                return Ok(NoDocs { id: id.to_string(), link: link_do(id), feito: "aberto".into() });
            }
            if substituir(http, base, acesso, id, docx).await? {
                return Ok(NoDocs { id: id.to_string(), link: link_do(id), feito: "substituido".into() });
            }
        }
    }
    let id = criar(http, base, acesso, nome, docx).await?;
    Ok(NoDocs { link: link_do(&id), id, feito: "criado".into() })
}

#[cfg(test)]
mod testes {
    use super::*;
    use std::io::{Read, Write};
    use std::sync::{Arc, Mutex};

    /// Um Drive de mentira: guarda os pedidos e responde conforme o caminho. `sumido` é o id que ele diz não existir.
    fn drive_de_mentira(sumido: &'static str, na_lixeira: &'static str) -> (String, Arc<Mutex<Vec<Vec<u8>>>>) {
        let l = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
        let base = format!("http://{}", l.local_addr().unwrap());
        let pedidos = Arc::new(Mutex::new(Vec::<Vec<u8>>::new()));
        let guardados = pedidos.clone();
        std::thread::spawn(move || {
            for s in l.incoming() {
                let mut s = s.unwrap();
                let mut tudo = Vec::new();
                let mut b = [0u8; 16384];
                loop {
                    let n = s.read(&mut b).unwrap_or(0);
                    if n == 0 {
                        break;
                    }
                    tudo.extend_from_slice(&b[..n]);
                    let txt = String::from_utf8_lossy(&tudo).to_string();
                    if let Some(fim) = txt.find("\r\n\r\n") {
                        let tam = txt.to_lowercase().lines().find_map(|l| l.strip_prefix("content-length: ").map(|v| v.trim().parse::<usize>().unwrap_or(0))).unwrap_or(0);
                        if tudo.len() >= txt[..fim].len() + 4 + tam {
                            break;
                        }
                    }
                }
                let linha = String::from_utf8_lossy(&tudo).lines().next().unwrap_or("").to_string();
                guardados.lock().unwrap().push(tudo);
                let (codigo, corpo) = if linha.contains(sumido) && !sumido.is_empty() {
                    (404, r#"{"error":{"code":404}}"#.to_string())
                } else if linha.starts_with("GET") {
                    (200, format!(r#"{{"id":"docJaEnviado0001","trashed":{}}}"#, !na_lixeira.is_empty() && linha.contains(na_lixeira)))
                } else if linha.starts_with("PATCH") {
                    (200, r#"{"id":"docJaEnviado0001"}"#.to_string())
                } else if linha.contains("semPermissao") {
                    (403, r#"{"error":{"status":"PERMISSION_DENIED","details":[{"reason":"ACCESS_TOKEN_SCOPE_INSUFFICIENT"}]}}"#.to_string())
                } else {
                    (200, r#"{"id":"docNovoCriado0002"}"#.to_string())
                };
                let _ = s.write_all(format!("HTTP/1.1 {codigo} X\r\ncontent-type: application/json\r\ncontent-length: {}\r\nconnection: close\r\n\r\n{corpo}", corpo.len()).as_bytes());
            }
        });
        (base, pedidos)
    }

    #[test]
    fn o_id_e_o_link() {
        assert!(id_valido("1AbC-dEf_0123456789"));
        assert!(!id_valido("curto") && !id_valido("com/barra/0123456789") && !id_valido("com espaço 0123456789") && !id_valido("x?y=0123456789"));
        assert_eq!(link_do("docNovoCriado0002"), "https://docs.google.com/document/d/docNovoCriado0002/edit");
    }

    #[test]
    fn o_corpo_tem_os_dados_e_o_docx() {
        let c = corpo_em_partes("LIM", "Minuta de prova", b"PK\x03\x04bytes");
        let t = String::from_utf8_lossy(&c);
        assert!(t.starts_with("--LIM\r\nContent-Type: application/json; charset=UTF-8\r\n\r\n{"));
        assert!(t.contains(r#""name":"Minuta de prova""#) && t.contains(r#""mimeType":"application/vnd.google-apps.document""#));
        assert!(t.contains("Content-Type: application/vnd.openxmlformats-officedocument.wordprocessingml.document\r\n\r\nPK"));
        assert!(t.ends_with("bytes\r\n--LIM--\r\n"));
    }

    #[tokio::test]
    async fn cria_o_documento_novo() {
        let (base, pedidos) = drive_de_mentira("", "");
        let r = enviar(&reqwest::Client::new(), &base, "acesso-de-prova", "Minuta de prova", b"PK-docx-de-prova", None, false).await.unwrap();
        assert_eq!(r, NoDocs { id: "docNovoCriado0002".into(), link: "https://docs.google.com/document/d/docNovoCriado0002/edit".into(), feito: "criado".into() });
        let p = pedidos.lock().unwrap();
        let t = String::from_utf8_lossy(&p[0]).to_string();
        assert_eq!(p.len(), 1);
        assert!(t.starts_with("POST /upload/drive/v3/files?uploadType=multipart&fields=id "));
        assert!(t.to_lowercase().contains("authorization: bearer acesso-de-prova") && t.to_lowercase().contains("content-type: multipart/related; boundary=incognitus"));
        assert!(t.contains("Minuta de prova") && t.contains("PK-docx-de-prova"));
    }

    #[tokio::test]
    async fn o_que_ja_esta_la_e_aberto_ou_substituido() {
        let (base, pedidos) = drive_de_mentira("", "");
        let cli = reqwest::Client::new();
        let a = enviar(&cli, &base, "a", "Minuta", b"PK-1", Some("docJaEnviado0001"), false).await.unwrap();
        assert_eq!(a.feito, "aberto");
        assert_eq!(pedidos.lock().unwrap().len(), 1, "só a conferência: nada sobe");
        let s = enviar(&cli, &base, "a", "Minuta", b"PK-2", Some("docJaEnviado0001"), true).await.unwrap();
        assert_eq!((s.feito.as_str(), s.id.as_str()), ("substituido", "docJaEnviado0001"));
        let p = pedidos.lock().unwrap();
        let t = String::from_utf8_lossy(&p[2]).to_string();
        assert!(t.starts_with("PATCH /upload/drive/v3/files/docJaEnviado0001?uploadType=media&fields=id ") && t.ends_with("PK-2"));
    }

    #[tokio::test]
    async fn o_que_sumiu_ou_foi_para_a_lixeira_vira_documento_novo() {
        let (base, _) = drive_de_mentira("docApagado000003", "");
        let r = enviar(&reqwest::Client::new(), &base, "a", "Minuta", b"PK", Some("docApagado000003"), false).await.unwrap();
        assert_eq!((r.feito.as_str(), r.id.as_str()), ("criado", "docNovoCriado0002"));
        let (base, _) = drive_de_mentira("", "docNaLixeira0004");
        let r = enviar(&reqwest::Client::new(), &base, "a", "Minuta", b"PK", Some("docNaLixeira0004"), true).await.unwrap();
        assert_eq!(r.feito, "criado");
    }

    #[tokio::test]
    async fn sem_a_permissao_o_erro_diz_o_que_fazer() {
        let (base, _) = drive_de_mentira("", "");
        // o nome vai no corpo; o de mentira recusa pela palavra na linha do pedido, então a base leva a marca
        let e = enviar(&reqwest::Client::new(), &format!("{base}/semPermissao"), "a", "Minuta", b"PK", None, false).await.unwrap_err();
        assert_eq!(e, ERRO_SEM_ESCRITA);
        assert!(enviar(&reqwest::Client::new(), &base, "a", "Minuta", b"", None, false).await.is_err());
    }
}
