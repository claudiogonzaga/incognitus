//! Incognitus — anonimização de PDFs em lote, no computador, com o motor de anonimização do Graphé, e a reidentificação das
//! minutas com o mapa. O Rust cuida do que é do sistema: os sidecars (pdftotext, pdftoppm, tesseract), a cópia temporária
//! de cada PDF, a gravação da saída (PDF, .txt e mapa), a leitura e a gravação das minutas (reid.rs) e o motor em uso
//! (embutido ou baixado e assinado). A anonimização e a reidentificação rodam no front, num Web Worker, com o motor.js.
//! Nada sai do computador sem a pessoa pedir: a conexão de sempre é a checagem das regras (só leitura do repositório
//! público incognitus-motor), que os Ajustes desligam. O 🧠 Melhor Próximo Ato (ia.rs), só com a chave da Anthropic de
//! quem usa e só ao clique, manda o texto anonimizado do caso e o prompt escolhido à Anthropic; o mapa nunca sai.
//!
//! Este crate é o plugin `anonimizacao` do Tauri (comandos `plugin:anonimizacao|…`), pelo contrato de módulo do Graphé
//! Desktop (MODULOS.md): serve ao app Incognitus avulso (`init`) e ao Graphé Desktop, que o hospeda como módulo
//! (`init_como_modulo`). A lista de comandos está aqui, no build.rs e em permissions/default.toml.
mod chaveiro;
mod claude;
mod cofre;
mod docx;
mod ferramentas;
mod gdocs;
mod google;
mod ia;
mod motor;
mod prompt;
mod visor;
mod acordado;
mod ner;
mod outras;
mod pastas;
mod pdf;
mod reid;

use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex};
use tauri::plugin::{Builder, TauriPlugin};
use tauri::{Manager, Runtime, State};

/// O `id` do módulo no `modulo.json` e a subpasta de dados quando hospedado.
pub const ID: &str = "incognitus";
pub const NOME: &str = "Incognitus";
pub const VERSAO: &str = env!("CARGO_PKG_VERSION");

const PASTA_SAIDA: &str = "Incognitus — anonimizados";
/// O mapa e a lista CONFIRA têm os nomes reais: ficam numa subpasta à parte, para não irem junto com os PDFs para a IA.
const PASTA_PRIVADA: &str = "NÃO ENVIAR À IA (nomes reais)";
const MARCA_PRODUTOR: &[u8] = b"/Producer (Incognitus ";
const MARCA_ANTIGA: &[u8] = b"/Producer (Skia ";   // as saídas de antes do nome novo (até a 0.2.0)

struct Interno {
    hospedado: bool,
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
struct Modulo {
    id: &'static str,
    nome: &'static str,
    versao: &'static str,
}

#[derive(Serialize)]
struct Info {
    modulo: Modulo,
    hospedado: bool,
    sistema: &'static str,
    dados: String,
}

/// Obrigatório pelo contrato de módulo: quem é o módulo e se está hospedado (a janela esconde o que é só do app avulso).
#[tauri::command]
fn info(estado: State<'_, Estado>) -> Info {
    Info {
        modulo: Modulo { id: ID, nome: NOME, versao: VERSAO },
        hospedado: estado.0.hospedado,
        sistema: std::env::consts::OS,
        dados: estado.0.dados.display().to_string(),
    }
}

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
    #[serde(default)]
    confira: Option<String>,
    /// O nome do arquivo de saída, sem a extensão, já sem os nomes de pessoa (o motor troca pelos tokens). Sem ele, o do original.
    #[serde(default)]
    nome_saida: Option<String>,
    versao_motor: String,
    /// O mapa vai cifrado para o disco (cofre.rs). Sem a chave do chaveiro, vai em claro e a janela é avisada.
    #[serde(default)]
    cifrar: bool,
}

/// O nome de saída termina em "_anon" (sem repetir, se já termina).
fn com_sufixo(base: &str) -> String {
    if base.to_lowercase().ends_with(SUFIXO) { base.to_string() } else { format!("{base}{SUFIXO}") }
}
const SUFIXO: &str = "_anon";

/// Um nome de arquivo que todo sistema aceita (sem / \\ : * ? " < > | e sem controle), no máximo 180 caracteres.
fn nome_seguro(s: &str) -> String {
    let t: String = s.chars().map(|c| if "/\\:*?\"<>|".contains(c) || c.is_control() { '-' } else { c }).collect();
    let t = t.split_whitespace().collect::<Vec<_>>().join(" ");
    let t = t.trim_matches(|c: char| c == '.' || c == ' ').to_string();
    let t: String = t.chars().take(180).collect();
    if t.is_empty() { "documento".into() } else { t }
}

