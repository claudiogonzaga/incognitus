//! O 🧠 Melhor Próximo Ato do Incognitus, do lado do Rust: a chave da Anthropic (no chaveiro do sistema), o login do
//! Google (para ler o prompt guardado num Google Docs), a leitura do prompt e o envio à Anthropic.
//!
//! O que sai do computador, e só quando a pessoa clica no 🧠: o texto anonimizado do caso (o mesmo do _anon) e o prompt
//! que ela escolheu, para a Anthropic, com a chave dela. O mapa, os nomes reais e os PDFs nunca saem; antes de cada
//! envio a janela confere o pedido contra o mapa (o porteiro, modulo/porteiro.js) e barra o que tiver dado em claro.
//! A chave e a renovação do login ficam no chaveiro e não vão a arquivo, a log nem à janela.
//!
//! O Qwen do servidor (26/11/2026): sem chave; o endereço vem da janela (as Configurações da Minuta pelo modelo) e a
//! senha do servidor, se ele pedir, fica no chaveiro (conta "local"), como as chaves.
use crate::outras::Provedor;
use crate::{chaveiro, claude, gdocs, google, outras, pastas, prompt};
use serde::Serialize;
use std::collections::BTreeMap;
use std::path::PathBuf;
use std::sync::atomic::AtomicBool;
use std::sync::{Arc, Mutex};
use tauri::State;

const CONTA_EMAIL: &str = "google-email";
/// as permissões que o Google deu no último login (não é segredo: só diz se as pastas do Drive estão liberadas)
const CONTA_ESCOPOS: &str = "google-escopos";

pub struct Ia {
    /// o serviço no chaveiro: o identificador do app avulso, com ".modulo" quando hospedado
    servico: String,
    dados: PathBuf,
    http: reqwest::Client,
    prova: Mutex<Option<google::Prova>>,
    cancelar: Mutex<Arc<AtomicBool>>,
}

impl Ia {
    /// O serviço no chaveiro (o cofre do mapa guarda a chave dele no mesmo serviço).
    pub fn servico(&self) -> &str {
        &self.servico
    }
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

/// "Fale com o desenvolvedor": o formulário de feedback (Google Forms), no navegador. O endereço é fixo: a janela não
/// manda abrir endereço nenhum.
pub const URL_FEEDBACK: &str = "https://docs.google.com/forms/d/e/1FAIpQLSfH2nYhDkq0rWblMwbqRr7H_-TIk0W4xOX2f2k4L9XCNJqSvA/viewform";
/// A pergunta "Versão do Incognitus e sistema (Mac ou Windows)" do formulário, que o app já preenche (17/11/2026, pedido
/// do usuário: "pré-preencha esse dado").
const CAMPO_VERSAO: &str = "entry.1154383043";

/// "Incognitus 0.7.1 · Mac (macOS 15.5, aarch64)" ou "Incognitus 0.7.1 · Windows (x86_64)".
fn versao_e_sistema() -> String {
    let arq = std::env::consts::ARCH;
    #[cfg(target_os = "macos")]
    let sistema = {
        let v = std::process::Command::new("sw_vers")
            .arg("-productVersion")
            .output()
            .ok()
            .map(|o| String::from_utf8_lossy(&o.stdout).trim().to_string())
            .filter(|v| !v.is_empty() && v.len() < 20);
        match v {
            Some(v) => format!("Mac (macOS {v}, {arq})"),
            None => format!("Mac ({arq})"),
        }
    };
    #[cfg(target_os = "windows")]
    let sistema = format!("Windows ({arq})");
    #[cfg(not(any(target_os = "macos", target_os = "windows")))]
    let sistema = format!("{} ({arq})", std::env::consts::OS);
    format!("Incognitus {} · {sistema}", crate::VERSAO)
}

fn url_escape(s: &str) -> String {
    s.bytes()
        .map(|b| if b.is_ascii_alphanumeric() || b"-_.~".contains(&b) { (b as char).to_string() } else { format!("%{b:02X}") })
        .collect()
}

/// O endereço do formulário com a versão e o sistema já preenchidos.
pub fn url_feedback() -> String {
    format!("{URL_FEEDBACK}?usp=pp_url&{CAMPO_VERSAO}={}", url_escape(&versao_e_sistema()))
}

#[tauri::command]
pub fn abrir_feedback() -> Result<(), String> {
    abrir_no_navegador(&url_feedback())
}

#[cfg(test)]
mod testes {
    #[test]
    fn o_formulario_vem_com_a_versao() {
        let u = super::url_feedback();
        assert!(u.starts_with(super::URL_FEEDBACK) && u.contains("usp=pp_url&entry.1154383043=Incognitus%20"), "{u}");
        assert!(u.contains(&crate::VERSAO.replace('.', ".")), "{u}");
    }
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
    /// a permissão de ler as pastas do Drive (a pasta de modelos por link) foi dada
    drive: bool,
    /// a permissão de criar documentos no Drive (a minuta no Google Docs) foi dada
    escrever: bool,
}

#[derive(Serialize)]
pub struct Situacao {
    /// a chave da Anthropic (como antes)
    chave: SituacaoChave,
    /// a de cada IA: anthropic, openai, gemini, deepinfra e local (a senha do Qwen do servidor, se houver)
    chaves: BTreeMap<String, SituacaoChave>,
    google: SituacaoGoogle,
}

const PROVEDORES: [&str; 5] = ["anthropic", "openai", "gemini", "deepinfra", "local"];

/// A conta do chaveiro da IA: a da Anthropic (a de sempre) ou a de outra (openai, gemini, deepinfra).
fn conta_da(provedor: &Option<String>) -> Result<&'static str, String> {
    match provedor.as_deref().unwrap_or("anthropic") {
        "anthropic" | "" => Ok(chaveiro::CONTA_ANTHROPIC),
        outro => Provedor::de(outro).map(Provedor::conta).ok_or_else(|| format!("IA desconhecida: {outro}")),
    }
}

