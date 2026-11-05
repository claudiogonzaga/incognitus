//! Os sidecars — pdftotext, pdftoppm e tesseract —, chamados como o serviço do Graphé os chama
//! (cloudrun-pdftotext/main.py): `pdftotext -layout arquivo -`, e para o OCR de uma página
//! `pdftoppm -r 200 -gray -png -f p -l p` seguido de `tesseract imagem stdout -l por`.
//! Prioridade baixa, no máximo 2 OCRs ao mesmo tempo, o tesseract com um núcleo só (OMP_THREAD_LIMIT=1, como no
//! serviço). A saída é lida como BYTES: o stderr do tesseract no Mac pode trazer bytes que não são UTF-8.
//! As ferramentas só veem a CÓPIA do PDF na pasta temporária do app (o original nunca é aberto por elas), e a pasta
//! temporária nunca começa por /tmp (o Leptonica do Mac reescreve esses caminhos e a imagem da página "some").
use std::io::Read;
use std::path::{Path, PathBuf};
use std::process::{Child, Command, Stdio};
use std::sync::{Condvar, Mutex};
use std::time::{Duration, Instant};

pub const OCR_DPI: u32 = 200;
pub const OCR_LANG: &str = "por";
const OCR_MAX_SIMULTANEOS: usize = 2;
const PDFTOTEXT_TEMPO: Duration = Duration::from_secs(900);
const OCR_TEMPO_PAGINA: Duration = Duration::from_secs(180);

pub struct Ferramentas {
    bin: PathBuf,
    lib: PathBuf,
    tessdata: PathBuf,
    fontconfig: PathBuf,
    vagas: Mutex<usize>,
    sinal: Condvar,
}

/// A vaga de OCR: devolvida ao sair do escopo.
struct Vaga<'a>(&'a Ferramentas);
impl Drop for Vaga<'_> {
    fn drop(&mut self) {
        *self.0.vagas.lock().unwrap() += 1;
        self.0.sinal.notify_one();
    }
}

/// No Windows o Tauri dá a pasta de recursos como caminho "verbatim" (`\\?\C:\…`, do canonicalize): o tesseract
/// emenda o modelo com "/" e o caminho verbatim não aceita "/" — o OCR não achava o por.traineddata. Tira o prefixo.
fn simplificar(p: PathBuf) -> PathBuf {
    #[cfg(windows)]
    {
        if let Some(s) = p.to_str() {
            if let Some(r) = s.strip_prefix(r"\\?\UNC\") {
                return PathBuf::from(format!(r"\\{r}"));
            }
            if let Some(r) = s.strip_prefix(r"\\?\") {
                if r.as_bytes().get(1) == Some(&b':') {
                    return PathBuf::from(r);
                }
            }
        }
    }
    p
}

impl Ferramentas {
    /// `bin`: a pasta do executável do app (onde o Tauri põe os sidecars); `recursos`: a pasta de recursos;
    /// `cache`: a pasta de cache do app (configuração do fontconfig).
    pub fn new(bin: PathBuf, recursos: PathBuf, cache: &Path) -> Self {
        let (bin, recursos) = (simplificar(bin), simplificar(recursos));
        let lib = recursos.join("lib");
        let fontconfig = cache.join("fonts.conf");
        let dirs = if cfg!(windows) {
            "<dir>WINDOWSFONTDIR</dir>".to_string()
        } else {
            "<dir>/System/Library/Fonts</dir><dir>/Library/Fonts</dir><dir>~/Library/Fonts</dir>".to_string()
        };
        let conf = format!(
            "<?xml version=\"1.0\"?>\n<!DOCTYPE fontconfig SYSTEM \"urn:fontconfig:fonts.dtd\">\n<fontconfig>{dirs}<cachedir>{}</cachedir></fontconfig>\n",
            cache.join("fontconfig").display()
        );
        let _ = std::fs::create_dir_all(cache);
        let _ = std::fs::write(&fontconfig, conf);
        Ferramentas { bin, lib, tessdata: recursos.join("tessdata"), fontconfig, vagas: Mutex::new(OCR_MAX_SIMULTANEOS), sinal: Condvar::new() }
    }

    fn caminho(&self, nome: &str) -> PathBuf {
        self.bin.join(if cfg!(windows) { format!("{nome}.exe") } else { nome.to_string() })
    }

    fn comando(&self, nome: &str, tmp: Option<&Path>) -> Command {
        let mut c = Command::new(self.caminho(nome));
        c.env("FONTCONFIG_FILE", &self.fontconfig).env("OMP_THREAD_LIMIT", "1");
        if let Some(t) = tmp {
            c.env("TMPDIR", t).env("TMP", t).env("TEMP", t);
        }
        #[cfg(windows)]
        {
            use std::os::windows::process::CommandExt;
            let path = std::env::var_os("PATH").unwrap_or_default();
            let mut novo = std::ffi::OsString::from(self.lib.as_os_str());
            novo.push(";");
            novo.push(path);
            c.env("PATH", novo);
            // CREATE_NO_WINDOW | BELOW_NORMAL_PRIORITY_CLASS
            c.creation_flags(0x0800_0000 | 0x0000_4000);
        }
        #[cfg(unix)]
        {
            use std::os::unix::process::CommandExt;
            unsafe {
                c.pre_exec(|| {
                    libc::setpriority(libc::PRIO_PROCESS, 0, 10);
                    Ok(())
                });
            }
        }
        let _ = &self.lib;
        c.stdin(Stdio::null()).stdout(Stdio::piped()).stderr(Stdio::piped());
        c
    }

