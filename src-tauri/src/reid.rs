//! A reidentificação de arquivos: ler a minuta (.txt, .md, .docx) e gravar a cópia com os nomes de volta, ao lado da
//! original, com "(reidentificado)" no nome. A troca em si é do motor, no front (as mesmas funções do 🔓 do Graphé);
//! aqui fica o que é do sistema: ler o arquivo, abrir o .docx (um zip) e gravar a cópia sem tocar na original nem nas
//! partes do .docx que não mudaram.
use serde::{Deserialize, Serialize};
use std::io::{Read, Write};
use std::path::Path;

const LIMITE: u64 = 64 * 1024 * 1024;

/// O texto de um arquivo: UTF-8 (com ou sem BOM) ou, se não for, Windows-1252 (o "ANSI" do Bloco de Notas antigo).
pub fn ler_texto(p: &Path) -> Result<String, String> {
    let meta = std::fs::metadata(p).map_err(|e| format!("não consegui ler {}: {e}", p.display()))?;
    if meta.len() > LIMITE {
        return Err("arquivo grande demais".into());
    }
    let b = std::fs::read(p).map_err(|e| format!("não consegui ler {}: {e}", p.display()))?;
    Ok(decodificar(&b))
}

pub fn decodificar(b: &[u8]) -> String {
    let b = b.strip_prefix(&[0xEF, 0xBB, 0xBF]).unwrap_or(b);
    match std::str::from_utf8(b) {
        Ok(s) => s.to_string(),
        Err(_) => b.iter().map(|&c| cp1252(c)).collect(),
    }
}

fn cp1252(c: u8) -> char {
    const ALTOS: [u32; 32] = [
        0x20AC, 0x81, 0x201A, 0x0192, 0x201E, 0x2026, 0x2020, 0x2021, 0x02C6, 0x2030, 0x0160, 0x2039, 0x0152, 0x8D, 0x017D, 0x8F,
        0x90, 0x2018, 0x2019, 0x201C, 0x201D, 0x2022, 0x2013, 0x2014, 0x02DC, 0x2122, 0x0161, 0x203A, 0x0153, 0x9D, 0x017E, 0x0178,
    ];
    if (0x80..0xA0).contains(&c) {
        char::from_u32(ALTOS[(c - 0x80) as usize]).unwrap_or('?')
    } else {
        c as char
    }
}

#[derive(Serialize, Deserialize, Clone)]
pub struct Parte {
    pub nome: String,
    pub xml: String,
}

/// As partes do .docx que têm texto: o corpo, os cabeçalhos, os rodapés, as notas e os comentários.
pub fn parte_com_texto(nome: &str) -> bool {
    let n = nome.to_ascii_lowercase();
    let Some(f) = n.strip_prefix("word/").and_then(|r| r.strip_suffix(".xml")) else { return false };
    if f.contains('/') {
        return false;
    }
    let numerado = |pre: &str| f.strip_prefix(pre).map_or(false, |r| r.chars().all(|c| c.is_ascii_digit()));
    matches!(f, "document" | "footnotes" | "endnotes" | "comments") || numerado("header") || numerado("footer")
}

pub fn docx_partes(p: &Path) -> Result<Vec<Parte>, String> {
    let f = std::fs::File::open(p).map_err(|e| format!("não consegui abrir {}: {e}", p.display()))?;
    let mut z = zip::ZipArchive::new(f).map_err(|_| "não parece um .docx (o arquivo não é um zip)".to_string())?;
    let mut v = Vec::new();
    for i in 0..z.len() {
        let mut e = z.by_index(i).map_err(|e| e.to_string())?;
        if !parte_com_texto(e.name()) {
            continue;
        }
        if e.size() > LIMITE {
            return Err("parte grande demais no .docx".into());
        }
        let mut s = String::new();
        e.read_to_string(&mut s).map_err(|err| format!("{}: {err}", e.name()))?;
        v.push(Parte { nome: e.name().to_string(), xml: s });
    }
    if !v.iter().any(|x| x.nome.eq_ignore_ascii_case("word/document.xml")) {
        return Err("não parece um .docx (sem word/document.xml)".into());
    }
    Ok(v)
}

