//! Skiá — anonimização de PDFs em lote, no computador, com o motor de anonimização do Graphé, e a reidentificação das
//! minutas com o mapa. O Rust cuida do que é do sistema: os sidecars (pdftotext, pdftoppm, tesseract), a cópia temporária
//! de cada PDF, a gravação da saída (PDF, .txt e mapa), a leitura e a gravação das minutas (reid.rs) e o motor em uso
//! (embutido ou baixado e assinado). A anonimização e a reidentificação rodam no front, num Web Worker, com o motor.js.
//! Nada sai do computador: a única conexão é a checagem das regras (só leitura do repositório público skia-motor), que
//! os Ajustes desligam.
mod ferramentas;
mod motor;
mod pdf;
mod reid;

use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex};
use tauri::{Manager, State};

const PASTA_SAIDA: &str = "Skiá — anonimizados";
const MARCA_PRODUTOR: &[u8] = b"/Producer (Skia ";

struct Interno {
    ferramentas: ferramentas::Ferramentas,
    cache: PathBuf,
    dados: PathBuf,
    arquivos: Mutex<HashMap<String, PathBuf>>,
    contador: AtomicU64,
}

#[derive(Clone)]
struct Estado(Arc<Interno>);

impl Estado {
    fn tmp(&self, id: &str) -> Result<PathBuf, String> {
        self.0.arquivos.lock().unwrap().get(id).cloned().ok_or_else(|| "arquivo desconhecido (já limpo?)".to_string())
    }
    fn url_motor(&self) -> String {
        std::env::var("SKIA_MOTOR_URL").unwrap_or_else(|_| motor::URL_PADRAO.to_string())
    }
}

async fn em_segundo_plano<T: Send + 'static>(f: impl FnOnce() -> Result<T, String> + Send + 'static) -> Result<T, String> {
    tauri::async_runtime::spawn_blocking(f).await.map_err(|e| format!("tarefa interrompida: {e}"))?
}

// ── ferramentas e arquivos ─────────────────────────────────────────────────────────────────────────────────────────

#[derive(Serialize)]
struct InfoApp {
    versao: String,
    ferramentas: Result<Vec<String>, String>,
    plataforma: String,
}

#[tauri::command]
async fn info_app(estado: State<'_, Estado>) -> Result<InfoApp, String> {
    let e = estado.inner().clone();
    let f = em_segundo_plano(move || Ok(e.0.ferramentas.versoes())).await?;
    Ok(InfoApp { versao: env!("CARGO_PKG_VERSION").into(), ferramentas: f, plataforma: std::env::consts::OS.into() })
}

#[derive(Serialize)]
struct Preparado {
    id: String,
    bytes: u64,
}

/// Copia o PDF para uma pasta temporária só dele (as ferramentas nunca abrem o original).
#[tauri::command]
async fn preparar_arquivo(estado: State<'_, Estado>, caminho: String) -> Result<Preparado, String> {
    let e = estado.inner().clone();
    em_segundo_plano(move || {
        let orig = PathBuf::from(&caminho);
        let meta = std::fs::metadata(&orig).map_err(|err| format!("não consegui ler o arquivo: {err}"))?;
        if !meta.is_file() {
            return Err("não é um arquivo".into());
        }
        let mut cabeca = [0u8; 1024];
        let n = std::io::Read::read(&mut std::fs::File::open(&orig).map_err(|err| err.to_string())?, &mut cabeca).unwrap_or(0);
        if !cabeca[..n].windows(5).any(|w| w == b"%PDF-") {
            return Err("não parece um PDF".into());
        }
        let id = format!("a{}-{}", e.0.contador.fetch_add(1, Ordering::SeqCst), std::process::id());
        let tmp = e.0.cache.join("tmp").join(&id);
        std::fs::create_dir_all(&tmp).map_err(|err| format!("pasta temporária: {err}"))?;
        std::fs::copy(&orig, tmp.join("entrada.pdf")).map_err(|err| format!("cópia temporária: {err}"))?;
        e.0.arquivos.lock().unwrap().insert(id.clone(), tmp);
        Ok(Preparado { id, bytes: meta.len() })
    })
    .await
}

