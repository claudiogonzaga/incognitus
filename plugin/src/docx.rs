//! A minuta em .docx (17/11/2026, pedido do usuário: abrir a minuta num editor de texto, "num formato docx ou algo
//! equivalente"). O editor da janela manda os parágrafos e daqui sai um Word mínimo, que o Word, o LibreOffice, o Pages
//! e o Google Docs abrem: a folha A4 com as margens de 3 e 2 cm, Times New Roman 12, entrelinha de 1,5.
//!
//! Sem numeração automática do Word: o marcador ("•") e o número ("1.") das listas vão escritos, com o recuo pendente.
//! Fica mais simples e abre igual em todo editor.

use serde::Deserialize;
use std::io::Write;

/// Um pedaço do parágrafo com a mesma formatação.
#[derive(Deserialize, Default, Debug)]
pub struct Trecho {
    #[serde(default)]
    pub t: String,
    #[serde(default)]
    pub b: bool,
    #[serde(default)]
    pub i: bool,
    #[serde(default)]
    pub u: bool,
    /// marca-texto (o ==destaque== da IA: o que depende de conferência)
    #[serde(default)]
    pub m: bool,
}

#[derive(Deserialize, Default, Debug)]
pub struct Paragrafo {
    /// "p", "h1", "h2", "h3", "li" (marcador), "ol" (numerada) ou "quote" (citação recuada)
    #[serde(default)]
    pub estilo: String,
    /// "left", "center", "right" ou "justify"
    #[serde(default)]
    pub alinhamento: String,
    /// níveis de recuo à esquerda (o indent do editor)
    #[serde(default)]
    pub recuo: u32,
    #[serde(default)]
    pub trechos: Vec<Trecho>,
}

const TIPOS: &str = r#"<?xml version="1.0" encoding="UTF-8" standalone="yes"?>
<Types xmlns="http://schemas.openxmlformats.org/package/2006/content-types"><Default Extension="rels" ContentType="application/vnd.openxmlformats-package.relationships+xml"/><Default Extension="xml" ContentType="application/xml"/><Override PartName="/word/document.xml" ContentType="application/vnd.openxmlformats-officedocument.wordprocessingml.document.main+xml"/><Override PartName="/word/styles.xml" ContentType="application/vnd.openxmlformats-officedocument.wordprocessingml.styles+xml"/></Types>"#;
const RELS: &str = r#"<?xml version="1.0" encoding="UTF-8" standalone="yes"?>
<Relationships xmlns="http://schemas.openxmlformats.org/package/2006/relationships"><Relationship Id="rId1" Type="http://schemas.openxmlformats.org/officeDocument/2006/relationships/officeDocument" Target="word/document.xml"/></Relationships>"#;
const RELS_DOC: &str = r#"<?xml version="1.0" encoding="UTF-8" standalone="yes"?>
<Relationships xmlns="http://schemas.openxmlformats.org/package/2006/relationships"><Relationship Id="rId1" Type="http://schemas.openxmlformats.org/officeDocument/2006/relationships/styles" Target="styles.xml"/></Relationships>"#;
const ESTILOS: &str = r#"<?xml version="1.0" encoding="UTF-8" standalone="yes"?>
<w:styles xmlns:w="http://schemas.openxmlformats.org/wordprocessingml/2006/main"><w:docDefaults><w:rPrDefault><w:rPr><w:rFonts w:ascii="Times New Roman" w:hAnsi="Times New Roman" w:cs="Times New Roman" w:eastAsia="Times New Roman"/><w:sz w:val="24"/><w:szCs w:val="24"/><w:lang w:val="pt-BR"/></w:rPr></w:rPrDefault><w:pPrDefault><w:pPr><w:spacing w:after="120" w:line="360" w:lineRule="auto"/></w:pPr></w:pPrDefault></w:docDefaults><w:style w:type="paragraph" w:default="1" w:styleId="Normal"><w:name w:val="Normal"/><w:qFormat/></w:style></w:styles>"#;

fn escapar(s: &str) -> String {
    // o que o XML 1.0 não aceita (controles fora tab, LF e CR) sai; o resto é escapado
    s.chars()
        .filter(|&c| c == '\t' || c == '\n' || c >= ' ')
        .map(|c| match c {
            '&' => "&amp;".to_string(),
            '<' => "&lt;".to_string(),
            '>' => "&gt;".to_string(),
            c => c.to_string(),
        })
        .collect()
}

