//! O PDF de saída: a primeira página é o cabeçalho de autos anonimizados do motor e depois vem UMA página para cada
//! página dos autos, em fonte monoespaçada de 11 pt (DejaVu Sans Mono, embutida só com os glifos usados), com camada de
//! texto (ToUnicode): dá para copiar, buscar e mandar a uma IA. A fonte nunca encolhe: a folha é que cresce quando a
//! página dos autos tem linhas longas ou muitas linhas (A4 é o mínimo).
use flate2::{write::ZlibEncoder, Compression};
use pdf_writer::types::{CidFontType, FontFlags, SystemInfo};
use pdf_writer::{Content, Date, Filter, Finish, Name, Pdf, Rect, Ref, Str, TextStr};
use std::collections::BTreeMap;
use std::io::Write;

const FONTE: &[u8] = include_bytes!("../recursos/fontes/DejaVuSansMono.ttf");
const TAMANHO: f32 = 11.0;
const ENTRELINHA: f32 = 13.2;
const MARGEM: f32 = 36.0;
const A4: (f32, f32) = (595.28, 841.89);
const COLUNAS_CABECALHO: usize = 78; // cabe no A4 em pé
const COLUNAS_MAX: usize = 160; // linha maior quebra (senão a folha passaria de 40 cm)

pub struct Resultado {
    pub bytes: Vec<u8>,
    pub sem_glifo: usize,
}

fn comprimir(dados: &[u8]) -> Vec<u8> {
    let mut e = ZlibEncoder::new(Vec::new(), Compression::new(6));
    e.write_all(dados).expect("zlib");
    e.finish().expect("zlib")
}

/// Quebra uma linha em pedaços de no máximo `n` caracteres (na palavra, se der).
fn quebrar(linha: &str, n: usize, na_palavra: bool) -> Vec<String> {
    let cs: Vec<char> = linha.chars().collect();
    if cs.len() <= n {
        return vec![linha.to_string()];
    }
    let mut out = Vec::new();
    let mut ini = 0;
    while cs.len() - ini > n {
        let mut fim = ini + n;
        if na_palavra {
            if let Some(p) = (ini + n / 2..ini + n).rev().find(|&k| cs[k] == ' ') {
                fim = p + 1;
            }
        }
        out.push(cs[ini..fim].iter().collect::<String>().trim_end().to_string());
        ini = fim;
    }
    out.push(cs[ini..].iter().collect());
    out
}

/// Tabulação vira espaço (de 8 em 8), caractere de controle vira espaço, e o fim da linha perde os espaços.
fn limpar(linha: &str) -> String {
    let mut s = String::new();
    for c in linha.chars() {
        if c == '\t' {
            let falta = 8 - (s.chars().count() % 8);
            s.extend(std::iter::repeat(' ').take(falta));
        } else if c.is_control() {
            s.push(' ');
        } else {
            s.push(c);
        }
    }
    s.trim_end().to_string()
}

/// As linhas de uma página dos autos: a marca "===== PÁGINA n =====" fica; o resto perde o recuo comum (a margem que
/// o pdftotext -layout põe) e quebra só acima de COLUNAS_MAX.
fn linhas_da_pagina(texto: &str) -> Vec<String> {
    let mut ls: Vec<String> = texto.split('\n').map(limpar).collect();
    let ini = if ls.first().map_or(false, |l| l.starts_with("===== P")) { 1 } else { 0 };
    let recuo = ls[ini..]
        .iter()
        .filter(|l| !l.is_empty())
        .map(|l| l.chars().take_while(|c| *c == ' ').count())
        .min()
        .unwrap_or(0);
    if recuo > 0 {
        for l in ls[ini..].iter_mut() {
            *l = l.chars().skip(recuo).collect();
        }
    }
    ls.into_iter().flat_map(|l| quebrar(&l, COLUNAS_MAX, false)).collect()
}

fn linhas_do_cabecalho(texto: &str) -> Vec<String> {
    texto.split('\n').map(limpar).flat_map(|l| quebrar(&l, COLUNAS_CABECALHO, true)).collect()
}

