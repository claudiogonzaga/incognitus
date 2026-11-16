//! O login do Google do Incognitus, para ler o prompt do 🧠 Melhor Próximo Ato guardado num Google Docs (vem do login
//! do Graphé Desktop, src-tauri/src/google.rs, de 15/11/2026). É o login de "app para computador" do Google: OAuth 2.0
//! com PKCE e volta em http://127.0.0.1. O app abre o navegador do sistema, a pessoa escolhe a conta, e o Google devolve
//! um código a um porteiro local que só existe durante o login. O app troca o código pelo acesso (o access_token, que
//! vence em 1 hora) e pela renovação (o refresh_token, que fica no chaveiro do sistema).
//!
//! O app pede ao Google `openid email` (quem entrou) e `documents.readonly` (ler os Google Docs que a pessoa já pode
//! abrir): nada do Drive, das planilhas ou do Gmail, e nada que grave. Nem o acesso nem a renovação vão a arquivo, a log
//! ou à janela.
use base64::Engine;
use serde_json::Value;
use sha2::{Digest, Sha256};
use std::collections::HashMap;
use std::io::{Read, Write};
use std::net::{TcpListener, TcpStream};
use std::path::Path;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

/// A conta da renovação do login no chaveiro (o serviço é o do app, chaveiro::SERVICO).
pub const CONTA_RENOVACAO: &str = "google-renovacao";
/// O arquivo do cliente OAuth na pasta de dados do app (o JSON que o Google Cloud dá para baixar), que vale mais que o
/// cliente compilado no app.
pub const ARQUIVO_CLIENTE: &str = "google_cliente.json";
/// A prova do app montado (scripts/prova_espelho.mjs): o endereço de um Google de mentira, em http://127.0.0.1.
pub const PROVA_VAR: &str = "SKIA_PROVA_GOOGLE";
pub const CLIENTE_DE_PROVA: &str = "cliente-de-prova.apps.googleusercontent.com";
pub const SEGREDO_DE_PROVA: &str = "segredo-de-prova";

pub const ESCOPO: &str = "openid email https://www.googleapis.com/auth/documents.readonly";
const PRAZO_DO_LOGIN: Duration = Duration::from_secs(300);
/// A prova é renovada quando faltam menos de 5 minutos para ela vencer.
const FOLGA: u64 = 300;
const B64: base64::engine::GeneralPurpose = base64::engine::general_purpose::URL_SAFE_NO_PAD;

pub const SEM_CLIENTE: &str = "O login do Google ainda não está configurado nesta cópia do app (falta o cliente OAuth do Incognitus).";
pub const VENCIDO: &str = "O login do Google venceu ou foi retirado: entre de novo nos Ajustes (🧠 Melhor Próximo Ato).";
pub const CANCELADO: &str = "O login do Google foi cancelado.";

#[derive(Clone, Debug, PartialEq)]
pub struct Cliente {
    pub id: String,
    pub segredo: String,
}

#[derive(Clone, Debug)]
pub struct Enderecos {
    pub autorizar: String,
    pub trocar: String,
    pub revogar: String,
}

impl Enderecos {
    pub fn do_google() -> Self {
        Enderecos {
            autorizar: "https://accounts.google.com/o/oauth2/v2/auth".into(),
            trocar: "https://oauth2.googleapis.com/token".into(),
            revogar: "https://oauth2.googleapis.com/revoke".into(),
        }
    }
    pub fn de_mentira(base: &str) -> Self {
        let b = base.trim_end_matches('/');
        Enderecos { autorizar: format!("{b}/o/oauth2/v2/auth"), trocar: format!("{b}/token"), revogar: format!("{b}/revoke") }
    }
}

pub fn modo_prova() -> Option<String> {
    std::env::var(PROVA_VAR).ok().filter(|v| v.starts_with("http://127.0.0.1:"))
}

pub fn enderecos() -> Enderecos {
    modo_prova().map(|b| Enderecos::de_mentira(&b)).unwrap_or_else(Enderecos::do_google)
}