fn trecho_xml(t: &Trecho, negrito: bool, tamanho: Option<u32>) -> String {
    if t.t.is_empty() {
        return String::new();
    }
    let mut pr = String::new();
    if t.b || negrito {
        pr.push_str("<w:b/><w:bCs/>");
    }
    if t.i {
        pr.push_str("<w:i/><w:iCs/>");
    }
    if t.u {
        pr.push_str(r#"<w:u w:val="single"/>"#);
    }
    if let Some(sz) = tamanho {
        pr.push_str(&format!(r#"<w:sz w:val="{sz}"/><w:szCs w:val="{sz}"/>"#));
    }
    if t.m {
        pr.push_str(r#"<w:highlight w:val="yellow"/>"#);
    }
    let pr = if pr.is_empty() { String::new() } else { format!("<w:rPr>{pr}</w:rPr>") };
    // a quebra de linha dentro do parágrafo vira <w:br/>; a tabulação, <w:tab/>
    let mut miolo = String::new();
    for (k, linha) in t.t.replace("\r\n", "\n").replace('\r', "\n").split('\n').enumerate() {
        if k > 0 {
            miolo.push_str("<w:br/>");
        }
        for (j, parte) in linha.split('\t').enumerate() {
            if j > 0 {
                miolo.push_str("<w:tab/>");
            }
            if !parte.is_empty() {
                miolo.push_str(&format!(r#"<w:t xml:space="preserve">{}</w:t>"#, escapar(parte)));
            }
        }
    }
    format!("<w:r>{pr}{miolo}</w:r>")
}

/// O word/document.xml dos parágrafos.
pub fn documento_xml(paragrafos: &[Paragrafo]) -> String {
    let mut corpo = String::new();
    let mut numero = 0u32;
    for p in paragrafos {
        let estilo = p.estilo.as_str();
        numero = if estilo == "ol" { numero + 1 } else { 0 };
        let jc = match p.alinhamento.as_str() {
            "center" => "center",
            "right" => "right",
            "justify" => "both",
            _ => "left",
        };
        let recuo = p.recuo.min(8) * 720;
        let (ind, negrito, tamanho, antes) = match estilo {
            "h1" => (format!(r#"<w:ind w:left="{recuo}"/>"#), true, Some(28), 240),
            "h2" => (format!(r#"<w:ind w:left="{recuo}"/>"#), true, Some(26), 200),
            "h3" => (format!(r#"<w:ind w:left="{recuo}"/>"#), true, None, 160),
            "li" | "ol" => (format!(r#"<w:ind w:left="{}" w:hanging="360"/>"#, recuo + 720), false, None, 0),
            // a citação recuada das peças: 4 cm, corpo 11, entrelinha simples
            "quote" => (format!(r#"<w:ind w:left="{}"/>"#, recuo + 2268), false, Some(22), 0),
            _ => (format!(r#"<w:ind w:left="{recuo}"/>"#), false, None, 0),
        };
        let espaco = if estilo == "quote" {
            r#"<w:spacing w:after="120" w:line="240" w:lineRule="auto"/>"#.to_string()
        } else if antes > 0 {
            format!(r#"<w:spacing w:before="{antes}" w:after="120"/>"#)
        } else {
            String::new()
        };
        let titulo = if matches!(estilo, "h1" | "h2" | "h3") { "<w:keepNext/>" } else { "" };
        corpo.push_str(&format!(r#"<w:p><w:pPr>{titulo}{espaco}{ind}<w:jc w:val="{jc}"/></w:pPr>"#));
        match estilo {
            "li" => corpo.push_str(&trecho_xml(&Trecho { t: "•\t".into(), ..Default::default() }, false, None)),
            "ol" => corpo.push_str(&trecho_xml(&Trecho { t: format!("{numero}.\t"), ..Default::default() }, false, None)),
            _ => {}
        }
        for t in &p.trechos {
            corpo.push_str(&trecho_xml(t, negrito, tamanho));
        }
        corpo.push_str("</w:p>");
    }
    format!(
        r#"<?xml version="1.0" encoding="UTF-8" standalone="yes"?>
<w:document xmlns:w="http://schemas.openxmlformats.org/wordprocessingml/2006/main"><w:body>{corpo}<w:sectPr><w:pgSz w:w="11906" w:h="16838"/><w:pgMar w:top="1701" w:right="1134" w:bottom="1134" w:left="1701" w:header="709" w:footer="709" w:gutter="0"/></w:sectPr></w:body></w:document>"#
    )
}

/// O .docx inteiro, em bytes.
pub fn montar(paragrafos: &[Paragrafo]) -> Result<Vec<u8>, String> {
    let mut w = zip::ZipWriter::new(std::io::Cursor::new(Vec::new()));
    let op = zip::write::SimpleFileOptions::default().compression_method(zip::CompressionMethod::Deflated);
    let doc = documento_xml(paragrafos);
    // [Content_Types].xml vem primeiro: é a ordem que o Word grava e que os leitores mais rígidos esperam
    for (nome, texto) in [
        ("[Content_Types].xml", TIPOS),
        ("_rels/.rels", RELS),
        ("word/document.xml", doc.as_str()),
        ("word/_rels/document.xml.rels", RELS_DOC),
        ("word/styles.xml", ESTILOS),
    ] {
        w.start_file(nome, op).map_err(|e| e.to_string())?;
        w.write_all(texto.as_bytes()).map_err(|e| e.to_string())?;
    }
    Ok(w.finish().map_err(|e| e.to_string())?.into_inner())
}

#[cfg(test)]
mod testes {
    use super::*;

    fn p(estilo: &str, alinhamento: &str, trechos: Vec<Trecho>) -> Paragrafo {
        Paragrafo { estilo: estilo.into(), alinhamento: alinhamento.into(), recuo: 0, trechos }
    }
    fn t(texto: &str) -> Trecho {
        Trecho { t: texto.into(), ..Default::default() }
    }

    #[test]
    fn a_minuta_vira_um_docx_que_o_proprio_app_le() {
        let paragrafos = vec![
            p("h1", "center", vec![t("ALEGAÇÕES FINAIS")]),
            p("p", "justify", vec![t("O Ministério Público, por "), Trecho { t: "Zebedeu & Cia <fictício>".into(), b: true, i: true, u: true, m: true }, t(", requer.")]),
            p("li", "", vec![t("primeiro item")]),
            p("ol", "", vec![t("um")]),
            p("ol", "", vec![t("dois")]),
            p("p", "", vec![t("volta ao texto")]),
            p("ol", "", vec![t("recomeça")]),
            p("quote", "justify", vec![t("citação\ncom quebra\te tabulação\u{1}")]),
            Paragrafo::default(),
        ];
        let bytes = montar(&paragrafos).unwrap();
        let pasta = std::env::temp_dir().join(format!("incognitus-docx-{}", std::process::id()));
        std::fs::create_dir_all(&pasta).unwrap();
        let arq = pasta.join("minuta de prova.docx");
        std::fs::write(&arq, &bytes).unwrap();
        // o leitor de .docx do próprio app (o da reidentificação) abre e devolve o texto, parágrafo a parágrafo
        let texto = crate::reid::docx_texto(&arq).unwrap();
        assert!(texto.contains("ALEGAÇÕES FINAIS"), "{texto}");
        assert!(texto.contains("O Ministério Público, por Zebedeu & Cia <fictício>, requer."), "{texto}");
        assert!(texto.contains("primeiro item") && texto.contains("recomeça"), "{texto}");
        let nomes: Vec<String> = {
            let mut z = zip::ZipArchive::new(std::fs::File::open(&arq).unwrap()).unwrap();
            (0..z.len()).map(|i| z.by_index(i).unwrap().name().to_string()).collect()
        };
        assert_eq!(nomes, ["[Content_Types].xml", "_rels/.rels", "word/document.xml", "word/_rels/document.xml.rels", "word/styles.xml"]);
        let _ = std::fs::remove_dir_all(&pasta);

        let xml = documento_xml(&paragrafos);
        assert!(xml.contains("Zebedeu &amp; Cia &lt;fictício&gt;"), "o texto vai escapado");
        assert!(xml.contains("<w:b/><w:bCs/><w:i/><w:iCs/><w:u w:val=\"single\"/><w:highlight w:val=\"yellow\"/>"), "negrito, itálico, sublinhado e marca-texto");
        assert!(xml.contains(r#"<w:jc w:val="center"/>"#) && xml.contains(r#"<w:jc w:val="both"/>"#) && xml.contains(r#"<w:jc w:val="left"/>"#));
        assert!(xml.contains("•") && xml.contains(">1.<") && xml.contains(">2.<"), "o marcador e os números das listas vão escritos");
        assert_eq!(xml.matches(">1.<").count(), 2, "a lista numerada recomeça depois de um parágrafo comum");
        assert!(xml.contains("citação</w:t><w:br/><w:t xml:space=\"preserve\">com quebra</w:t><w:tab/>"), "quebra de linha e tabulação");
        assert!(!xml.contains('\u{1}'), "caractere de controle não entra no XML");
        assert!(xml.contains(r#"<w:ind w:left="2268"/>"#), "a citação recuada em 4 cm");
        assert!(xml.contains(r#"<w:pgSz w:w="11906" w:h="16838"/>"#), "folha A4");
    }
}