#[derive(Serialize)]
struct Gravado {
    pdf: String,
    txt: Option<String>,
    mapa: Option<String>,
    confira: Option<String>,
    sem_glifo: usize,
    folhas: usize,
    /// o mapa foi gravado cifrado
    mapa_cifrado: bool,
    /// por que o mapa, pedido cifrado, foi em claro (o chaveiro do sistema recusou a chave)
    aviso_cofre: Option<String>,
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

/// O nome livre: o próprio, se não existe ou se é saída do Incognitus; senão "nome (2).ext", "nome (3).ext"…
fn livre(dir: &Path, base: &str, ext: &str, eh_nosso: impl Fn(&Path) -> bool) -> PathBuf {
    let p = dir.join(format!("{base}{ext}"));
    if !p.exists() || eh_nosso(&p) {
        return p;
    }
    (2..).map(|n| dir.join(format!("{base} ({n}){ext}"))).find(|q| !q.exists() || eh_nosso(q)).unwrap()
}

fn pdf_nosso(p: &Path) -> bool {
    std::fs::read(p)
        .map(|b| [MARCA_PRODUTOR, MARCA_ANTIGA].iter().any(|m| b.windows(m.len()).any(|w| w == *m)))
        .unwrap_or(false)
}

fn txt_nosso(p: &Path) -> bool {
    std::fs::read(p).map(|b| String::from_utf8_lossy(&b[..b.len().min(400)]).contains("AUTOS ANONIMIZADOS")).unwrap_or(false)
}

fn gravar_atomico(p: &Path, dados: &[u8]) -> Result<(), String> {
    let tmp = p.with_file_name(format!(".{}.parcial", p.file_name().unwrap().to_string_lossy()));
    std::fs::write(&tmp, dados).map_err(|e| format!("não consegui gravar {}: {e}", p.display()))?;
    std::fs::rename(&tmp, p).map_err(|e| format!("não consegui gravar {}: {e}", p.display()))
}

#[tauri::command]
async fn gravar_saida(estado: tauri::State<'_, ia::EstadoIa>, pedido: PedidoSaida) -> Result<Gravado, String> {
    let servico = estado.0.servico().to_string();
    em_segundo_plano(move || {
        let origem = PathBuf::from(&pedido.origem);
        let dir = pasta_destino(&origem, &pedido.destino);
        std::fs::create_dir_all(&dir).map_err(|e| format!("não consegui criar a pasta de saída: {e}"))?;
        let nome = origem.file_name().ok_or("origem sem nome")?.to_string_lossy().to_string();
        let base = match &pedido.nome_saida {
            Some(n) => nome_seguro(n),
            None => match nome.rfind('.') {
                Some(k) if k > 0 => nome[..k].to_string(),
                _ => nome.clone(),
            },
        };
        // a saída é sempre um PDF (também a de um .txt ou .docx), com "_anon" no fim do nome (pedido do Promotor,
        // 14/11/2026): nunca o mesmo nome do original, nem na mesma pasta
        let base = com_sufixo(&base);
        let destino_pdf = livre(&dir, &base, ".pdf", pdf_nosso);
        if mesmo_arquivo(&destino_pdf, &origem) || destino_pdf == origem {
            return Err("a pasta de saída é a dos originais: o anonimizado teria o mesmo nome e tomaria o lugar do original".into());
        }
        let produtor = format!("Incognitus {} - motor de anonimizacao {}", env!("CARGO_PKG_VERSION"), pedido.versao_motor);
        let r = pdf::gerar(&pedido.cabecalho, &pedido.paginas, &produtor)?;
        gravar_atomico(&destino_pdf, &r.bytes)?;
        let base_final = destino_pdf.file_stem().unwrap().to_string_lossy().to_string();
        let mut txt = None;
        if let Some(t) = &pedido.texto {
            let p = livre(&dir, &base_final, ".txt", txt_nosso);
            if mesmo_arquivo(&p, &origem) {
                return Err("o .txt tomaria o lugar do original".into());
            }
            gravar_atomico(&p, t.as_bytes())?;
            txt = Some(p.to_string_lossy().to_string());
        }
        let privada = dir.join(PASTA_PRIVADA);
        let (mut mapa, mut confira) = (None, None);
        if pedido.mapa.is_some() || pedido.confira.is_some() {
            std::fs::create_dir_all(&privada).map_err(|e| format!("não consegui criar a pasta do mapa: {e}"))?;
        }
        let (mut mapa_cifrado, mut aviso_cofre) = (false, None);
        if let Some(m) = &pedido.mapa {
            let p = privada.join(format!("{base_final} — MAPA (nomes reais).json"));
            // o mapa nunca deixa de ser gravado: se o chaveiro recusa a chave, ele vai em claro e a janela avisa
            match if pedido.cifrar { cofre::selar_aqui(&servico, m.as_bytes()).map(Some) } else { Ok(None) } {
                Ok(Some(envelope)) => {
                    gravar_atomico(&p, envelope.as_bytes())?;
                    mapa_cifrado = true;
                }
                Ok(None) => gravar_atomico(&p, m.as_bytes())?,
                Err(e) => {
                    gravar_atomico(&p, m.as_bytes())?;
                    aviso_cofre = Some(e);
                }
            }
            mapa = Some(p.to_string_lossy().to_string());
        }
        if let Some(c) = &pedido.confira {
            let p = privada.join(format!("{base_final} — CONFIRA (nomes reais).txt"));
            gravar_atomico(&p, c.as_bytes())?;
            confira = Some(p.to_string_lossy().to_string());
        }
        Ok(Gravado { pdf: destino_pdf.to_string_lossy().to_string(), txt, mapa, confira, sem_glifo: r.sem_glifo, folhas: pedido.paginas.len() + 1, mapa_cifrado, aviso_cofre })
    })
    .await
}

/// A pasta de saída padrão de um arquivo (ao lado do original).
#[tauri::command]
fn pasta_padrao(caminho: String) -> String {
    pasta_destino(Path::new(&caminho), &None).to_string_lossy().to_string()
}

/// O que a fila aceita: PDF e, desde 14/11/2026, .docx e .txt (o relatório, o termo… do mesmo caso).
fn aceito(p: &Path) -> bool {
    let ext = p.extension().map(|x| x.to_string_lossy().to_ascii_lowercase()).unwrap_or_default();
    let nome = p.file_name().map(|n| n.to_string_lossy().to_string()).unwrap_or_default();
    matches!(ext.as_str(), "pdf" | "docx" | "txt") && !nome.starts_with('.') && !nome.starts_with("~$")
}

/// A fila avisa quando começa e termina: no macOS, uma atividade iniciada pelo usuário mantém a página rodando com a
/// janela fora da tela (ver acordado.rs).
#[tauri::command]
fn acordado(ligar: bool) -> bool {
    acordado::acordado(ligar)
}

/// O texto de um .txt (ou .md) ou de um .docx da fila.
#[tauri::command]
async fn texto_documento(caminho: String) -> Result<String, String> {
    em_segundo_plano(move || {
        let p = PathBuf::from(&caminho);
        match p.extension().map(|x| x.to_string_lossy().to_ascii_lowercase()).as_deref() {
            Some("docx") => reid::docx_texto(&p),
            Some("txt") | Some("md") => reid::ler_texto(&p),
            _ => Err("só .txt e .docx (o PDF passa pelo pdftotext)".into()),
        }
    })
    .await
}

/// Os PDFs (e .docx e .txt) de uma pasta (sem descer às subpastas; a de saída do Incognitus fica de fora).
#[tauri::command]
fn listar_pdfs(pasta: String) -> Result<Vec<String>, String> {
    let mut v: Vec<String> = std::fs::read_dir(&pasta)
        .map_err(|e| format!("não consegui ler a pasta: {e}"))?
        .filter_map(|e| e.ok())
        .map(|e| e.path())
        .filter(|p| p.is_file() && aceito(p))
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
        } else if p.is_file() && aceito(&p) {
            v.push(c);
        }
    }
    v
}