#[tauri::command]
async fn extrair_texto(estado: State<'_, Estado>, id: String) -> Result<String, String> {
    let e = estado.inner().clone();
    let tmp = e.tmp(&id)?;
    em_segundo_plano(move || e.0.ferramentas.pdftotext(&tmp.join("entrada.pdf"), &tmp)).await
}

#[tauri::command]
async fn ocr_pagina(estado: State<'_, Estado>, id: String, pagina: u32) -> Result<String, String> {
    let e = estado.inner().clone();
    let tmp = e.tmp(&id)?;
    em_segundo_plano(move || e.0.ferramentas.ocr_pagina(&tmp.join("entrada.pdf"), pagina, &tmp)).await
}

/// Apaga a pasta temporária do arquivo (chamado ao fim de cada PDF, com ou sem erro).
#[tauri::command]
fn limpar_arquivo(estado: State<'_, Estado>, id: String) {
    if let Some(tmp) = estado.0.arquivos.lock().unwrap().remove(&id) {
        let _ = std::fs::remove_dir_all(tmp);
    }
}

// ── saída ──────────────────────────────────────────────────────────────────────────────────────────────────────────

#[derive(Deserialize)]
struct PedidoSaida {
    origem: String,
    destino: Option<String>,
    cabecalho: String,
    paginas: Vec<String>,
    texto: Option<String>,
    mapa: Option<String>,
    versao_motor: String,
}

#[derive(Serialize)]
struct Gravado {
    pdf: String,
    txt: Option<String>,
    mapa: Option<String>,
    sem_glifo: usize,
    folhas: usize,
}

fn pasta_destino(origem: &Path, destino: &Option<String>) -> PathBuf {
    match destino {
        Some(d) if !d.trim().is_empty() => PathBuf::from(d),
        _ => origem.parent().unwrap_or(Path::new(".")).join(PASTA_SAIDA),
    }
}

fn mesmo_arquivo(a: &Path, b: &Path) -> bool {
    match (std::fs::canonicalize(a), std::fs::canonicalize(b)) {
        (Ok(x), Ok(y)) => x == y,
        _ => false,
    }
}

/// O nome livre: o próprio, se não existe ou se é saída do Skiá; senão "nome (2).ext", "nome (3).ext"…
fn livre(dir: &Path, base: &str, ext: &str, eh_nosso: impl Fn(&Path) -> bool) -> PathBuf {
    let p = dir.join(format!("{base}{ext}"));
    if !p.exists() || eh_nosso(&p) {
        return p;
    }
    (2..).map(|n| dir.join(format!("{base} ({n}){ext}"))).find(|q| !q.exists() || eh_nosso(q)).unwrap()
}

fn pdf_do_skia(p: &Path) -> bool {
    std::fs::read(p).map(|b| b.windows(MARCA_PRODUTOR.len()).any(|w| w == MARCA_PRODUTOR)).unwrap_or(false)
}

fn txt_do_skia(p: &Path) -> bool {
    std::fs::read(p).map(|b| String::from_utf8_lossy(&b[..b.len().min(400)]).contains("AUTOS ANONIMIZADOS")).unwrap_or(false)
}

fn gravar_atomico(p: &Path, dados: &[u8]) -> Result<(), String> {
    let tmp = p.with_file_name(format!(".{}.parcial", p.file_name().unwrap().to_string_lossy()));
    std::fs::write(&tmp, dados).map_err(|e| format!("não consegui gravar {}: {e}", p.display()))?;
    std::fs::rename(&tmp, p).map_err(|e| format!("não consegui gravar {}: {e}", p.display()))
}