/// O cliente OAuth do Incognitus. Na prova, o de mentira; fora dela, o arquivo google_cliente.json da pasta de dados
/// e, sem ele, o que veio compilado no app (INCOGNITUS_GOOGLE_CLIENT_ID e _SECRET no ambiente do build, fora do git).
pub fn cliente(dados: &Path) -> Option<Cliente> {
    if modo_prova().is_some() {
        return Some(Cliente { id: CLIENTE_DE_PROVA.into(), segredo: SEGREDO_DE_PROVA.into() });
    }
    if let Ok(c) = std::fs::read_to_string(dados.join(ARQUIVO_CLIENTE)).map_err(|_| ()).and_then(|t| ler_cliente_json(&t).map_err(|_| ())) {
        return Some(c);
    }
    match (option_env!("INCOGNITUS_GOOGLE_CLIENT_ID"), option_env!("INCOGNITUS_GOOGLE_CLIENT_SECRET")) {
        (Some(id), Some(s)) if !id.is_empty() && !s.is_empty() => Some(Cliente { id: id.into(), segredo: s.into() }),
        _ => None,
    }
}

/// O JSON que o Google Cloud dá para baixar ao criar o cliente "App para computador" ({"installed": {...}}), ou só o
/// {client_id, client_secret}.
pub fn ler_cliente_json(texto: &str) -> Result<Cliente, String> {
    let v: Value = serde_json::from_str(texto).map_err(|_| "o arquivo do cliente OAuth não é JSON".to_string())?;
    if v.get("web").is_some() {
        return Err("este cliente OAuth é do tipo \"Aplicativo da Web\"; o Incognitus precisa de um do tipo \"App para computador\"".into());
    }
    let o = v.get("installed").unwrap_or(&v);
    let campo = |n: &str| o.get(n).and_then(Value::as_str).unwrap_or("").trim().to_string();
    let (id, segredo) = (campo("client_id"), campo("client_secret"));
    if !id.ends_with(".apps.googleusercontent.com") || segredo.is_empty() {
        return Err("o arquivo não traz o client_id e o client_secret de um cliente OAuth do Google".into());
    }
    Ok(Cliente { id, segredo })
}

fn agora() -> u64 {
    SystemTime::now().duration_since(UNIX_EPOCH).map(|d| d.as_secs()).unwrap_or(0)
}

fn aleatorio(bytes: usize) -> Result<String, String> {
    let mut b = vec![0u8; bytes];
    getrandom::getrandom(&mut b).map_err(|e| format!("o sistema não deu números aleatórios: {e}"))?;
    Ok(B64.encode(b))
}

/// O desafio do PKCE (RFC 7636, S256): o SHA-256 do verificador, em base64url sem "=".
pub fn desafio(verificador: &str) -> String {
    B64.encode(Sha256::digest(verificador.as_bytes()))
}

pub fn endereco_de_autorizacao(ep: &Enderecos, cli: &Cliente, retorno: &str, desafio: &str, estado: &str) -> String {
    let mut u = reqwest::Url::parse(&ep.autorizar).expect("o endereço de autorização é fixo");
    u.query_pairs_mut()
        .append_pair("client_id", &cli.id)
        .append_pair("redirect_uri", retorno)
        .append_pair("response_type", "code")
        .append_pair("scope", ESCOPO)
        .append_pair("code_challenge", desafio)
        .append_pair("code_challenge_method", "S256")
        .append_pair("state", estado)
        .append_pair("access_type", "offline")
        // "consent": o Google só manda a renovação quando a pessoa consente; sem ele, o segundo login viria sem ela
        .append_pair("prompt", "consent select_account");
    u.to_string()
}

// ── o porteiro local: a volta do navegador ─────────────────────────────────────────────────────────────────────────

fn pagina(titulo: &str, texto: &str) -> String {
    format!(
        "<!doctype html><html lang=\"pt-BR\"><meta charset=\"utf-8\"><title>{titulo}</title>\
         <body style=\"font:16px/1.5 -apple-system,Segoe UI,sans-serif;max-width:34em;margin:4em auto;padding:0 1em;color:#222\">\
         <h1 style=\"font-size:1.3em\">{titulo}</h1><p>{texto}</p></body></html>"
    )
}

fn responder(s: &mut TcpStream, codigo: u16, html: &str) {
    let _ = s.write_all(
        format!("HTTP/1.1 {codigo} X\r\ncontent-type: text/html; charset=utf-8\r\ncache-control: no-store\r\ncontent-length: {}\r\nconnection: close\r\n\r\n{html}", html.len())
            .as_bytes(),
    );
}