#[tauri::command]
pub async fn ia_situacao(estado: State<'_, EstadoIa>) -> Result<Situacao, String> {
    let ia = estado.0.clone();
    let prova = de_prova("SKIA_CHAVE_DE_PROVA");
    let lidas: Vec<Option<String>> = match prova.clone() {
        Some(c) => PROVEDORES.iter().map(|_| Some(c.clone())).collect(),
        None => {
            let s = ia.servico.clone();
            bloqueante(move || {
                PROVEDORES.iter().map(|p| chaveiro::ler(&s, conta_da(&Some(p.to_string()))?)).collect::<Result<Vec<_>, String>>()
            })
            .await?
        }
    };
    let chaves: BTreeMap<String, SituacaoChave> = PROVEDORES
        .iter()
        .zip(lidas.iter())
        .map(|(p, c)| (p.to_string(), SituacaoChave { guardada: c.is_some(), fim: c.as_deref().map(chaveiro::final_da_chave).unwrap_or_default() }))
        .collect();
    let chave = lidas[0].clone();
    let configurado = google::cliente(&ia.dados).is_some();
    let (entrou, email, drive, escrever) = if de_prova("SKIA_CHAVE_DE_PROVA").is_some() {
        (false, String::new(), false, false)
    } else {
        let s = ia.servico.clone();
        bloqueante(move || {
            let r = chaveiro::ler(&s, google::CONTA_RENOVACAO)?;
            let e = chaveiro::ler(&s, CONTA_EMAIL)?.unwrap_or_default();
            let escopos = chaveiro::ler(&s, CONTA_ESCOPOS)?.unwrap_or_default();
            Ok((r.is_some(), e, escopos.contains(google::ESCOPO_DRIVE), escopos.contains(google::ESCOPO_ARQUIVOS)))
        })
        .await?
    };
    Ok(Situacao {
        chave: SituacaoChave { guardada: chave.is_some(), fim: chave.map(|c| chaveiro::final_da_chave(&c)).unwrap_or_default() },
        chaves,
        google: SituacaoGoogle { configurado, entrou, email, drive: entrou && drive, escrever: entrou && escrever },
    })
}

// ── a chave da Anthropic ────────────────────────────────────────────────────────────────────────────────────────────

/// Guarda a chave de uma IA (sem `provedor`, a da Anthropic).
#[tauri::command]
pub async fn ia_chave_guardar(estado: State<'_, EstadoIa>, chave: String, provedor: Option<String>) -> Result<SituacaoChave, String> {
    let conta = conta_da(&provedor)?;
    let chave = match provedor.as_deref().and_then(Provedor::de) {
        Some(p) => outras::conferir_chave(p, &chave)?,
        None => chaveiro::conferir_chave_anthropic(&chave)?,
    };
    let s = estado.0.servico.clone();
    bloqueante(move || {
        chaveiro::guardar(&s, conta, &chave)?;
        Ok(SituacaoChave { guardada: true, fim: chaveiro::final_da_chave(&chave) })
    })
    .await
}

