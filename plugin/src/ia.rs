//! O 🧠 Melhor Próximo Ato do Incognitus, do lado do Rust: a chave da Anthropic (no chaveiro do sistema), o login do
//! Google (para ler o prompt guardado num Google Docs), a leitura do prompt e o envio à Anthropic.
//!
//! O que sai do computador, e só quando a pessoa clica no 🧠: o texto anonimizado do caso (o mesmo do _anon) e o prompt
//! que ela escolheu, para a Anthropic, com a chave dela. O mapa, os nomes reais e os PDFs nunca saem; antes de cada
//! envio a janela confere o pedido contra o mapa (o porteiro, modulo/porteiro.js) e barra o que tiver dado em claro.
//! A chave e a renovação do login ficam no chaveiro e não vão a arquivo, a log nem à janela.
use crate::{chaveiro, claude, google, prompt};
use serde::Serialize;
use std::collections::BTreeMap;
use std::path::PathBuf;
use std::sync::atomic::AtomicBool;
use std::sync::{Arc, Mutex};
use tauri::State;

const CONTA_EMAIL: &str = "google-email";

pub struct Ia {
    /// o serviço no chaveiro: o identificador do app avulso, com ".modulo" quando hospedado
    servico: String,
    dados: PathBuf,
    http: reqwest::Client,
    prova: Mutex<Option<google::Prova>>,
    cancelar: Mutex<Arc<AtomicBool>>,
}

#[derive(Clone)]
pub struct EstadoIa(pub Arc<Ia>);

pub fn novo(identificador: &str, hospedado: bool, dados: PathBuf) -> EstadoIa {
    let servico = if hospedado { format!("{identificador}.modulo") } else { identificador.to_string() };
    let http = claude::cliente().unwrap_or_default();
    EstadoIa(Arc::new(Ia { servico, dados, http, prova: Mutex::new(None), cancelar: Mutex::new(Arc::new(AtomicBool::new(false))) }))
}

/// Só no teste de ponta a ponta (SKIA_E2E): a chave e a Anthropic de mentira, sem tocar no chaveiro.
fn de_prova(var: &str) -> Option<String> {
    std::env::var_os("SKIA_E2E")?;
    std::env::var(var).ok().filter(|v| !v.is_empty())
}

async fn bloqueante<T: Send + 'static>(f: impl FnOnce() -> Result<T, String> + Send + 'static) -> Result<T, String> {
    tauri::async_runtime::spawn_blocking(f).await.map_err(|e| format!("tarefa interrompida: {e}"))?
}

fn abrir_no_navegador(url: &str) -> Result<(), String> {
    #[cfg(target_os = "macos")]
    let r = std::process::Command::new("open").arg(url).spawn();
    #[cfg(target_os = "windows")]
    let r = std::process::Command::new("rundll32").arg("url.dll,FileProtocolHandler").arg(url).spawn();
    #[cfg(not(any(target_os = "macos", target_os = "windows")))]
    let r = std::process::Command::new("xdg-open").arg(url).spawn();
    r.map(|_| ()).map_err(|e| format!("não consegui abrir o navegador: {e}"))
}

// ── a situação ──────────────────────────────────────────────────────────────────────────────────────────────────────

#[derive(Serialize)]
pub struct SituacaoChave {
    guardada: bool,
    fim: String,
}

#[derive(Serialize)]
pub struct SituacaoGoogle {
    /// o app tem o cliente OAuth (sem ele, não há login)
    configurado: bool,
    entrou: bool,
    email: String,
}

#[derive(Serialize)]
pub struct Situacao {
    chave: SituacaoChave,
    google: SituacaoGoogle,
}

#[tauri::command]
pub async fn ia_situacao(estado: State<'_, EstadoIa>) -> Result<Situacao, String> {
    let ia = estado.0.clone();
    let chave = match de_prova("SKIA_CHAVE_DE_PROVA") {
        Some(c) => Some(c),
        None => {
            let s = ia.servico.clone();
            bloqueante(move || chaveiro::ler(&s, chaveiro::CONTA_ANTHROPIC)).await?
        }
    };
    let configurado = google::cliente(&ia.dados).is_some();
    let (entrou, email) = if de_prova("SKIA_CHAVE_DE_PROVA").is_some() {
        (false, String::new())
    } else {
        let s = ia.servico.clone();
        bloqueante(move || {
            let r = chaveiro::ler(&s, google::CONTA_RENOVACAO)?;
            let e = chaveiro::ler(&s, CONTA_EMAIL)?.unwrap_or_default();
            Ok((r.is_some(), e))
        })
        .await?
    };
    Ok(Situacao {
        chave: SituacaoChave { guardada: chave.is_some(), fim: chave.map(|c| chaveiro::final_da_chave(&c)).unwrap_or_default() },
        google: SituacaoGoogle { configurado, entrou, email },
    })
}

// ── a chave da Anthropic ────────────────────────────────────────────────────────────────────────────────────────────

#[tauri::command]
pub async fn ia_chave_guardar(estado: State<'_, EstadoIa>, chave: String) -> Result<SituacaoChave, String> {
    let chave = chaveiro::conferir_chave_anthropic(&chave)?;
    let s = estado.0.servico.clone();
    bloqueante(move || {
        chaveiro::guardar(&s, chaveiro::CONTA_ANTHROPIC, &chave)?;
        Ok(SituacaoChave { guardada: true, fim: chaveiro::final_da_chave(&chave) })
    })
    .await
}