/// Uma visita ao porteiro. None: não era a volta deste login (o ícone da aba, um endereço qualquer, outro estado); o
/// porteiro segue esperando.
fn atender(mut s: TcpStream, estado: &str) -> Option<Result<String, String>> {
    let _ = s.set_nonblocking(false);
    let _ = s.set_read_timeout(Some(Duration::from_secs(5)));
    let mut dados = Vec::new();
    let mut b = [0u8; 4096];
    while let Ok(n) = s.read(&mut b) {
        if n == 0 {
            break;
        }
        dados.extend_from_slice(&b[..n]);
        if dados.windows(4).any(|w| w == b"\r\n\r\n") || dados.len() > 16384 {
            break;
        }
    }
    let texto = String::from_utf8_lossy(&dados);
    let mut partes = texto.lines().next().unwrap_or("").split_whitespace();
    let (metodo, alvo) = (partes.next().unwrap_or(""), partes.next().unwrap_or(""));
    let u = reqwest::Url::parse(&format!("http://127.0.0.1{alvo}")).ok().filter(|u| metodo == "GET" && u.path() == "/");
    let Some(u) = u else {
        responder(&mut s, 404, &pagina("Incognitus", "Nada aqui."));
        return None;
    };
    let q: HashMap<String, String> = u.query_pairs().into_owned().collect();
    if q.get("state").map(String::as_str) != Some(estado) {
        responder(&mut s, 400, &pagina("Este login não é o de agora", "Volte ao Incognitus e clique de novo em Entrar com o Google."));
        return None;
    }
    if let Some(e) = q.get("error") {
        responder(&mut s, 200, &pagina("O login não terminou", "Volte ao Incognitus. Pode fechar esta aba."));
        return Some(Err(if e == "access_denied" { "Você cancelou o login no Google.".to_string() } else { format!("O Google recusou o login ({e}).") }));
    }
    match q.get("code").filter(|c| !c.is_empty()) {
        Some(c) => {
            responder(&mut s, 200, &pagina("Pronto", "Você entrou no Incognitus com a sua conta do Google. Pode fechar esta aba e voltar ao app."));
            Some(Ok(c.clone()))
        }
        None => {
            responder(&mut s, 400, &pagina("O login não terminou", "O Google voltou sem o código. Volte ao Incognitus e tente de novo."));
            None
        }
    }
}

/// Espera o navegador voltar com o código, até o prazo ou até o login ser cancelado.
pub fn esperar_retorno(l: &TcpListener, estado: &str, prazo: Duration, cancelado: &AtomicBool) -> Result<String, String> {
    l.set_nonblocking(true).map_err(|e| format!("o porteiro do login falhou: {e}"))?;
    let fim = Instant::now() + prazo;
    loop {
        if cancelado.load(Ordering::SeqCst) {
            return Err(CANCELADO.into());
        }
        if Instant::now() > fim {
            return Err("O login do Google não voltou em 5 minutos. Clique de novo em Entrar com o Google.".into());
        }
        match l.accept() {
            Ok((s, _)) => {
                if let Some(r) = atender(s, estado) {
                    return r;
                }
            }
            Err(e) if e.kind() == std::io::ErrorKind::WouldBlock => std::thread::sleep(Duration::from_millis(120)),
            Err(e) => return Err(format!("o porteiro do login falhou: {e}")),
        }
    }
}

// ── a prova do login e a renovação ─────────────────────────────────────────────────────────────────────────────────

/// A prova do login (o id_token do Google, de onde sai o e-mail) e o acesso (o access_token, que vai à API do Docs).
#[derive(Clone, Debug)]
#[allow(dead_code)] // a prova inteira e o vencimento dela ficam como no Graphé Desktop (lá a prova vai ao WebApp)
pub struct Prova {
    pub texto: String,
    pub email: String,
    pub exp: u64,
    pub acesso: String,
    pub acesso_exp: u64,
}

impl Prova {
    /// A prova ainda vale por mais de 5 minutos.
    #[allow(dead_code)]
    pub fn vale(&self) -> bool {
        self.exp > agora() + FOLGA
    }
    /// O acesso ainda vale por mais de 5 minutos.
    pub fn acesso_vale(&self) -> bool {
        !self.acesso.is_empty() && self.acesso_exp > agora() + FOLGA
    }
}