#[tauri::command]
pub async fn ia_chave_apagar(estado: State<'_, EstadoIa>, provedor: Option<String>) -> Result<(), String> {
    let conta = conta_da(&provedor)?;
    let s = estado.0.servico.clone();
    bloqueante(move || chaveiro::apagar(&s, conta)).await
}

async fn chave_de(ia: &Ia, provedor: &Option<String>) -> Result<String, String> {
    if let Some(c) = de_prova("SKIA_CHAVE_DE_PROVA") {
        return Ok(c);
    }
    let conta = conta_da(provedor)?;
    let nome = provedor.as_deref().and_then(Provedor::de).map(Provedor::rotulo).unwrap_or("Anthropic");
    let s = ia.servico.clone();
    bloqueante(move || chaveiro::ler(&s, conta)).await?.ok_or_else(|| {
        format!("Falta a chave da {nome}: guarde-a no ⚙ ao lado do 🧠 Gerar minuta (ela fica no chaveiro do sistema).")
    })
}

async fn chave(ia: &Ia) -> Result<String, String> {
    chave_de(ia, &None).await
}

/// A senha do Qwen do servidor, se houver (o servidor sem porteiro não pede nenhuma).
async fn senha_local(ia: &Ia) -> Result<Option<String>, String> {
    if let Some(c) = de_prova("SKIA_CHAVE_DE_PROVA") {
        return Ok(Some(c));
    }
    let s = ia.servico.clone();
    bloqueante(move || chaveiro::ler(&s, Provedor::Local.conta())).await
}

/// O endereço do Qwen do servidor: o da janela (no teste de ponta a ponta, o do servidor de mentira).
fn endereco_local(endereco: Option<String>) -> String {
    de_prova("SKIA_LOCAL_URL").or(endereco).unwrap_or_default()
}

/// Os modelos que a conta da pessoa pode usar naquela IA (lidos da própria IA, com a chave dela). No Qwen do servidor,
/// os que o servidor tem, no `endereco` que a janela manda.
#[tauri::command]
pub async fn ia_modelos(estado: State<'_, EstadoIa>, provedor: String, endereco: Option<String>) -> Result<Vec<String>, String> {
    let ia = estado.0.clone();
    if Provedor::de(&provedor) == Some(Provedor::Local) {
        let senha = senha_local(&ia).await?;
        return outras::listar_modelos_local(&ia.http, &endereco_local(endereco), senha.as_deref()).await;
    }
    let prov = Some(provedor.clone());
    let chave = chave_de(&ia, &prov).await?;
    match Provedor::de(&provedor) {
        Some(p) => outras::listar_modelos(&ia.http, p, &chave).await,
        None => {
            let r = ia
                .http
                .get("https://api.anthropic.com/v1/models?limit=100")
                .header("x-api-key", &chave)
                .header("anthropic-version", "2023-06-01")
                .timeout(std::time::Duration::from_secs(30))
                .send()
                .await
                .map_err(|e| format!("A Anthropic não respondeu ({e})."))?;
            let code = r.status().as_u16();
            let texto = r.text().await.unwrap_or_default();
            let v: serde_json::Value = serde_json::from_str(&texto).map_err(|e| format!("lista de modelos ilegível: {e}"))?;
            if code != 200 {
                return Err(format!("A Anthropic não listou os modelos (código {code})."));
            }
            let mut out: Vec<String> = v
                .get("data")
                .and_then(|d| d.as_array())
                .into_iter()
                .flatten()
                .filter_map(|m| m.get("id").and_then(|x| x.as_str()).map(str::to_string))
                .collect();
            out.sort();
            Ok(out)
        }
    }
}

// ── o login do Google ───────────────────────────────────────────────────────────────────────────────────────────────

#[derive(Serialize)]
pub struct Entrou {
    email: String,
}