#[tauri::command]
async fn gravar_saida(pedido: PedidoSaida) -> Result<Gravado, String> {
    em_segundo_plano(move || {
        let origem = PathBuf::from(&pedido.origem);
        let dir = pasta_destino(&origem, &pedido.destino);
        std::fs::create_dir_all(&dir).map_err(|e| format!("não consegui criar a pasta de saída: {e}"))?;
        let nome = origem.file_name().ok_or("origem sem nome")?.to_string_lossy().to_string();
        let (base, ext) = match nome.rfind('.') {
            Some(k) if k > 0 => (nome[..k].to_string(), nome[k..].to_string()),
            _ => (nome.clone(), ".pdf".to_string()),
        };
        let destino_pdf = livre(&dir, &base, &ext, pdf_do_skia);
        if mesmo_arquivo(&destino_pdf, &origem) || destino_pdf == origem {
            return Err("a pasta de saída é a dos originais: o anonimizado teria o mesmo nome e tomaria o lugar do original".into());
        }
        let produtor = format!("Skia {} - motor de anonimizacao {}", env!("CARGO_PKG_VERSION"), pedido.versao_motor);
        let r = pdf::gerar(&pedido.cabecalho, &pedido.paginas, &produtor)?;
        gravar_atomico(&destino_pdf, &r.bytes)?;
        let base_final = destino_pdf.file_stem().unwrap().to_string_lossy().to_string();
        let mut txt = None;
        if let Some(t) = &pedido.texto {
            let p = livre(&dir, &base_final, ".txt", txt_do_skia);
            if mesmo_arquivo(&p, &origem) {
                return Err("o .txt tomaria o lugar do original".into());
            }
            gravar_atomico(&p, t.as_bytes())?;
            txt = Some(p.to_string_lossy().to_string());
        }
        let mut mapa = None;
        if let Some(m) = &pedido.mapa {
            let p = dir.join(format!("{base_final} — MAPA (nomes reais).json"));
            gravar_atomico(&p, m.as_bytes())?;
            mapa = Some(p.to_string_lossy().to_string());
        }
        Ok(Gravado { pdf: destino_pdf.to_string_lossy().to_string(), txt, mapa, sem_glifo: r.sem_glifo, folhas: pedido.paginas.len() + 1 })
    })
    .await
}

/// A pasta de saída padrão de um arquivo (ao lado do original).
#[tauri::command]
fn pasta_padrao(caminho: String) -> String {
    pasta_destino(Path::new(&caminho), &None).to_string_lossy().to_string()
}

/// Os PDFs de uma pasta (sem descer às subpastas; a de saída do Skiá fica de fora).
#[tauri::command]
fn listar_pdfs(pasta: String) -> Result<Vec<String>, String> {
    let mut v: Vec<String> = std::fs::read_dir(&pasta)
        .map_err(|e| format!("não consegui ler a pasta: {e}"))?
        .filter_map(|e| e.ok())
        .map(|e| e.path())
        .filter(|p| p.is_file() && p.extension().map_or(false, |x| x.eq_ignore_ascii_case("pdf")))
        .filter(|p| !p.file_name().map_or(false, |n| n.to_string_lossy().starts_with('.')))
        .map(|p| p.to_string_lossy().to_string())
        .collect();
    v.sort();
    Ok(v)
}

/// O que foi escolhido ou arrastado: os PDFs, e os PDFs das pastas (sem descer às subpastas).
#[tauri::command]
fn expandir_caminhos(caminhos: Vec<String>) -> Vec<String> {
    let mut v = Vec::new();
    for c in caminhos {
        let p = PathBuf::from(&c);
        if p.is_dir() {
            v.extend(listar_pdfs(c).unwrap_or_default());
        } else if p.is_file() && p.extension().map_or(false, |x| x.eq_ignore_ascii_case("pdf")) {
            v.push(c);
        }
    }
    v
}

// ── reidentificação ────────────────────────────────────────────────────────────────────────────────────────────────

/// O texto de um arquivo (o mapa .json, uma minuta .txt ou .md).
#[tauri::command]
async fn ler_texto(caminho: String) -> Result<String, String> {
    em_segundo_plano(move || reid::ler_texto(Path::new(&caminho))).await
}

/// As partes com texto de uma minuta .docx, para o front trocar os tokens.
#[tauri::command]
async fn docx_partes(caminho: String) -> Result<Vec<reid::Parte>, String> {
    em_segundo_plano(move || reid::docx_partes(Path::new(&caminho))).await
}

#[derive(Deserialize)]
struct PedidoReid {
    origem: String,
    titulo: String,
    texto: Option<String>,
    partes: Option<Vec<reid::Parte>>,
}