#[tauri::command]
pub async fn ia_chave_apagar(estado: State<'_, EstadoIa>) -> Result<(), String> {
    let s = estado.0.servico.clone();
    bloqueante(move || chaveiro::apagar(&s, chaveiro::CONTA_ANTHROPIC)).await
}

async fn chave(ia: &Ia) -> Result<String, String> {
    if let Some(c) = de_prova("SKIA_CHAVE_DE_PROVA") {
        return Ok(c);
    }
    let s = ia.servico.clone();
    bloqueante(move || chaveiro::ler(&s, chaveiro::CONTA_ANTHROPIC)).await?.ok_or_else(|| {
        "Falta a chave da Anthropic: guarde-a nos Ajustes (ela fica no chaveiro do sistema).".to_string()
    })
}

// ── o login do Google ───────────────────────────────────────────────────────────────────────────────────────────────

#[derive(Serialize)]
pub struct Entrou {
    email: String,
}

#[tauri::command]
pub async fn ia_google_entrar(estado: State<'_, EstadoIa>) -> Result<Entrou, String> {
    let ia = estado.0.clone();
    let cli = google::cliente(&ia.dados).ok_or(google::SEM_CLIENTE)?;
    let cancelado = Arc::new(AtomicBool::new(false));
    *ia.cancelar.lock().unwrap() = cancelado.clone();
    let e = google::entrar(&ia.http, &google::enderecos(), &cli, abrir_no_navegador, cancelado).await?;
    let (s, renovacao, email) = (ia.servico.clone(), e.renovacao.clone(), e.prova.email.clone());
    bloqueante(move || {
        chaveiro::guardar(&s, google::CONTA_RENOVACAO, &renovacao)?;
        chaveiro::guardar(&s, CONTA_EMAIL, &email)
    })
    .await?;
    let email = e.prova.email.clone();
    *ia.prova.lock().unwrap() = Some(e.prova);
    Ok(Entrou { email })
}

#[tauri::command]
pub fn ia_google_cancelar(estado: State<'_, EstadoIa>) {
    estado.0.cancelar.lock().unwrap().store(true, std::sync::atomic::Ordering::SeqCst);
}

#[tauri::command]
pub async fn ia_google_sair(estado: State<'_, EstadoIa>) -> Result<(), String> {
    let ia = estado.0.clone();
    *ia.prova.lock().unwrap() = None;
    let s = ia.servico.clone();
    let renovacao = bloqueante(move || chaveiro::ler(&s, google::CONTA_RENOVACAO)).await?;
    if let Some(r) = renovacao {
        google::revogar(&ia.http, &google::enderecos(), &r).await;
    }
    let s = ia.servico.clone();
    bloqueante(move || {
        chaveiro::apagar(&s, google::CONTA_RENOVACAO)?;
        chaveiro::apagar(&s, CONTA_EMAIL)
    })
    .await
}

/// O acesso ao Google da vez: o guardado, se ainda vale; senão, um novo pela renovação do chaveiro.
async fn acesso_google(ia: &Ia) -> Result<(String, String), String> {
    if let Some(p) = ia.prova.lock().unwrap().clone().filter(|p| p.acesso_vale()) {
        return Ok((p.acesso, p.email));
    }
    let cli = google::cliente(&ia.dados).ok_or(google::SEM_CLIENTE)?;
    let s = ia.servico.clone();
    let renovacao = bloqueante(move || chaveiro::ler(&s, google::CONTA_RENOVACAO)).await?.ok_or_else(|| {
        "Para ler o prompt do Google Docs, entre com o Google nos Ajustes (🧠 Melhor Próximo Ato).".to_string()
    })?;
    let p = google::renovar(&ia.http, &google::enderecos(), &cli, &renovacao).await?;
    let r = (p.acesso.clone(), p.email.clone());
    *ia.prova.lock().unwrap() = Some(p);
    Ok(r)
}

// ── o prompt e o envio ──────────────────────────────────────────────────────────────────────────────────────────────

/// Lê o prompt de onde a pessoa o guardou (o arquivo, o Google Docs ou o link da Microsoft). Lido a cada 🧠.
#[tauri::command]
pub async fn ia_prompt_ler(estado: State<'_, EstadoIa>, fonte: String) -> Result<prompt::Lido, String> {
    let ia = estado.0.clone();
    match prompt::classificar(&fonte)? {
        prompt::Fonte::Arquivo(p) => bloqueante(move || prompt::ler_arquivo(&p)).await,
        prompt::Fonte::GoogleDocs(id) => {
            let (acesso, email) = acesso_google(&ia).await?;
            prompt::ler_google(&ia.http, &id, &acesso, &email).await
        }
        prompt::Fonte::Microsoft(link) => prompt::ler_microsoft(&ia.http, &link).await,
    }
}

/// O pedido que a janela montou (já conferido pelo porteiro dela) vai à Anthropic com a chave do chaveiro.
#[tauri::command]
pub async fn ia_enviar(estado: State<'_, EstadoIa>, corpo: String, cabecalhos: BTreeMap<String, String>) -> Result<claude::Resposta, String> {
    let ia = estado.0.clone();
    let chave = chave(&ia).await?;
    let mut op = claude::Opcoes::default();
    if let Some(u) = de_prova("SKIA_CLAUDE_URL") {
        op.url = u;
        op.espera = std::time::Duration::from_millis(50);
    }
    claude::enviar(&ia.http, &op, &chave, &corpo, &cabecalhos).await
}