/// O login do Google. Com `drive`, pede também a leitura das pastas do Drive (a pasta de modelos por link), somada às
/// permissões de antes. Com `escrever`, a de criar documentos no Drive (a minuta no Google Docs).
#[tauri::command]
pub async fn ia_google_entrar(estado: State<'_, EstadoIa>, drive: Option<bool>, escrever: Option<bool>) -> Result<Entrou, String> {
    let ia = estado.0.clone();
    let cli = google::cliente(&ia.dados).ok_or(google::SEM_CLIENTE)?;
    let cancelado = Arc::new(AtomicBool::new(false));
    *ia.cancelar.lock().unwrap() = cancelado.clone();
    // o que já foi dado antes continua pedido: um login novo não tira a permissão das pastas nem a de criar documentos
    let s = ia.servico.clone();
    let antes = bloqueante(move || chaveiro::ler(&s, CONTA_ESCOPOS)).await.ok().flatten().unwrap_or_default();
    let mut escopo = google::ESCOPO.to_string();
    for (pedido, e) in [(drive, google::ESCOPO_DRIVE), (escrever, google::ESCOPO_ARQUIVOS)] {
        if pedido.unwrap_or(false) || antes.contains(e) {
            escopo.push(' ');
            escopo.push_str(e);
        }
    }
    let e = google::entrar(&ia.http, &google::enderecos(), &cli, abrir_no_navegador, cancelado, &escopo).await?;
    let (s, renovacao, email, escopos) = (ia.servico.clone(), e.renovacao.clone(), e.prova.email.clone(), e.escopos.clone());
    bloqueante(move || {
        chaveiro::guardar(&s, google::CONTA_RENOVACAO, &renovacao)?;
        chaveiro::guardar(&s, CONTA_ESCOPOS, &escopos)?;
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
        chaveiro::apagar(&s, CONTA_ESCOPOS)?;
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
        "Para ler o Google Docs ou a planilha, entre com o Google nas Configurações (🧠 Melhor Próximo Ato).".to_string()
    })?;
    let p = google::renovar(&ia.http, &google::enderecos(), &cli, &renovacao).await?;
    let r = (p.acesso.clone(), p.email.clone());
    *ia.prova.lock().unwrap() = Some(p);
    Ok(r)
}

// ── o prompt e o envio ──────────────────────────────────────────────────────────────────────────────────────────────

/// Lê uma fonte do 🧠 — o prompt, o Guia, o RPP, o Livro de Modelos ou um modelo — de onde a pessoa a guardou (o arquivo,
/// o Google Docs, a planilha do Google ou o link da Microsoft). Lida a cada 🧠.
#[tauri::command]
pub async fn ia_prompt_ler(estado: State<'_, EstadoIa>, fonte: String) -> Result<prompt::Lido, String> {
    let ia = estado.0.clone();
    match prompt::classificar(&fonte)? {
        prompt::Fonte::Arquivo(p) => bloqueante(move || prompt::ler_arquivo(&p)).await,
        prompt::Fonte::GoogleDocs(id) => {
            let (acesso, email) = acesso_google(&ia).await?;
            prompt::ler_google(&ia.http, &id, &acesso, &email).await
        }
        prompt::Fonte::GoogleSheets(id, gid) => {
            let (acesso, email) = acesso_google(&ia).await?;
            prompt::ler_planilha(&ia.http, &id, gid.as_deref(), &acesso, &email).await
        }
        prompt::Fonte::Microsoft(link) => prompt::ler_microsoft(&ia.http, &link).await,
        prompt::Fonte::GoogleDrive(id) => {
            let (acesso, email) = acesso_google(&ia).await?;
            let (bytes, nome, mime) = pastas::baixar_drive(&ia.http, pastas::DRIVE_BASE, &id, &acesso).await?;
            if mime == "application/vnd.google-apps.document" {
                return prompt::ler_google(&ia.http, &id, &acesso, &email).await;
            }
            if bytes.len() > prompt::MAX_BYTES * 5 {
                return Err("O arquivo do Drive é grande demais.".into());
            }
            let texto = bloqueante(move || prompt::texto_dos_bytes(&bytes)).await?;
            prompt::lido(texto, "drive", nome)
        }
    }
}

