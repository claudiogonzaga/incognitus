//! O 👁 do card: o PDF (o anonimizado ou o original) numa janela do próprio Incognitus, com o leitor de PDF do sistema
//! (o do WebKit no Mac, o do Edge no Windows), sem abrir o Finder nem outro programa.
//!
//! A janela carrega o arquivo por um protocolo próprio (incognitus-visor://localhost/<n>/<nome>, ou
//! http://incognitus-visor.localhost/… no Windows), que só serve os arquivos que a janela principal mandou abrir, e
//! só para leitura. O original nunca é copiado nem alterado.
use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Mutex;
use tauri::{Manager, Runtime};

pub const ESQUEMA: &str = "incognitus-visor";

#[derive(Default)]
pub struct Visor {
    arquivos: Mutex<HashMap<u64, PathBuf>>,
    contador: AtomicU64,
}

fn tipo(p: &std::path::Path) -> Option<&'static str> {
    match p.extension().map(|x| x.to_string_lossy().to_ascii_lowercase()).as_deref() {
        Some("pdf") => Some("application/pdf"),
        Some("txt") | Some("md") => Some("text/plain; charset=utf-8"),
        _ => None,
    }
}

/// O número do arquivo no endereço "/<n>/<nome>".
pub fn numero_do_caminho(caminho: &str) -> Option<u64> {
    caminho.trim_start_matches('/').split('/').next()?.parse().ok()
}

fn resposta(codigo: u16, tipo: &str, corpo: Vec<u8>) -> tauri::http::Response<Vec<u8>> {
    tauri::http::Response::builder()
        .status(codigo)
        .header("content-type", tipo)
        .header("cache-control", "no-store")
        .body(corpo)
        .unwrap()
}

/// O protocolo: devolve os bytes do arquivo registrado (lidos fora da linha da janela).
pub fn servir<R: Runtime>(ctx: tauri::UriSchemeContext<'_, R>, req: tauri::http::Request<Vec<u8>>, resp: tauri::UriSchemeResponder) {
    let alvo = numero_do_caminho(req.uri().path()).and_then(|n| ctx.app_handle().try_state::<Visor>().and_then(|v| v.arquivos.lock().unwrap().get(&n).cloned()));
    std::thread::spawn(move || {
        let r = match alvo {
            None => resposta(404, "text/plain; charset=utf-8", "Este arquivo não está aberto no Incognitus.".into()),
            Some(p) => match (std::fs::read(&p), tipo(&p)) {
                (Ok(b), Some(t)) => resposta(200, t, b),
                (Err(e), _) => resposta(404, "text/plain; charset=utf-8", format!("Não consegui ler o arquivo: {e}").into_bytes()),
                (_, None) => resposta(415, "text/plain; charset=utf-8", "Esse tipo de arquivo não abre aqui.".into()),
            },
        };
        resp.respond(r);
    });
}

fn endereco(n: u64, nome: &str) -> String {
    let nome: String = url_escape(nome);
    if cfg!(windows) {
        format!("http://{ESQUEMA}.localhost/{n}/{nome}")
    } else {
        format!("{ESQUEMA}://localhost/{n}/{nome}")
    }
}

fn url_escape(s: &str) -> String {
    let mut out = String::new();
    for b in s.bytes() {
        if b.is_ascii_alphanumeric() || b"-_.~".contains(&b) {
            out.push(b as char);
        } else {
            out.push_str(&format!("%{b:02X}"));
        }
    }
    out
}

/// Abre o arquivo numa janela nova do Incognitus. `rotulo` vai no título ("anonimizado", "original").
/// É assíncrono de propósito: criar janela num comando síncrono trava (ele roda na linha principal, que a janela usa).
#[tauri::command]
pub async fn visor_abrir<R: Runtime>(app: tauri::AppHandle<R>, caminho: String, rotulo: String) -> Result<(), String> {
    let p = PathBuf::from(&caminho);
    if !p.is_file() {
        return Err("Não achei o arquivo (foi movido ou apagado?).".into());
    }
    if tipo(&p).is_none() {
        return Err("Só PDF e texto abrem no visor; um .docx abre no Word (Mostrar na pasta).".into());
    }
    let visor = app.try_state::<Visor>().ok_or("o visor não está pronto")?;
    let n = visor.contador.fetch_add(1, Ordering::SeqCst) + 1;
    visor.arquivos.lock().unwrap().insert(n, p.clone());
    let nome = p.file_name().map(|x| x.to_string_lossy().to_string()).unwrap_or_else(|| "documento.pdf".into());
    let url = endereco(n, &nome).parse().map_err(|e| format!("endereço do visor: {e}"))?;
    let titulo = if rotulo.trim().is_empty() { format!("👁 {nome}") } else { format!("👁 {nome} — {}", rotulo.trim()) };
    tauri::WebviewWindowBuilder::new(&app, format!("visor-{n}"), tauri::WebviewUrl::CustomProtocol(url))
        .title(titulo)
        .inner_size(900.0, 1000.0)
        .min_inner_size(420.0, 400.0)
        .build()
        .map(|_| ())
        .map_err(|e| format!("não consegui abrir a janela do visor: {e}"))
}

#[cfg(test)]
mod testes {
    use super::*;

    #[test]
    fn o_endereco_do_visor() {
        assert_eq!(numero_do_caminho("/12/autos_anon.pdf"), Some(12));
        assert_eq!(numero_do_caminho("12"), Some(12));
        assert_eq!(numero_do_caminho("/x/autos.pdf"), None);
        let e = endereco(3, "custódia 1/2.pdf");
        assert!(e.ends_with("/3/cust%C3%B3dia%201%2F2.pdf"), "{e}");
        assert!(tipo(std::path::Path::new("a.PDF")).is_some() && tipo(std::path::Path::new("a.docx")).is_none());
    }
}