    fn vaga(&self) -> Vaga<'_> {
        let mut v = self.vagas.lock().unwrap();
        while *v == 0 {
            v = self.sinal.wait(v).unwrap();
        }
        *v -= 1;
        Vaga(self)
    }

    /// As versões das ferramentas (a conferência de que os sidecars rodam).
    pub fn versoes(&self) -> Result<Vec<String>, String> {
        let mut out = Vec::new();
        for (nome, arg) in [("pdftotext", "-v"), ("pdftoppm", "-v"), ("tesseract", "--version")] {
            let r = rodar(self.comando(nome, None).arg(arg), Duration::from_secs(30))?;
            let t = String::from_utf8_lossy(if r.saida.is_empty() { &r.erro } else { &r.saida }).to_string();
            out.push(t.lines().next().unwrap_or("").trim().to_string());
        }
        if !self.tessdata.join(format!("{OCR_LANG}.traineddata")).exists() {
            return Err(format!("falta o {OCR_LANG}.traineddata em {}", self.tessdata.display()));
        }
        Ok(out)
    }

    /// `pdftotext -layout arquivo -`: o texto com um \f no fim de cada página (bytes inválidos viram U+FFFD).
    pub fn pdftotext(&self, pdf: &Path, tmp: &Path) -> Result<String, String> {
        let r = rodar(self.comando("pdftotext", Some(tmp)).arg("-layout").arg(pdf).arg("-"), PDFTOTEXT_TEMPO)?;
        if !r.ok {
            return Err(format!("pdftotext falhou: {}", resumo(&r.erro)));
        }
        Ok(String::from_utf8_lossy(&r.saida).into_owned())
    }

    /// O OCR de UMA página: pdftoppm só dela, a 200 dpi em cinza, e o tesseract em português. Devolve a saída crua do
    /// tesseract (o motor a normaliza como o serviço: \f vira \n, espaço do fim sai, uma quebra no fim).
    pub fn ocr_pagina(&self, pdf: &Path, pagina: u32, tmp: &Path) -> Result<String, String> {
        let _vaga = self.vaga();
        let dir = tmp.join(format!("ocr-{pagina}"));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).map_err(|e| format!("pasta do OCR: {e}"))?;
        let resultado = (|| {
            let inicio = Instant::now();
            let p = pagina.to_string();
            let r = rodar(
                self.comando("pdftoppm", Some(tmp))
                    .args(["-r", &OCR_DPI.to_string(), "-gray", "-png", "-f", &p, "-l", &p])
                    .arg(pdf)
                    .arg(dir.join("p")),
                OCR_TEMPO_PAGINA,
            )?;
            if !r.ok {
                return Err(format!("pdftoppm falhou na página {pagina}: {}", resumo(&r.erro)));
            }
            let mut imagens: Vec<PathBuf> = std::fs::read_dir(&dir)
                .map_err(|e| format!("pasta do OCR: {e}"))?
                .filter_map(|e| e.ok().map(|e| e.path()))
                .filter(|p| p.extension().map_or(false, |x| x == "png"))
                .collect();
            imagens.sort();
            let Some(img) = imagens.first() else { return Ok(String::new()) };
            let resta = OCR_TEMPO_PAGINA.saturating_sub(inicio.elapsed()).max(Duration::from_secs(1));
            // o modelo pelo caminho relativo, a partir da pasta dele: nenhum caminho da instalação (que pode ter acento)
            // passa pelo argv de 8 bits do tesseract
            let r = rodar(
                self.comando("tesseract", Some(tmp)).current_dir(&self.tessdata).arg(img).arg("stdout").args(["-l", OCR_LANG, "--tessdata-dir", "."]),
                resta,
            )?;
            if !r.ok {
                return Err(format!("tesseract falhou na página {pagina}: {}", resumo(&r.erro)));
            }
            Ok(String::from_utf8_lossy(&r.saida).into_owned())
        })();
        let _ = std::fs::remove_dir_all(&dir);
        resultado
    }
}

struct Saida {
    ok: bool,
    saida: Vec<u8>,
    erro: Vec<u8>,
}

/// Roda com prazo; stdout e stderr lidos em paralelo (o pdftotext de autos grandes enche o cano).
fn rodar(c: &mut Command, prazo: Duration) -> Result<Saida, String> {
    let mut filho: Child = c.spawn().map_err(|e| format!("não consegui rodar {:?}: {e}", c.get_program()))?;
    let mut so = filho.stdout.take().unwrap();
    let mut se = filho.stderr.take().unwrap();
    let t1 = std::thread::spawn(move || {
        let mut b = Vec::new();
        let _ = so.read_to_end(&mut b);
        b
    });
    let t2 = std::thread::spawn(move || {
        let mut b = Vec::new();
        let _ = se.read_to_end(&mut b);
        b
    });
    let inicio = Instant::now();
    let status = loop {
        match filho.try_wait() {
            Ok(Some(s)) => break s,
            Ok(None) if inicio.elapsed() > prazo => {
                let _ = filho.kill();
                let _ = filho.wait();
                return Err(format!("tempo esgotado ({} s)", prazo.as_secs()));
            }
            Ok(None) => std::thread::sleep(Duration::from_millis(50)),
            Err(e) => return Err(format!("espera do processo: {e}")),
        }
    };
    let saida = t1.join().unwrap_or_default();
    let erro = t2.join().unwrap_or_default();
    Ok(Saida { ok: status.success(), saida, erro })
}

fn resumo(b: &[u8]) -> String {
    let s = String::from_utf8_lossy(b);
    let s = s.trim();
    s.chars().take(200).collect()
}