/// Gera o PDF. `paginas` já vem no formato do corpo do .txt ("===== PÁGINA n =====" + o texto).
pub fn gerar(cabecalho: &str, paginas: &[String], produtor: &str) -> Result<Resultado, String> {
    let face = ttf_parser::Face::parse(FONTE, 0).map_err(|e| format!("fonte: {e:?}"))?;
    let upem = face.units_per_em() as f32;
    let por_mil = |v: f32| v * 1000.0 / upem;
    let gid_m = face.glyph_index('M').ok_or("fonte sem M")?;
    let avanco = face.glyph_hor_advance(gid_m).ok_or("fonte sem avanço")? as f32;
    let largura_char = avanco * TAMANHO / upem;
    let ascendente = face.ascender() as f32 * TAMANHO / upem;
    let gid_interrogacao = face.glyph_index('?').ok_or("fonte sem ?")?;

    let mut folhas: Vec<Vec<String>> = vec![linhas_do_cabecalho(cabecalho)];
    folhas.extend(paginas.iter().map(|p| linhas_da_pagina(p)));

    // Glifos usados: o subconjunto da fonte e o ToUnicode.
    let mut remap = subsetter::GlyphRemapper::new();
    let mut por_char: BTreeMap<char, u16> = BTreeMap::new();
    let mut sem_glifo = 0usize;
    let mut glifo = |c: char, remap: &mut subsetter::GlyphRemapper| -> u16 {
        if let Some(g) = por_char.get(&c) {
            return *g;
        }
        let g = match face.glyph_index(c) {
            Some(g) => remap.remap(g.0),
            None => {
                sem_glifo += 1;
                remap.remap(gid_interrogacao.0)
            }
        };
        por_char.insert(c, g);
        g
    };

    let mut pdf = Pdf::new();
    let mut proximo = 1;
    let mut novo = || {
        let r = Ref::new(proximo);
        proximo += 1;
        r
    };
    let catalogo = novo();
    let arvore = novo();
    let info = novo();
    let fonte0 = novo();
    let fonte_cid = novo();
    let descritor = novo();
    let arquivo_fonte = novo();
    let to_unicode = novo();
    let mut ids_paginas = Vec::new();

    for linhas in &folhas {
        let colunas = linhas.iter().map(|l| l.chars().count()).max().unwrap_or(0) as f32;
        let w = A4.0.max(2.0 * MARGEM + colunas * largura_char);
        let h = A4.1.max(2.0 * MARGEM + linhas.len() as f32 * ENTRELINHA);
        let mut c = Content::new();
        c.begin_text();
        c.set_font(Name(b"F1"), TAMANHO);
        c.set_leading(ENTRELINHA);
        c.next_line(MARGEM, h - MARGEM - ascendente);
        for (k, l) in linhas.iter().enumerate() {
            if k > 0 {
                c.next_line_using_leading();
            }
            if l.is_empty() {
                continue;
            }
            let mut bytes = Vec::with_capacity(l.len() * 2);
            for ch in l.chars() {
                let g = glifo(ch, &mut remap);
                bytes.extend_from_slice(&g.to_be_bytes());
            }
            c.show(Str(&bytes));
        }
        c.end_text();
        let dados = comprimir(&c.finish());
        let id_pagina = novo();
        let id_conteudo = novo();
        let mut p = pdf.page(id_pagina);
        p.media_box(Rect::new(0.0, 0.0, w, h));
        p.parent(arvore);
        p.contents(id_conteudo);
        p.resources().fonts().pair(Name(b"F1"), fonte0);
        p.finish();
        pdf.stream(id_conteudo, &dados).filter(Filter::FlateDecode);
        ids_paginas.push(id_pagina);
    }

    pdf.catalog(catalogo).pages(arvore);
    pdf.pages(arvore).kids(ids_paginas.iter().copied()).count(ids_paginas.len() as i32);

    // A fonte: só os glifos usados; os CIDs são os glifos novos (CIDToGIDMap Identity).
    let sub = subsetter::subset(FONTE, 0, &remap).map_err(|e| format!("subconjunto da fonte: {e:?}"))?;
    let marca: String = {
        let mut h: u32 = 2166136261;
        for g in remap.remapped_gids() {
            h = (h ^ g as u32).wrapping_mul(16777619);
        }
        (0..6).map(|i| (b'A' + ((h >> (i * 5)) % 26) as u8) as char).collect()
    };
    let nome_base = format!("{marca}+DejaVuSansMono");
    let sistema = SystemInfo { registry: Str(b"Adobe"), ordering: Str(b"Identity"), supplement: 0 };
    pdf.type0_font(fonte0)
        .base_font(Name(nome_base.as_bytes()))
        .encoding_predefined(Name(b"Identity-H"))
        .descendant_font(fonte_cid)
        .to_unicode(to_unicode);
    let mut cid = pdf.cid_font(fonte_cid);
    cid.subtype(CidFontType::Type2);
    cid.base_font(Name(nome_base.as_bytes()));
    cid.system_info(sistema);
    cid.font_descriptor(descritor);
    // inteiro: o poppler só aceita /DW inteiro (com casas decimais, ignora e usa 1000 — as letras se espalham)
    cid.default_width(por_mil(avanco).round());
    cid.cid_to_gid_map_predefined(Name(b"Identity"));
    cid.finish();
    let bb = face.global_bounding_box();
    pdf.font_descriptor(descritor)
        .name(Name(nome_base.as_bytes()))
        .flags(FontFlags::FIXED_PITCH | FontFlags::SYMBOLIC)
        .bbox(Rect::new(por_mil(bb.x_min as f32), por_mil(bb.y_min as f32), por_mil(bb.x_max as f32), por_mil(bb.y_max as f32)))
        .italic_angle(0.0)
        .ascent(por_mil(face.ascender() as f32))
        .descent(por_mil(face.descender() as f32))
        .cap_height(por_mil(face.capital_height().unwrap_or(face.ascender()) as f32))
        .stem_v(80.0)
        .font_file2(arquivo_fonte);
    let sub_z = comprimir(&sub);
    pdf.stream(arquivo_fonte, &sub_z).filter(Filter::FlateDecode);
    let mut cmap = pdf_writer::types::UnicodeCmap::new(Name(b"Skia-UTF16"), sistema);
    for (ch, g) in &por_char {
        if face.glyph_index(*ch).is_some() || *ch == '?' {
            cmap.pair(*g, *ch);
        }
    }
    let cmap_z = comprimir(&cmap.finish());
    pdf.cmap(to_unicode, &cmap_z).filter(Filter::FlateDecode);

    let agora = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map(|d| d.as_secs()).unwrap_or(0);
    let (a, m, d, hh, mm, ss) = data_utc(agora);
    pdf.document_info(info)
        .title(TextStr("Autos anonimizados"))
        .producer(TextStr(produtor))
        .creation_date(Date::new(a).month(m).day(d).hour(hh).minute(mm).second(ss).utc_offset_hour(0));
    Ok(Resultado { bytes: pdf.finish(), sem_glifo })
}