// ── reidentificação ────────────────────────────────────────────────────────────────────────────────────────────────

/// O texto de um arquivo (o mapa .json, uma minuta .txt ou .md).
/// O mapa cifrado (cofre.rs) é aberto aqui, com a chave do chaveiro: a janela recebe o dicionário como sempre.
#[tauri::command]
async fn ler_texto(estado: tauri::State<'_, ia::EstadoIa>, caminho: String) -> Result<String, String> {
    let servico = estado.0.servico().to_string();
    em_segundo_plano(move || {
        let t = reid::ler_texto(Path::new(&caminho))?;
        if cofre::eh_cofre(&t) { cofre::abrir_aqui(&servico, &t) } else { Ok(t) }
    })
    .await
}

/// O cofre do mapa nas Configurações: se já há chave neste computador e o apelido dela (nunca a chave).
#[derive(Serialize)]
struct SituacaoCofre {
    tem_chave: bool,
    apelido: String,
}

#[tauri::command]
async fn cofre_situacao(estado: tauri::State<'_, ia::EstadoIa>) -> Result<SituacaoCofre, String> {
    let servico = estado.0.servico().to_string();
    em_segundo_plano(move || {
        let c = cofre::chaves(&servico, false)?;
        Ok(SituacaoCofre { tem_chave: c.is_some(), apelido: c.map(|(a, _)| cofre::apelido(&a)).unwrap_or_default() })
    })
    .await
}