/// A pasta de modelos (17/11/2026): a do computador, ou o link de uma pasta do Google Drive (com o login do Google e a
/// permissão das pastas do Drive), do OneDrive ou do SharePoint (compartilhada com "qualquer pessoa com o link").
#[tauri::command]
pub async fn ia_pasta_listar(estado: State<'_, EstadoIa>, pasta: String) -> Result<Vec<pastas::ArquivoModelo>, String> {
    let ia = estado.0.clone();
    match pastas::classificar(&pasta)? {
        pastas::Pasta::Local(p) => bloqueante(move || pastas::listar_local(&p)).await,
        pastas::Pasta::Drive { id, chave } => {
            let (acesso, _) = acesso_google(&ia).await.map_err(|e| {
                if e.contains("entre com o Google") { "Para ler a pasta do Google Drive, entre com o Google nas Configurações do 🧠 e clique em \"Permitir pastas do Drive\".".to_string() } else { e }
            })?;
            pastas::listar_drive(&ia.http, pastas::DRIVE_BASE, &id, chave.as_deref(), &acesso).await
        }
        pastas::Pasta::OneDrive(link) | pastas::Pasta::SharePoint(link) => {
            let teste = de_prova("SKIA_MICROSOFT_URL");
            pastas::listar_microsoft(&ia.http, &link, teste.as_deref()).await
        }
    }
}

/// A minuta no Google Docs (18/11/2026): o .docx que o editor gravou sobe para o Drive de quem usa, vira um documento do
/// Google Docs e abre no navegador. Só .docx que existe; o endereço que abre é montado aqui, do id.
#[tauri::command]
pub async fn ia_gdocs_enviar(estado: State<'_, EstadoIa>, caminho: String, existente: Option<String>, trocar: Option<bool>) -> Result<gdocs::NoDocs, String> {
    let ia = estado.0.clone();
    let p = std::path::PathBuf::from(&caminho);
    let eh_docx = p.extension().map(|e| e.to_string_lossy().eq_ignore_ascii_case("docx")).unwrap_or(false);
    if !eh_docx || !p.is_file() {
        return Err("só mando ao Google Docs um .docx que já foi gravado".into());
    }
    let nome = p.file_stem().map(|n| n.to_string_lossy().to_string()).unwrap_or_else(|| "Minuta".into());
    let bytes = bloqueante(move || std::fs::read(&p).map_err(|e| format!("não consegui ler a minuta: {e}"))).await?;
    let (base, acesso) = match de_prova("SKIA_DRIVE_URL") {
        Some(b) => (b, "acesso-de-prova".to_string()),
        None => {
            let (a, _) = acesso_google(&ia).await.map_err(|e| if e.contains("entre com o Google") { gdocs::ERRO_SEM_ESCRITA.to_string() } else { e })?;
            (pastas::DRIVE_BASE.to_string(), a)
        }
    };
    let r = gdocs::enviar(&ia.http, &base, &acesso, &nome, &bytes, existente.as_deref(), trocar.unwrap_or(false)).await?;
    if std::env::var_os("SKIA_E2E").is_none() {
        abrir_no_navegador(&r.link)?;
    }
    Ok(r)
}

/// O pedido que a janela montou (já conferido pelo porteiro dela) vai à Anthropic com a chave do chaveiro.
/// Com `provedor` (openai, gemini, deepinfra), o pedido já vem no formato daquela IA (a janela traduz depois do porteiro).
/// Com `provedor` local, vai ao Qwen do servidor no `endereco` da janela, sem chave (com a senha do servidor, se houver).
#[tauri::command]
pub async fn ia_enviar(
    estado: State<'_, EstadoIa>,
    corpo: String,
    cabecalhos: BTreeMap<String, String>,
    provedor: Option<String>,
    endereco: Option<String>,
) -> Result<claude::Resposta, String> {
    let ia = estado.0.clone();
    if let Some(Provedor::Local) = provedor.as_deref().and_then(Provedor::de) {
        let senha = senha_local(&ia).await?;
        return outras::enviar_local(&ia.http, &endereco_local(endereco), senha.as_deref(), &corpo).await;
    }
    if let Some(p) = provedor.as_deref().and_then(Provedor::de) {
        let chave = chave_de(&ia, &provedor).await?;
        let url = de_prova("SKIA_OUTRA_IA_URL");
        return outras::enviar(&ia.http, p, &chave, &corpo, url.as_deref()).await;
    }
    let chave = chave(&ia).await?;
    let mut op = claude::Opcoes::default();
    if let Some(u) = de_prova("SKIA_CLAUDE_URL") {
        op.url = u;
        op.espera = std::time::Duration::from_millis(50);
    }
    claude::enviar(&ia.http, &op, &chave, &corpo, &cabecalhos).await
}