/// O .docx de saída: as partes trocadas no lugar das originais; o resto copiado como estava, na mesma ordem.
pub fn docx_gravar(origem: &Path, destino: &Path, partes: &[Parte]) -> Result<(), String> {
    let f = std::fs::File::open(origem).map_err(|e| e.to_string())?;
    let mut z = zip::ZipArchive::new(f).map_err(|e| e.to_string())?;
    let tmp = destino.with_file_name(format!(".{}.parcial", destino.file_name().unwrap().to_string_lossy()));
    let r = (|| -> Result<(), String> {
        let mut w = zip::ZipWriter::new(std::fs::File::create(&tmp).map_err(|e| e.to_string())?);
        let op = zip::write::SimpleFileOptions::default().compression_method(zip::CompressionMethod::Deflated);
        for i in 0..z.len() {
            let nome = z.by_index_raw(i).map_err(|e| e.to_string())?.name().to_string();
            if let Some(p) = partes.iter().find(|p| p.nome == nome) {
                w.start_file(nome.as_str(), op).map_err(|e| e.to_string())?;
                w.write_all(p.xml.as_bytes()).map_err(|e| e.to_string())?;
            } else {
                let e = z.by_index_raw(i).map_err(|e| e.to_string())?;
                w.raw_copy_file(e).map_err(|e| e.to_string())?;
            }
        }
        w.finish().map_err(|e| e.to_string())?;
        Ok(())
    })();
    if let Err(e) = r {
        let _ = std::fs::remove_file(&tmp);
        return Err(format!("não consegui gravar o .docx: {e}"));
    }
    std::fs::rename(&tmp, destino).map_err(|e| format!("não consegui gravar {}: {e}", destino.display()))
}

/// Nome de arquivo sem o que o Windows ou o macOS recusam (o título reidentificado pode trazer a "/" de uma data).
pub fn nome_seguro(s: &str) -> String {
    let t: String = s.chars().map(|c| if "\\/:*?\"<>|".contains(c) || c.is_control() { '-' } else { c }).collect();
    let t: String = t.trim().trim_end_matches('.').chars().take(150).collect();
    if t.trim().is_empty() {
        "minuta".into()
    } else {
        t
    }
}

#[cfg(test)]
mod testes {
    use super::*;

    #[test]
    fn texto_em_utf8_e_em_ansi() {
        assert_eq!(decodificar("ação".as_bytes()), "ação");
        assert_eq!(decodificar(&[0xEF, 0xBB, 0xBF, b'o', b'k']), "ok");
        assert_eq!(decodificar(&[b'a', 0xE7, 0xE3, b'o', b' ', 0x93, b'x', 0x94]), "ação “x”");
    }

    #[test]
    fn partes_do_docx() {
        for n in ["word/document.xml", "word/header1.xml", "word/footer12.xml", "word/footnotes.xml", "word/comments.xml"] {
            assert!(parte_com_texto(n), "{n}");
        }
        for n in ["word/styles.xml", "word/_rels/document.xml.rels", "word/headerX.xml", "docProps/core.xml", "word/media/a.xml"] {
            assert!(!parte_com_texto(n), "{n}");
        }
    }

    #[test]
    fn nome_sem_caractere_proibido() {
        assert_eq!(nome_seguro("Minuta FULANO 05/11/2026: ok?"), "Minuta FULANO 05-11-2026- ok-");
        assert_eq!(nome_seguro("  ...  "), "minuta");
    }

    #[test]
    fn docx_ida_e_volta() {
        let dir = std::env::temp_dir().join(format!("skia-reid-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let orig = dir.join("minuta.docx");
        {
            let mut w = zip::ZipWriter::new(std::fs::File::create(&orig).unwrap());
            let op = zip::write::SimpleFileOptions::default().compression_method(zip::CompressionMethod::Deflated);
            w.start_file("[Content_Types].xml", op).unwrap();
            w.write_all(b"<Types/>").unwrap();
            w.start_file("word/document.xml", op).unwrap();
            w.write_all("<w:document><w:t>«Tha</w:t><w:t>nasis»</w:t></w:document>".as_bytes()).unwrap();
            w.start_file("word/media/imagem.png", zip::write::SimpleFileOptions::default()).unwrap();
            w.write_all(&[1, 2, 3, 4]).unwrap();
            w.finish().unwrap();
        }
        let mut partes = docx_partes(&orig).unwrap();
        assert_eq!(partes.len(), 1);
        partes[0].xml = partes[0].xml.replace("«Tha", "FULANO").replace("nasis»", "");
        let saida = dir.join("minuta (reidentificado).docx");
        docx_gravar(&orig, &saida, &partes).unwrap();
        let novas = docx_partes(&saida).unwrap();
        assert!(novas[0].xml.contains("<w:t>FULANO</w:t><w:t></w:t>"));
        let mut z = zip::ZipArchive::new(std::fs::File::open(&saida).unwrap()).unwrap();
        let nomes: Vec<String> = (0..z.len()).map(|i| z.by_index(i).unwrap().name().to_string()).collect();
        assert_eq!(nomes, ["[Content_Types].xml", "word/document.xml", "word/media/imagem.png"]);
        let mut img = Vec::new();
        z.by_name("word/media/imagem.png").unwrap().read_to_end(&mut img).unwrap();
        assert_eq!(img, [1, 2, 3, 4]);
        assert!(docx_partes(&orig).unwrap()[0].xml.contains("«Tha"), "a original não muda");
        let _ = std::fs::remove_dir_all(&dir);
    }
}