/// A prova e o acesso da resposta do /token.
fn prova_da_resposta(v: &Value) -> Result<Prova, String> {
    let mut p = ler_prova(v.get("id_token").and_then(Value::as_str).unwrap_or(""))?;
    p.acesso = v.get("access_token").and_then(Value::as_str).unwrap_or("").to_string();
    p.acesso_exp = agora() + v.get("expires_in").and_then(Value::as_u64).unwrap_or(0);
    if p.acesso.is_empty() {
        return Err("o Google não devolveu o acesso (access_token)".into());
    }
    Ok(p)
}

/// Lê o e-mail e o vencimento da prova. A assinatura não é conferida aqui: a prova veio direto do Google, pela conexão
/// cifrada (OpenID Connect, 3.1.3.7); o app só usa o e-mail para mostrar quem entrou.
pub fn ler_prova(jwt: &str) -> Result<Prova, String> {
    let meio = jwt.split('.').nth(1).filter(|m| !m.is_empty()).ok_or("o Google não devolveu a prova do login (id_token)")?;
    let bytes = B64.decode(meio.trim_end_matches('=')).map_err(|_| "a prova do login do Google veio estragada".to_string())?;
    let v: Value = serde_json::from_slice(&bytes).map_err(|_| "a prova do login do Google veio estragada".to_string())?;
    let email = v.get("email").and_then(Value::as_str).unwrap_or("").trim().to_lowercase();
    let verificado = v.get("email_verified").map_or(false, |x| x == &Value::Bool(true) || x == "true");
    if email.is_empty() {
        return Err("o login do Google não trouxe o e-mail da conta".into());
    }
    if !verificado {
        return Err("o e-mail desta conta do Google não está confirmado".into());
    }
    Ok(Prova { texto: jwt.to_string(), email, exp: v.get("exp").and_then(Value::as_u64).unwrap_or(0), acesso: String::new(), acesso_exp: 0 })
}

/// O que o login entrega: a prova da vez e a renovação (que vai ao chaveiro).
pub struct Entrada {
    pub prova: Prova,
    pub renovacao: String,
}

fn sem_conexao(e: &reqwest::Error) -> String {
    let m = if e.is_timeout() { "tempo esgotado".to_string() } else if e.is_connect() { "a rede não respondeu".to_string() } else { e.to_string() };
    format!("Sem conexão com o Google ({m}).")
}

async fn pedir(http: &reqwest::Client, url: &str, form: &[(&str, &str)]) -> Result<Value, String> {
    let r = http.post(url).timeout(Duration::from_secs(30)).form(form).send().await.map_err(|e| sem_conexao(&e))?;
    let codigo = r.status();
    let texto = r.text().await.map_err(|e| sem_conexao(&e))?;
    let v: Value = serde_json::from_str(&texto).unwrap_or(Value::Null);
    if !codigo.is_success() {
        let e = v.get("error").and_then(Value::as_str).unwrap_or("");
        return Err(match e {
            "invalid_grant" => VENCIDO.to_string(),
            "invalid_client" | "unauthorized_client" => "O Google recusou o cliente OAuth do app (apagado ou trocado no Google Cloud).".to_string(),
            _ => format!("O Google recusou o login (código {}{}).", codigo.as_u16(), if e.is_empty() { String::new() } else { format!(", {e}") }),
        });
    }
    Ok(v)
}

pub async fn trocar_codigo(http: &reqwest::Client, ep: &Enderecos, cli: &Cliente, codigo: &str, verificador: &str, retorno: &str) -> Result<Entrada, String> {
    let v = pedir(
        http,
        &ep.trocar,
        &[
            ("grant_type", "authorization_code"),
            ("code", codigo),
            ("client_id", &cli.id),
            ("client_secret", &cli.segredo),
            ("redirect_uri", retorno),
            ("code_verifier", verificador),
        ],
    )
    .await?;
    let prova = prova_da_resposta(&v)?;
    let renovacao = v
        .get("refresh_token")
        .and_then(Value::as_str)
        .filter(|s| !s.is_empty())
        .ok_or("o Google não devolveu a renovação do login: clique de novo em Entrar com o Google")?;
    Ok(Entrada { prova, renovacao: renovacao.to_string() })
}