/// A minuta reidentificada, ao lado da original, com "(reidentificado)" no nome; a original não muda.
#[tauri::command]
async fn gravar_reidentificado(pedido: PedidoReid) -> Result<String, String> {
    em_segundo_plano(move || {
        let origem = PathBuf::from(&pedido.origem);
        let dir = origem.parent().ok_or("a minuta não tem pasta")?.to_path_buf();
        let ext = origem.extension().map(|x| format!(".{}", x.to_string_lossy().to_lowercase())).unwrap_or_default();
        let destino = livre(&dir, &format!("{} (reidentificado)", reid::nome_seguro(&pedido.titulo)), &ext, |_| false);
        if mesmo_arquivo(&destino, &origem) || destino == origem {
            return Err("a cópia reidentificada tomaria o lugar da original".into());
        }
        match (&pedido.partes, &pedido.texto) {
            (Some(p), _) => reid::docx_gravar(&origem, &destino, p)?,
            (None, Some(t)) => gravar_atomico(&destino, t.as_bytes())?,
            _ => return Err("nada para gravar".into()),
        }
        Ok(destino.to_string_lossy().to_string())
    })
    .await
}

/// O texto reidentificado, no arquivo que o usuário escolheu ("Salvar como .txt…").
#[tauri::command]
async fn gravar_texto(caminho: String, texto: String) -> Result<(), String> {
    em_segundo_plano(move || gravar_atomico(Path::new(&caminho), texto.as_bytes())).await
}

#[tauri::command]
fn revelar(caminho: String) -> Result<(), String> {
    let p = PathBuf::from(&caminho);
    #[cfg(target_os = "macos")]
    let r = std::process::Command::new("open").arg("-R").arg(&p).spawn();
    #[cfg(target_os = "windows")]
    let r = {
        use std::os::windows::process::CommandExt;
        let mut arg = std::ffi::OsString::from("/select,");
        arg.push(p.as_os_str());
        std::process::Command::new("explorer").raw_arg(arg).spawn()
    };
    #[cfg(not(any(target_os = "macos", target_os = "windows")))]
    let r = std::process::Command::new("xdg-open").arg(p.parent().unwrap_or(&p)).spawn();
    r.map(|_| ()).map_err(|e| e.to_string())
}

// ── ajustes ────────────────────────────────────────────────────────────────────────────────────────────────────────

#[tauri::command]
fn ajustes_ler(estado: State<'_, Estado>) -> serde_json::Value {
    std::fs::read(estado.0.dados.join("ajustes.json"))
        .ok()
        .and_then(|b| serde_json::from_slice(&b).ok())
        .unwrap_or(serde_json::json!({}))
}

#[tauri::command]
fn ajustes_gravar(estado: State<'_, Estado>, ajustes: serde_json::Value) -> Result<(), String> {
    std::fs::create_dir_all(&estado.0.dados).map_err(|e| e.to_string())?;
    gravar_atomico(&estado.0.dados.join("ajustes.json"), serde_json::to_string_pretty(&ajustes).unwrap().as_bytes())
}

// ── motor ──────────────────────────────────────────────────────────────────────────────────────────────────────────

#[tauri::command]
fn motor_ativo(estado: State<'_, Estado>) -> motor::MotorAtivo {
    motor::ativo(&motor::pasta(&estado.0.dados))
}

#[tauri::command]
async fn motor_checar(estado: State<'_, Estado>, manual: bool) -> Result<Option<motor::Manifesto>, String> {
    let dir = motor::pasta(&estado.0.dados);
    let em_uso = motor::ativo(&dir).manifesto;
    let recusado = if manual { None } else { motor::recusado(&dir) };
    motor::checar(&estado.url_motor(), &em_uso, recusado).await
}

#[tauri::command]
async fn motor_baixar(estado: State<'_, Estado>) -> Result<motor::Baixado, String> {
    motor::baixar(&estado.url_motor(), &motor::pasta(&estado.0.dados)).await
}

#[tauri::command]
fn motor_instalar(estado: State<'_, Estado>, sha256: String) -> Result<motor::MotorAtivo, String> {
    motor::instalar(&motor::pasta(&estado.0.dados), &sha256)
}

#[tauri::command]
fn motor_voltar(estado: State<'_, Estado>) -> Result<motor::MotorAtivo, String> {
    motor::voltar(&motor::pasta(&estado.0.dados))
}