/// A chave de recuperação, para quem usa copiar e guardar (só no clique dela; cria a chave se ainda não há).
#[tauri::command]
async fn cofre_chave_mostrar(estado: tauri::State<'_, ia::EstadoIa>) -> Result<String, String> {
    let servico = estado.0.servico().to_string();
    em_segundo_plano(move || cofre::chaves(&servico, true)?.map(|(a, _)| cofre::em_grupos(&a)).ok_or_else(|| "sem a chave do cofre".to_string())).await
}

/// A chave de outro computador, colada: passa a ser a deste (a de antes fica guardada e ainda abre os mapas dela).
#[tauri::command]
async fn cofre_chave_importar(estado: tauri::State<'_, ia::EstadoIa>, chave: String) -> Result<String, String> {
    let servico = estado.0.servico().to_string();
    em_segundo_plano(move || cofre::importar(&servico, &chave)).await
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

/// A minuta do editor da janela, em .docx (o Word, o LibreOffice, o Pages e o Google Docs abrem).
#[tauri::command]
async fn gravar_docx(caminho: String, paragrafos: Vec<docx::Paragrafo>) -> Result<(), String> {
    em_segundo_plano(move || {
        let p = Path::new(&caminho);
        if !eh_docx(p) {
            return Err("a minuta só é gravada em .docx".into());
        }
        gravar_atomico(p, &docx::montar(&paragrafos)?)
    })
    .await
}

fn eh_docx(p: &Path) -> bool {
    p.extension().map(|e| e.to_string_lossy().eq_ignore_ascii_case("docx")).unwrap_or(false)
}

/// Abre um .docx no editor de texto do computador (o programa padrão: Word, LibreOffice, Pages…). Só .docx que existe:
/// a janela não manda o sistema abrir outra coisa.
#[tauri::command]
fn abrir_documento(caminho: String) -> Result<(), String> {
    let p = PathBuf::from(&caminho);
    if !eh_docx(&p) || !p.is_file() {
        return Err("só abro um .docx que já foi gravado".into());
    }
    #[cfg(target_os = "macos")]
    let r = std::process::Command::new("open").arg(&p).spawn();
    #[cfg(target_os = "windows")]
    let r = std::process::Command::new("explorer").arg(&p).spawn();
    #[cfg(not(any(target_os = "macos", target_os = "windows")))]
    let r = std::process::Command::new("xdg-open").arg(&p).spawn();
    r.map(|_| ()).map_err(|e| format!("não consegui abrir o documento: {e}"))
}

/// Abre o template (o modelo) que a minuta usou: o arquivo da pasta de modelos (.docx, .txt, .md) no programa padrão, ou o
/// link do Google Docs, do Drive, do OneDrive ou do SharePoint no navegador. Outro endereço, não.
#[tauri::command]
fn abrir_modelo(alvo: String) -> Result<(), String> {
    let a = alvo.trim();
    let abrir: String = if a.starts_with("https://") {
        let host = a[8..].split(['/', '?', '#']).next().unwrap_or("").to_ascii_lowercase();
        let ok = ["docs.google.com", "drive.google.com", "1drv.ms", "onedrive.live.com"].contains(&host.as_str()) || host.ends_with(".sharepoint.com");
        if !ok || a.contains(char::is_whitespace) {
            return Err("só abro modelos do Google Docs, do Drive, do OneDrive e do SharePoint".into());
        }
        a.to_string()
    } else {
        let p = PathBuf::from(a);
        let ext = p.extension().map(|x| x.to_string_lossy().to_ascii_lowercase()).unwrap_or_default();
        if !["docx", "txt", "md", "gdoc"].contains(&ext.as_str()) || !p.is_file() {
            return Err("não achei o arquivo do modelo (foi movido ou apagado?)".into());
        }
        a.to_string()
    };
    #[cfg(target_os = "macos")]
    let r = std::process::Command::new("open").arg(&abrir).spawn();
    #[cfg(target_os = "windows")]
    let r = if abrir.starts_with("https://") {
        std::process::Command::new("rundll32").arg("url.dll,FileProtocolHandler").arg(&abrir).spawn()
    } else {
        std::process::Command::new("explorer").arg(&abrir).spawn()
    };
    #[cfg(not(any(target_os = "macos", target_os = "windows")))]
    let r = std::process::Command::new("xdg-open").arg(&abrir).spawn();
    r.map(|_| ()).map_err(|e| format!("não consegui abrir o modelo: {e}"))
}

// ── a Mesa de trabalho (18/11/2026): os casos e os campos do Graphé (status, autor, prazo, feedbacks…), gravados num
// arquivo só, nos dados do app. O formato já é o da sincronização futura (docs/SINCRONIZACAO.md): cada campo guarda o
// valor, a hora e quem mudou; os caminhos dos arquivos ficam numa parte "local", que nunca é sincronizada.
const ARQUIVO_MESA: &str = "mesa.json";

#[tauri::command]
async fn mesa_ler(estado: State<'_, Estado>) -> Result<String, String> {
    let p = estado.0.dados.join(ARQUIVO_MESA);
    em_segundo_plano(move || Ok(std::fs::read_to_string(p).unwrap_or_default())).await
}

#[tauri::command]
async fn mesa_gravar(estado: State<'_, Estado>, texto: String) -> Result<(), String> {
    if texto.len() > 20_000_000 || serde_json::from_str::<serde_json::Value>(&texto).is_err() {
        return Err("a mesa não pôde ser gravada (formato)".into());
    }
    let p = estado.0.dados.join(ARQUIVO_MESA);
    em_segundo_plano(move || {
        let tmp = p.with_extension("json.tmp");
        std::fs::write(&tmp, texto).map_err(|e| format!("não consegui gravar a mesa: {e}"))?;
        std::fs::rename(&tmp, &p).map_err(|e| format!("não consegui gravar a mesa: {e}"))
    })
    .await
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

// ── o modelo da anonimização rigorosa (opcional; baixado na primeira vez) ─────────────────────────────────────────

#[tauri::command]
async fn ner_estado(estado: State<'_, Estado>) -> Result<ner::EstadoNer, String> {
    let dados = estado.0.dados.clone();
    em_segundo_plano(move || Ok(ner::estado(&dados))).await
}

#[tauri::command]
async fn ner_baixar(estado: State<'_, Estado>) -> Result<ner::EstadoNer, String> {
    ner::baixar(&estado.0.dados).await
}

/// Os bytes do model.onnx ou do tokenizer.json, crus (sem passar por JSON).
#[tauri::command]
async fn ner_ler(estado: State<'_, Estado>, arquivo: String) -> Result<tauri::ipc::Response, String> {
    let dados = estado.0.dados.clone();
    let b = em_segundo_plano(move || ner::ler(&dados, &arquivo)).await?;
    Ok(tauri::ipc::Response::new(b))
}

// ── a busca nos modelos da pasta (17/11/2026): o BERT de frases, a lista da pasta e o índice dos vetores ──────────────

#[tauri::command]
async fn busca_estado(estado: State<'_, Estado>) -> Result<ner::EstadoNer, String> {
    let dados = estado.0.dados.clone();
    em_segundo_plano(move || Ok(ner::estado_de(&ner::BUSCA, &dados))).await
}

#[tauri::command]
async fn busca_baixar(estado: State<'_, Estado>) -> Result<ner::EstadoNer, String> {
    ner::baixar_de(&ner::BUSCA, &estado.0.dados).await
}

#[tauri::command]
async fn busca_ler(estado: State<'_, Estado>, arquivo: String) -> Result<tauri::ipc::Response, String> {
    let dados = estado.0.dados.clone();
    let b = em_segundo_plano(move || ner::ler_de(&ner::BUSCA, &dados, &arquivo)).await?;
    Ok(tauri::ipc::Response::new(b))
}

/// Os modelos da pasta do computador (e das subpastas): .docx, .txt, .md e os .gdoc do Google Drive para computador.
#[tauri::command]
async fn modelos_listar(pasta: String) -> Result<Vec<pastas::ArquivoModelo>, String> {
    em_segundo_plano(move || pastas::listar_local(Path::new(&pasta))).await
}

const INDICE_MODELOS: &str = "busca-modelos.json";

/// O índice da busca (os vetores dos modelos, para não refazer a cada 🧠). "" se ainda não há.
#[tauri::command]
async fn indice_ler(estado: State<'_, Estado>) -> Result<String, String> {
    let p = estado.0.dados.join(INDICE_MODELOS);
    em_segundo_plano(move || Ok(std::fs::read_to_string(p).unwrap_or_default())).await
}

#[tauri::command]
async fn indice_gravar(estado: State<'_, Estado>, texto: String) -> Result<(), String> {
    if texto.len() > 80_000_000 {
        return Err("o índice da busca ficou grande demais".into());
    }
    let p = estado.0.dados.join(INDICE_MODELOS);
    em_segundo_plano(move || {
        let tmp = p.with_extension("json.tmp");
        std::fs::write(&tmp, texto).map_err(|e| format!("não consegui gravar o índice: {e}"))?;
        std::fs::rename(&tmp, &p).map_err(|e| format!("não consegui gravar o índice: {e}"))
    })
    .await
}

/// Erros e marcos do front no stderr (para diagnóstico; nunca leva texto dos autos).
#[tauri::command]
fn registrar(msg: String) {
    eprintln!("[incognitus] {}", msg.chars().take(500).collect::<String>());
}

// ── teste de ponta a ponta (só com SKIA_E2E apontando para um plano) ──────────────────────────────────────────────

fn plano_e2e() -> Option<(PathBuf, serde_json::Value)> {
    let p = PathBuf::from(std::env::var_os("SKIA_E2E")?);
    let v = serde_json::from_slice(&std::fs::read(&p).ok()?).ok()?;
    Some((p, v))
}

/// O plano do SKIA_E2E abre (JSON em UTF-8)? O app avulso pergunta antes de montar a janela.
pub fn plano_e2e_abre() -> bool {
    plano_e2e().is_some()
}

/// O modo de teste é do app avulso: hospedado, o módulo não segue plano nem fecha o app de quem o hospeda.
fn plano_do(estado: &Estado) -> Option<(PathBuf, serde_json::Value)> {
    if estado.0.hospedado {
        return None;
    }
    plano_e2e()
}

#[tauri::command]
fn e2e_plano(estado: State<'_, Estado>) -> Option<serde_json::Value> {
    plano_do(&estado).map(|(_, v)| v)
}

#[tauri::command]
fn e2e_gravar(estado: State<'_, Estado>, nome: String, conteudo: String) -> Result<(), String> {
    let (_, v) = plano_do(&estado).ok_or("fora do modo de teste")?;
    let dir = PathBuf::from(v["extracao"].as_str().ok_or("plano sem pasta de extração")?);
    std::fs::create_dir_all(&dir).map_err(|e| e.to_string())?;
    let nome = Path::new(&nome).file_name().ok_or("nome inválido")?.to_owned();
    std::fs::write(dir.join(nome), conteudo).map_err(|e| e.to_string())
}

#[tauri::command]
fn e2e_fim<R: Runtime>(app: tauri::AppHandle<R>, estado: State<'_, Estado>, relatorio: String) -> Result<(), String> {
    let (_, v) = plano_do(&estado).ok_or("fora do modo de teste")?;
    let p = PathBuf::from(v["relatorio"].as_str().ok_or("plano sem relatório")?);
    std::fs::write(p, relatorio).map_err(|e| e.to_string())?;
    // "manter": a janela fica aberta no fim (para a foto do README); senão, o app sai.
    if !v["manter"].as_bool().unwrap_or(false) {
        app.exit(0);
    }
    Ok(())
}

// ── início ─────────────────────────────────────────────────────────────────────────────────────────────────────────

/// As pastas de dados e de cache do módulo (contrato, §1.1): no app avulso, a raiz das pastas do app, como sempre foi
/// (nada a migrar); hospedado, `modulos/incognitus/` dentro das do hospedeiro.
fn pastas(dados: PathBuf, cache: PathBuf, hospedado: bool) -> (PathBuf, PathBuf) {
    if hospedado {
        (dados.join("modulos").join(ID), cache.join("modulos").join(ID))
    } else {
        (dados, cache)
    }
}

fn montar<R: Runtime>(hospedado: bool) -> TauriPlugin<R> {
    Builder::new("anonimizacao")
        .invoke_handler(tauri::generate_handler![
            info,
            info_app,
            preparar_arquivo,
            extrair_texto,
            ocr_pagina,
            limpar_arquivo,
            texto_documento,
            acordado,
            gravar_saida,
            pasta_padrao,
            listar_pdfs,
            expandir_caminhos,
            ler_texto,
            docx_partes,
            gravar_reidentificado,
            gravar_texto,
            gravar_docx,
            abrir_documento,
            abrir_modelo,
            mesa_ler,
            mesa_gravar,
            revelar,
            ajustes_ler,
            ajustes_gravar,
            motor_ativo,
            motor_checar,
            motor_baixar,
            motor_instalar,
            motor_voltar,
            ner_estado,
            ner_baixar,
            ner_ler,
            busca_estado,
            busca_baixar,
            busca_ler,
            modelos_listar,
            indice_ler,
            indice_gravar,
            registrar,
            e2e_plano,
            e2e_gravar,
            e2e_fim,
            ia::ia_situacao,
            ia::ia_chave_guardar,
            ia::ia_chave_apagar,
            ia::ia_google_entrar,
            ia::ia_google_cancelar,
            ia::ia_google_sair,
            ia::abrir_feedback,
            ia::ia_prompt_ler,
            ia::ia_enviar,
            ia::ia_modelos,
            ia::ia_pasta_listar,
            ia::ia_gdocs_enviar,
            cofre_situacao,
            cofre_chave_mostrar,
            cofre_chave_importar,
            visor::visor_abrir
        ])
        .register_asynchronous_uri_scheme_protocol(visor::ESQUEMA, visor::servir)
        .setup(move |app, _api| {
            // SKIA_DADOS (só para testes): dados e cache numa pasta à parte, sem tocar nos do usuário.
            let (cache, dados) = match std::env::var_os("SKIA_DADOS") {
                Some(d) => (PathBuf::from(&d).join("cache"), PathBuf::from(&d).join("dados")),
                None => (app.path().app_cache_dir()?, app.path().app_data_dir()?),
            };
            let (dados, cache) = pastas(dados, cache, hospedado);
            // Os sidecars ficam ao lado do executável do app e os recursos (tessdata/, lib/) na pasta de recursos dele,
            // nos mesmos caminhos no app avulso e no hospedeiro (o `para` do modulo.json).
            let recursos = app.path().resource_dir()?;
            let bin = std::env::current_exe()?.parent().map(Path::to_path_buf).unwrap_or_default();
            // Sobras de uma execução interrompida: a pasta temporária começa vazia.
            let _ = std::fs::remove_dir_all(cache.join("tmp"));
            std::fs::create_dir_all(cache.join("tmp"))?;
            std::fs::create_dir_all(&dados)?;
            let interno = Interno {
                hospedado,
                ferramentas: ferramentas::Ferramentas::new(bin, recursos, &cache),
                cache,
                dados,
                arquivos: Mutex::new(HashMap::new()),
                contador: AtomicU64::new(1),
            };
            app.manage(ia::novo(&app.config().identifier, hospedado, interno.dados.clone()));
            app.manage(visor::Visor::default());
            app.manage(Estado(Arc::new(interno)));
            Ok(())
        })
        // no Mac, a página não dorme com a janela fora da tela (o lote congelava; ver acordado.rs)
        .on_webview_ready(|webview| {
            #[cfg(target_os = "macos")]
            {
                let _ = webview.with_webview(|pw| acordado::webview_sem_sono(pw.inner()));
            }
            #[cfg(not(target_os = "macos"))]
            let _ = webview;
        })
        .on_event(|app, ev| {
            if let tauri::RunEvent::Exit = ev {
                if let Some(e) = app.try_state::<Estado>() {
                    let _ = std::fs::remove_dir_all(e.0.cache.join("tmp"));
                }
            }
        })
        .build()
}

/// App avulso: os dados ficam na raiz da pasta de dados do app, e o app cuida da própria atualização.
pub fn init<R: Runtime>() -> TauriPlugin<R> {
    montar(false)
}

/// Hospedado no Graphé Desktop: os dados ficam em `<dados do hospedeiro>/modulos/incognitus/` e o cache em
/// `<cache do hospedeiro>/modulos/incognitus/`; a atualização do app é do hospedeiro (a do motor continua aqui).
pub fn init_como_modulo<R: Runtime>() -> TauriPlugin<R> {
    montar(true)
}

#[cfg(test)]
mod testes {
    use super::*;

    #[test]
    fn nome_de_saida_seguro() {
        assert_eq!(nome_seguro("relatorio - «Thanasis»"), "relatorio - «Thanasis»");
        assert_eq!(nome_seguro(" a/b:c*?  d. "), "a-b-c-- d");
        assert_eq!(nome_seguro("../.."), "-");
        assert_eq!(nome_seguro("   "), "documento");
        assert_eq!(com_sufixo("relatorio - «Thanasis»"), "relatorio - «Thanasis»_anon");
        assert_eq!(com_sufixo("autos_ANON"), "autos_ANON");
        assert!(aceito(Path::new("x/Relatório.DOCX")) && aceito(Path::new("a.txt")) && aceito(Path::new("a.pdf")));
        assert!(!aceito(Path::new("~$rascunho.docx")) && !aceito(Path::new(".oculto.pdf")) && !aceito(Path::new("a.json")));
    }

    #[test]
    fn pastas_do_avulso_e_do_hospedado() {
        let (d, c) = (PathBuf::from("dados"), PathBuf::from("cache"));
        assert_eq!(pastas(d.clone(), c.clone(), false), (d.clone(), c.clone()));
        assert_eq!(pastas(d.clone(), c.clone(), true), (d.join("modulos").join("incognitus"), c.join("modulos").join("incognitus")));
    }

    /// O modulo.json, o build.rs e o permissions/default.toml dizem o mesmo que este arquivo.
    #[test]
    fn modulo_json_confere_com_o_plugin() {
        let m: serde_json::Value = serde_json::from_str(include_str!("../../modulo/modulo.json")).expect("modulo.json");
        assert_eq!(m["contrato"], 1);
        assert_eq!(m["id"], ID);
        assert_eq!(m["dados"], ID);
        assert_eq!(m["nome"], NOME);
        assert_eq!(m["versao"], VERSAO);
        assert_eq!(m["plugin"]["nome"], "anonimizacao");
        assert_eq!(m["plugin"]["crate"], env!("CARGO_PKG_NAME"));
        let lista = |v: &serde_json::Value| -> Vec<String> { v.as_array().expect("lista").iter().map(|x| x.as_str().unwrap().to_string()).collect() };
        assert!(lista(&m["permissoes"]).contains(&"anonimizacao:default".to_string()));
        assert_eq!(lista(&m["api"]["permissoes"]), ["anonimizacao:api"]);
        let sidecars: Vec<String> = m["sidecars"].as_array().unwrap().iter().map(|s| s["nome"].as_str().unwrap().to_string()).collect();
        assert_eq!(sidecars, ["pdftotext", "pdftoppm", "tesseract"]);
        // os arquivos da api e a entrada da janela existem em modulo/
        let pasta = Path::new(env!("CARGO_MANIFEST_DIR")).join("../modulo");
        for a in lista(&m["api"]["arquivos"]).iter().chain([m["janela"]["entrada"].as_str().unwrap().to_string(), m["icone"].as_str().unwrap().to_string()].iter()) {
            assert!(pasta.join(a).is_file(), "falta modulo/{a}");
        }
        // cada comando do build.rs está no conjunto `default`; o `api` só tem comandos que existem
        let build = include_str!("../build.rs");
        let comandos: Vec<String> = build.split('"').skip(1).step_by(2).map(|c| c.to_string()).collect();
        assert_eq!(comandos.len(), 58);
        let toml = include_str!("../permissions/default.toml");
        let (padrao, api) = toml.split_once("[[set]]").expect("o conjunto api");
        for c in &comandos {
            assert!(padrao.contains(&format!("\"allow-{}\"", c.replace('_', "-"))), "{c} fora do default");
        }
        let da_api: Vec<&str> = api.split('"').filter(|x| x.starts_with("allow-")).collect();
        assert_eq!(da_api, ["allow-info", "allow-preparar-arquivo", "allow-extrair-texto", "allow-ocr-pagina", "allow-limpar-arquivo", "allow-motor-ativo"]);
        assert!(da_api.iter().all(|a| comandos.contains(&a["allow-".len()..].replace('-', "_"))));
    }
}