/// Uma prova nova, pela renovação. VENCIDO quando o Google não a aceita mais (a pessoa tirou o acesso, trocou a senha).
pub async fn renovar(http: &reqwest::Client, ep: &Enderecos, cli: &Cliente, renovacao: &str) -> Result<Prova, String> {
    let v = pedir(http, &ep.trocar, &[("grant_type", "refresh_token"), ("refresh_token", renovacao), ("client_id", &cli.id), ("client_secret", &cli.segredo)]).await?;
    prova_da_resposta(&v)
}

/// Tira do Google o acesso do app (ao sair). Sem rede, fica para o Google vencer sozinho: não é erro.
pub async fn revogar(http: &reqwest::Client, ep: &Enderecos, renovacao: &str) {
    let _ = http.post(&ep.revogar).timeout(Duration::from_secs(10)).form(&[("token", renovacao)]).send().await;
}

/// O login inteiro: o porteiro em 127.0.0.1, o navegador (pela função `abrir`), a volta e a troca do código.
pub async fn entrar<F>(http: &reqwest::Client, ep: &Enderecos, cli: &Cliente, abrir: F, cancelado: Arc<AtomicBool>) -> Result<Entrada, String>
where
    F: FnOnce(&str) -> Result<(), String>,
{
    let l = TcpListener::bind("127.0.0.1:0").map_err(|e| format!("não consegui abrir o porteiro do login: {e}"))?;
    let porta = l.local_addr().map_err(|e| format!("não consegui abrir o porteiro do login: {e}"))?.port();
    let retorno = format!("http://127.0.0.1:{porta}");
    let verificador = aleatorio(48)?;
    let estado = aleatorio(18)?;
    abrir(&endereco_de_autorizacao(ep, cli, &retorno, &desafio(&verificador), &estado))?;
    let codigo = tauri::async_runtime::spawn_blocking(move || esperar_retorno(&l, &estado, PRAZO_DO_LOGIN, &cancelado))
        .await
        .map_err(|e| format!("o porteiro do login parou: {e}"))??;
    trocar_codigo(http, ep, cli, &codigo, &verificador, &retorno).await
}

#[cfg(test)]
mod testes {
    use super::*;
    use std::sync::Mutex;

    fn rt() -> tokio::runtime::Runtime {
        tokio::runtime::Builder::new_current_thread().enable_all().build().unwrap()
    }