/// Erros e marcos do front no stderr (para diagnóstico; nunca leva texto dos autos).
#[tauri::command]
fn registrar(msg: String) {
    eprintln!("[skia] {}", msg.chars().take(500).collect::<String>());
}

// ── teste de ponta a ponta (só com SKIA_E2E apontando para um plano) ──────────────────────────────────────────────

fn plano_e2e() -> Option<(PathBuf, serde_json::Value)> {
    let p = PathBuf::from(std::env::var_os("SKIA_E2E")?);
    let v = serde_json::from_slice(&std::fs::read(&p).ok()?).ok()?;
    Some((p, v))
}

#[tauri::command]
fn e2e_plano() -> Option<serde_json::Value> {
    plano_e2e().map(|(_, v)| v)
}

#[tauri::command]
fn e2e_gravar(nome: String, conteudo: String) -> Result<(), String> {
    let (_, v) = plano_e2e().ok_or("fora do modo de teste")?;
    let dir = PathBuf::from(v["extracao"].as_str().ok_or("plano sem pasta de extração")?);
    std::fs::create_dir_all(&dir).map_err(|e| e.to_string())?;
    let nome = Path::new(&nome).file_name().ok_or("nome inválido")?.to_owned();
    std::fs::write(dir.join(nome), conteudo).map_err(|e| e.to_string())
}

#[tauri::command]
fn e2e_fim(app: tauri::AppHandle, relatorio: String) -> Result<(), String> {
    let (_, v) = plano_e2e().ok_or("fora do modo de teste")?;
    let p = PathBuf::from(v["relatorio"].as_str().ok_or("plano sem relatório")?);
    std::fs::write(p, relatorio).map_err(|e| e.to_string())?;
    // "manter": a janela fica aberta no fim (para a foto do README); senão, o app sai.
    if !v["manter"].as_bool().unwrap_or(false) {
        app.exit(0);
    }
    Ok(())
}

// ── início ─────────────────────────────────────────────────────────────────────────────────────────────────────────

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    tauri::Builder::default()
        .plugin(tauri_plugin_dialog::init())
        .setup(|app| {
            // SKIA_DADOS (só para testes): dados e cache numa pasta à parte, sem tocar nos do usuário.
            let (cache, dados) = match std::env::var_os("SKIA_DADOS") {
                Some(d) => (PathBuf::from(&d).join("cache"), PathBuf::from(&d).join("dados")),
                None => (app.path().app_cache_dir()?, app.path().app_data_dir()?),
            };
            let recursos = app.path().resource_dir()?;
            let bin = std::env::current_exe()?.parent().map(Path::to_path_buf).unwrap_or_default();
            // Sobras de uma execução interrompida: a pasta temporária começa vazia.
            let _ = std::fs::remove_dir_all(cache.join("tmp"));
            std::fs::create_dir_all(cache.join("tmp"))?;
            std::fs::create_dir_all(&dados)?;
            let interno = Interno {
                ferramentas: ferramentas::Ferramentas::new(bin, recursos, &cache),
                cache,
                dados,
                arquivos: Mutex::new(HashMap::new()),
                contador: AtomicU64::new(1),
            };
            app.manage(Estado(Arc::new(interno)));
            Ok(())
        })
        .invoke_handler(tauri::generate_handler![
            info_app,
            preparar_arquivo,
            extrair_texto,
            ocr_pagina,
            limpar_arquivo,
            gravar_saida,
            pasta_padrao,
            listar_pdfs,
            expandir_caminhos,
            ler_texto,
            docx_partes,
            gravar_reidentificado,
            gravar_texto,
            revelar,
            ajustes_ler,
            ajustes_gravar,
            motor_ativo,
            motor_checar,
            motor_baixar,
            motor_instalar,
            motor_voltar,
            registrar,
            e2e_plano,
            e2e_gravar,
            e2e_fim
        ])
        .build(tauri::generate_context!())
        .expect("erro ao montar o Skiá")
        .run(|app, ev| {
            if let tauri::RunEvent::Exit = ev {
                if let Some(e) = app.try_state::<Estado>() {
                    let _ = std::fs::remove_dir_all(e.0.cache.join("tmp"));
                }
            }
        });
}