/// Segundos desde 1970 → (ano, mês, dia, hora, minuto, segundo) em UTC (algoritmo de dias civis).
fn data_utc(s: u64) -> (u16, u8, u8, u8, u8, u8) {
    let dias = (s / 86400) as i64;
    let resto = s % 86400;
    let z = dias + 719468;
    let era = z.div_euclid(146097);
    let doe = z - era * 146097;
    let yoe = (doe - doe / 1460 + doe / 36524 - doe / 146096) / 365;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = (doy - (153 * mp + 2) / 5 + 1) as u8;
    let m = if mp < 10 { mp + 3 } else { mp - 9 } as u8;
    let a = (yoe + era * 400 + if m <= 2 { 1 } else { 0 }) as u16;
    (a, m, d, (resto / 3600) as u8, ((resto % 3600) / 60) as u8, (resto % 60) as u8)
}

#[cfg(test)]
mod testes {
    use super::*;

    #[test]
    fn pagina_tem_uma_folha_por_pagina() {
        let p = vec!["===== PÁGINA 1 =====\n     «REU_01» foi ouvido.".to_string(), "===== PÁGINA 2 =====\n".to_string()];
        let r = gerar("═══ AUTOS ANONIMIZADOS ═══", &p, "Skiá teste").unwrap();
        let s = String::from_utf8_lossy(&r.bytes);
        assert_eq!(s.matches("/Type /Page").count() - s.matches("/Type /Pages").count(), 3);
        assert_eq!(r.sem_glifo, 0);
    }

    #[test]
    fn recuo_comum_sai_e_marca_fica() {
        let ls = linhas_da_pagina("===== PÁGINA 3 =====\n      a\n        b");
        assert_eq!(ls, vec!["===== PÁGINA 3 =====", "a", "  b"]);
    }

    #[test]
    fn data() {
        assert_eq!(data_utc(0), (1970, 1, 1, 0, 0, 0));
        assert_eq!(data_utc(1_793_664_000), (2026, 11, 3, 0, 0, 0));
    }
}