    /// Uma prova de mentira, no formato do id_token (cabeçalho.corpo.assinatura).
    fn jwt(email: &str, exp: u64) -> String {
        let corpo = serde_json::json!({ "iss": "https://accounts.google.com", "aud": CLIENTE_DE_PROVA, "email": email, "email_verified": true, "exp": exp });
        format!("{}.{}.assinatura-de-mentira", B64.encode(br#"{"alg":"RS256"}"#), B64.encode(corpo.to_string()))
    }

    /// O /token de um Google de mentira: devolve as respostas na ordem e guarda os formulários recebidos.
    fn google(respostas: Vec<(u16, String)>) -> (Enderecos, Arc<Mutex<Vec<HashMap<String, String>>>>) {
        let l = TcpListener::bind("127.0.0.1:0").unwrap();
        let base = format!("http://{}", l.local_addr().unwrap());
        let forms = Arc::new(Mutex::new(vec![]));
        let guardados = forms.clone();
        std::thread::spawn(move || {
            let mut fila = respostas.into_iter();
            for s in l.incoming() {
                let mut s = s.unwrap();
                let mut dados = vec![];
                let mut b = [0u8; 8192];
                let corpo = loop {
                    let n = s.read(&mut b).unwrap();
                    dados.extend_from_slice(&b[..n]);
                    if let Some(i) = dados.windows(4).position(|w| w == b"\r\n\r\n") {
                        let cab = String::from_utf8_lossy(&dados[..i]).to_ascii_lowercase();
                        let tam = cab.lines().find_map(|l| l.strip_prefix("content-length:").map(|v| v.trim().parse::<usize>().unwrap())).unwrap_or(0);
                        while dados.len() < i + 4 + tam {
                            let n = s.read(&mut b).unwrap();
                            dados.extend_from_slice(&b[..n]);
                        }
                        break String::from_utf8_lossy(&dados[i + 4..i + 4 + tam]).to_string();
                    }
                };
                guardados.lock().unwrap().push(reqwest::Url::parse(&format!("http://x/?{corpo}")).unwrap().query_pairs().into_owned().collect());
                let (code, r) = fila.next().unwrap_or((500, "{}".into()));
                let _ = s.write_all(format!("HTTP/1.1 {code} X\r\ncontent-type: application/json\r\ncontent-length: {}\r\nconnection: close\r\n\r\n{r}", r.len()).as_bytes());
            }
        });
        (Enderecos::de_mentira(&base), forms)
    }

    fn cli() -> Cliente {
        Cliente { id: CLIENTE_DE_PROVA.into(), segredo: SEGREDO_DE_PROVA.into() }
    }

    #[test]
    fn o_desafio_do_pkce_e_o_da_rfc_7636() {
        assert_eq!(desafio("dBjftJeZ4CVP-mB92K27uhbUJU1p1r_wW1gFWFOEjXk"), "E9Melhoa2OwvFrEMTJguCHaoeK1t8URWbuGJSstw-cM");
        let v = aleatorio(48).unwrap();
        assert_eq!(v.len(), 64, "o verificador tem de ter de 43 a 128 caracteres");
        assert!(v.chars().all(|c| c.is_ascii_alphanumeric() || c == '-' || c == '_'));
        assert_ne!(v, aleatorio(48).unwrap());
    }

    #[test]
    fn le_o_arquivo_do_cliente_que_o_google_da_para_baixar() {
        let baixado = r#"{"installed":{"client_id":"123-abc.apps.googleusercontent.com","project_id":"p","client_secret":"segredo-x","redirect_uris":["http://localhost"]}}"#;
        assert_eq!(ler_cliente_json(baixado).unwrap(), Cliente { id: "123-abc.apps.googleusercontent.com".into(), segredo: "segredo-x".into() });
        assert!(ler_cliente_json(r#"{"client_id":"1.apps.googleusercontent.com","client_secret":"s"}"#).is_ok());
        assert!(ler_cliente_json(r#"{"web":{"client_id":"1.apps.googleusercontent.com","client_secret":"s"}}"#).unwrap_err().contains("App para computador"));
        assert!(ler_cliente_json(r#"{"installed":{"client_id":"1.apps.googleusercontent.com"}}"#).is_err());
        assert!(ler_cliente_json("não é json").is_err());
    }

    #[test]
    fn o_endereco_de_autorizacao_pede_o_email_e_so_a_leitura_dos_docs_e_nao_leva_o_segredo() {
        let u = endereco_de_autorizacao(&Enderecos::do_google(), &cli(), "http://127.0.0.1:5555", "desafio", "estado");
        let u = reqwest::Url::parse(&u).unwrap();
        assert_eq!(u.host_str(), Some("accounts.google.com"));
        let q: HashMap<String, String> = u.query_pairs().into_owned().collect();
        assert_eq!(q["scope"], "openid email https://www.googleapis.com/auth/documents.readonly");
        assert!(!q["scope"].contains("drive") && !q["scope"].contains("spreadsheets"));
        assert_eq!(q["code_challenge_method"], "S256");
        assert_eq!(q["redirect_uri"], "http://127.0.0.1:5555");
        assert!(!u.as_str().contains(SEGREDO_DE_PROVA));
    }

    #[test]
    fn le_a_prova_do_login() {
        let p = ler_prova(&jwt("Pessoa@Example.Invalid", agora() + 3600)).unwrap();
        assert_eq!(p.email, "pessoa@example.invalid");
        assert!(p.vale());
        assert!(!ler_prova(&jwt("pessoa@example.invalid", agora() + 60)).unwrap().vale(), "a 1 minuto de vencer, renova");
        assert!(ler_prova("sem-pontos").is_err());
        let sem_confirmacao = format!("x.{}.y", B64.encode(br#"{"email":"a@example.invalid","email_verified":false,"exp":1}"#));
        assert!(ler_prova(&sem_confirmacao).unwrap_err().contains("não está confirmado"));
    }

    #[test]
    fn o_login_inteiro_com_o_navegador_de_mentira() {
        let resposta = serde_json::json!({ "id_token": jwt("assessora@example.invalid", agora() + 3600), "access_token": "acesso-1", "refresh_token": "renovacao-1", "expires_in": 3599 }).to_string();
        let (ep, forms) = google(vec![(200, resposta)]);
        // o "navegador": vai ao endereço de volta com o código e o estado do pedido
        let abrir = |url: &str| -> Result<(), String> {
            let u = reqwest::Url::parse(url).unwrap();
            let q: HashMap<String, String> = u.query_pairs().into_owned().collect();
            let volta = format!("{}/?favicon=1", q["redirect_uri"]);
            let certo = format!("{}/?state={}&code=codigo-1", q["redirect_uri"], q["state"]);
            std::thread::spawn(move || {
                let r = rt();
                let c = reqwest::Client::new();
                // primeiro uma visita que não é a volta (outro estado): o porteiro segue esperando
                let _ = r.block_on(c.get(volta.replace("favicon=1", "state=outro&code=x")).send());
                let pagina = r.block_on(async { c.get(certo).send().await.unwrap().text().await.unwrap() });
                assert!(pagina.contains("Pronto"));
            });
            Ok(())
        };
        let e = rt().block_on(entrar(&reqwest::Client::new(), &ep, &cli(), abrir, Arc::new(AtomicBool::new(false)))).unwrap();
        assert_eq!(e.prova.email, "assessora@example.invalid");
        assert_eq!(e.renovacao, "renovacao-1");
        assert_eq!(e.prova.acesso, "acesso-1");
        assert!(e.prova.acesso_vale());
        let f = &forms.lock().unwrap()[0];
        assert_eq!(f["grant_type"], "authorization_code");
        assert_eq!(f["code"], "codigo-1");
        assert_eq!(f["client_secret"], SEGREDO_DE_PROVA);
        assert_eq!(f["code_verifier"].len(), 64);
        assert!(f["redirect_uri"].starts_with("http://127.0.0.1:"));
    }

    #[test]
    fn o_login_cancelado_e_o_recusado() {
        let (ep, _) = google(vec![]);
        let cancelado = Arc::new(AtomicBool::new(false));
        let c2 = cancelado.clone();
        let abrir = move |_: &str| -> Result<(), String> {
            std::thread::spawn(move || {
                std::thread::sleep(Duration::from_millis(300));
                c2.store(true, Ordering::SeqCst);
            });
            Ok(())
        };
        let r = rt().block_on(entrar(&reqwest::Client::new(), &ep, &cli(), abrir, cancelado));
        assert_eq!(r.err().unwrap(), CANCELADO);
        // a pessoa clicou em "Cancelar" na página do Google
        let abrir = |url: &str| -> Result<(), String> {
            let q: HashMap<String, String> = reqwest::Url::parse(url).unwrap().query_pairs().into_owned().collect();
            let volta = format!("{}/?state={}&error=access_denied", q["redirect_uri"], q["state"]);
            std::thread::spawn(move || {
                let _ = rt().block_on(reqwest::Client::new().get(volta).send());
            });
            Ok(())
        };
        let r = rt().block_on(entrar(&reqwest::Client::new(), &ep, &cli(), abrir, Arc::new(AtomicBool::new(false))));
        assert!(r.err().unwrap().contains("cancelou"));
    }

    #[test]
    fn renova_e_reconhece_o_login_vencido() {
        let nova = serde_json::json!({ "id_token": jwt("assessora@example.invalid", agora() + 3600), "access_token": "acesso-2", "expires_in": 3599 }).to_string();
        let (ep, forms) = google(vec![(200, nova), (400, r#"{"error":"invalid_grant","error_description":"Token has been expired or revoked."}"#.into())]);
        let http = reqwest::Client::new();
        let p = rt().block_on(renovar(&http, &ep, &cli(), "renovacao-1")).unwrap();
        assert_eq!(p.email, "assessora@example.invalid");
        assert_eq!(p.acesso, "acesso-2");
        assert_eq!(forms.lock().unwrap()[0]["grant_type"], "refresh_token");
        assert_eq!(rt().block_on(renovar(&http, &ep, &cli(), "renovacao-1")).unwrap_err(), VENCIDO);
        let fechado = TcpListener::bind("127.0.0.1:0").unwrap();
        let morto = Enderecos::de_mentira(&format!("http://{}", fechado.local_addr().unwrap()));
        drop(fechado);
        assert!(rt().block_on(renovar(&http, &morto, &cli(), "r")).unwrap_err().starts_with("Sem conexão"), "a sincronização reconhece a falta de rede pelo começo");
    }
}
