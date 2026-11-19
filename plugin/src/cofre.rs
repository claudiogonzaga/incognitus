//! O cofre do mapa (19/11/2026, pedido do usuário: "criptografar o mapa", para o caso de o computador ser invadido ou
//! de a pasta ser copiada). O "… — MAPA (nomes reais).json" é o dicionário de cada token: com ele, o texto anonimizado
//! volta aos nomes. Aqui ele é gravado cifrado (AES-256-GCM) e só o Incognitus o abre.
//!
//! A chave (32 bytes ao acaso) mora no chaveiro do sistema (o Keychain no Mac, o Gerenciador de Credenciais no Windows),
//! nunca em arquivo. Para abrir os mapas em outro computador, ou depois de formatar este, quem usa copia a "chave de
//! recuperação" nas Configurações e a cola no outro Incognitus. A chave antiga não se perde quando outra é colada: fica
//! guardada, e cada mapa diz com qual foi cifrado.
//!
//! O arquivo continua com o nome e a extensão de sempre (um JSON), agora com um envelope no lugar do dicionário. O mapa
//! em claro (o de antes, ou o do Graphé) continua sendo lido como sempre.

use base64::Engine;
use ring::aead::{Aad, LessSafeKey, Nonce, UnboundKey, AES_256_GCM};
use serde_json::{json, Value};
use sha2::{Digest, Sha256};
use std::sync::Mutex;

pub const CONTA: &str = "cofre-mapas";
/// As chaves de antes (quando outra foi colada por cima), separadas por vírgula: os mapas cifrados com elas ainda abrem.
pub const CONTA_ANTIGAS: &str = "cofre-mapas-antigas";
const MARCA: &str = "incognitus_cofre";
const DADO_AUTENTICADO: &[u8] = b"incognitus-cofre-1";
const B64: base64::engine::GeneralPurpose = base64::engine::general_purpose::STANDARD;
pub const AVISO: &str = "Mapa do Incognitus, cifrado (AES-256-GCM). Abre no Incognitus deste computador, ou em outro com a chave de recuperação (Configurações, Mapa cifrado).";

pub type Chave = [u8; 32];

/// As chaves já lidas do chaveiro nesta execução: a atual e as antigas (o chaveiro pode pedir a senha a cada leitura).
static LIDAS: Mutex<Option<(Chave, Vec<Chave>)>> = Mutex::new(None);

/// O apelido da chave (os 4 primeiros bytes do SHA-256 dela, em hexadecimal): diz qual chave abre, sem dizer a chave.
pub fn apelido(chave: &Chave) -> String {
    Sha256::digest(chave).iter().take(4).map(|b| format!("{b:02x}")).collect()
}

fn hex(b: &[u8]) -> String {
    b.iter().map(|x| format!("{x:02x}")).collect()
}

/// A chave em 16 grupos de 4 letras e números (para copiar e guardar).
pub fn em_grupos(chave: &Chave) -> String {
    hex(chave).as_bytes().chunks(4).map(|c| String::from_utf8_lossy(c).to_uppercase()).collect::<Vec<_>>().join("-")
}

/// A chave colada: 64 dígitos hexadecimais, com ou sem traços e espaços.
pub fn de_grupos(texto: &str) -> Result<Chave, String> {
    let limpo: String = texto.chars().filter(|c| c.is_ascii_hexdigit()).collect();
    let sujeira = texto.chars().any(|c| !(c.is_ascii_hexdigit() || c == '-' || c.is_whitespace()));
    if limpo.len() != 64 || sujeira {
        return Err("Isso não parece a chave de recuperação: são 16 grupos de 4 letras e números, separados por traço.".into());
    }
    let mut c = [0u8; 32];
    for (i, par) in limpo.as_bytes().chunks(2).enumerate() {
        c[i] = u8::from_str_radix(&String::from_utf8_lossy(par), 16).map_err(|_| "chave inválida".to_string())?;
    }
    Ok(c)
}

fn cifra(chave: &Chave) -> Result<LessSafeKey, String> {
    Ok(LessSafeKey::new(UnboundKey::new(&AES_256_GCM, chave).map_err(|_| "chave do cofre inválida".to_string())?))
}

/// O texto é um mapa cifrado?
pub fn eh_cofre(texto: &str) -> bool {
    let t = texto.trim_start_matches('\u{feff}').trim_start();
    t.starts_with('{') && t.chars().take(80).collect::<String>().contains(MARCA)
}

/// O envelope: um JSON com o apelido da chave, o número de uso único e o mapa cifrado (com a etiqueta de autenticação).
pub fn selar(chave: &Chave, claro: &[u8]) -> Result<String, String> {
    let mut n = [0u8; 12];
    getrandom::getrandom(&mut n).map_err(|e| format!("o sistema não deu números ao acaso: {e}"))?;
    let mut dados = claro.to_vec();
    cifra(chave)?
        .seal_in_place_append_tag(Nonce::assume_unique_for_key(n), Aad::from(DADO_AUTENTICADO), &mut dados)
        .map_err(|_| "não consegui cifrar o mapa".to_string())?;
    // escrito à mão para a marca vir primeiro (o serde_json põe as chaves em ordem alfabética)
    Ok(format!(
        "{{\n \"{MARCA}\": 1,\n \"aviso\": {},\n \"chave\": \"{}\",\n \"nonce\": \"{}\",\n \"dados\": \"{}\"\n}}\n",
        json!(AVISO),
        apelido(chave),
        B64.encode(n),
        B64.encode(&dados)
    ))
}

/// O apelido da chave que cifrou o envelope.
pub fn apelido_do(envelope: &str) -> Option<String> {
    let v: Value = serde_json::from_str(envelope.trim_start_matches('\u{feff}')).ok()?;
    v.get("chave").and_then(Value::as_str).map(str::to_string)
}

/// O mapa em claro, com a chave dada. Erro se a chave não é a do envelope ou se o arquivo foi mexido.
pub fn abrir(chave: &Chave, envelope: &str) -> Result<Vec<u8>, String> {
    let v: Value = serde_json::from_str(envelope.trim_start_matches('\u{feff}')).map_err(|_| "o mapa cifrado está corrompido".to_string())?;
    let campo = |k: &str| v.get(k).and_then(Value::as_str).and_then(|s| B64.decode(s).ok());
    let (n, mut dados) = (campo("nonce").ok_or("o mapa cifrado está corrompido")?, campo("dados").ok_or("o mapa cifrado está corrompido")?);
    let nonce = Nonce::try_assume_unique_for_key(&n).map_err(|_| "o mapa cifrado está corrompido".to_string())?;
    let claro = cifra(chave)?
        .open_in_place(nonce, Aad::from(DADO_AUTENTICADO), &mut dados)
        .map_err(|_| "a chave não abre este mapa (ou o arquivo foi alterado)".to_string())?;
    Ok(claro.to_vec())
}

/// A chave do teste de ponta a ponta (SKIA_E2E): fixa, sem tocar no chaveiro.
fn de_prova() -> Option<Chave> {
    std::env::var_os("SKIA_E2E")?;
    Some(Sha256::digest(b"incognitus-cofre-de-prova").into())
}

fn ler_do_chaveiro(servico: &str) -> Result<Option<(Chave, Vec<Chave>)>, String> {
    let Some(atual) = crate::chaveiro::ler(servico, CONTA)? else { return Ok(None) };
    let atual = de_grupos(&atual).map_err(|_| "a chave do cofre guardada no chaveiro está estragada".to_string())?;
    let antigas = crate::chaveiro::ler(servico, CONTA_ANTIGAS)?.unwrap_or_default();
    Ok(Some((atual, antigas.split(',').filter_map(|c| de_grupos(c).ok()).collect())))
}

/// A chave atual e as antigas. Com `criar`, a primeira chamada neste computador sorteia a chave e a guarda no chaveiro.
pub fn chaves(servico: &str, criar: bool) -> Result<Option<(Chave, Vec<Chave>)>, String> {
    if let Some(c) = de_prova() {
        return Ok(Some((c, vec![])));
    }
    let mut lidas = LIDAS.lock().unwrap();
    if lidas.is_none() {
        *lidas = ler_do_chaveiro(servico)?;
    }
    if lidas.is_none() && criar {
        let mut c = [0u8; 32];
        getrandom::getrandom(&mut c).map_err(|e| format!("o sistema não deu números ao acaso: {e}"))?;
        crate::chaveiro::guardar(servico, CONTA, &em_grupos(&c))?;
        // só vale se o chaveiro devolve a mesma chave: mapa cifrado com chave que não ficou guardada estaria perdido
        match ler_do_chaveiro(servico)? {
            Some((lida, _)) if lida == c => *lidas = Some((c, vec![])),
            _ => return Err("o chaveiro do sistema não guardou a chave do cofre".into()),
        }
    }
    Ok(lidas.clone())
}

/// O mapa cifrado com a chave deste computador (que é criada no primeiro uso).
pub fn selar_aqui(servico: &str, claro: &[u8]) -> Result<String, String> {
    let (atual, _) = chaves(servico, true)?.ok_or("sem a chave do cofre")?;
    selar(&atual, claro)
}

/// O mapa em claro: tenta a chave que o envelope aponta (a atual ou uma das antigas).
pub fn abrir_aqui(servico: &str, envelope: &str) -> Result<String, String> {
    let sem_chave = || {
        "Este mapa está cifrado com a chave de outro computador (ou de antes de uma formatação). Cole a chave de recuperação \
dele nas Configurações (Mapa cifrado) para abri-lo."
            .to_string()
    };
    let (atual, antigas) = chaves(servico, false)?.ok_or_else(sem_chave)?;
    let alvo = apelido_do(envelope);
    let mut todas = vec![atual];
    todas.extend(antigas);
    todas.sort_by_key(|c| Some(apelido(c)) != alvo);
    for c in &todas {
        if let Ok(b) = abrir(c, envelope) {
            return String::from_utf8(b).map_err(|_| "o mapa cifrado está corrompido".to_string());
        }
    }
    Err(sem_chave())
}

/// Troca a chave deste computador pela colada (a de outro computador). A de antes vai para as antigas.
pub fn importar(servico: &str, texto: &str) -> Result<String, String> {
    if de_prova().is_some() {
        return Err("no teste de ponta a ponta a chave é fixa".into());
    }
    let nova = de_grupos(texto)?;
    let antes = chaves(servico, false)?;
    if let Some((atual, mut antigas)) = antes {
        if atual == nova {
            return Ok(apelido(&nova));
        }
        antigas.retain(|c| *c != nova && *c != atual);
        antigas.insert(0, atual);
        crate::chaveiro::guardar(servico, CONTA_ANTIGAS, &antigas.iter().map(em_grupos).collect::<Vec<_>>().join(","))?;
    }
    crate::chaveiro::guardar(servico, CONTA, &em_grupos(&nova))?;
    *LIDAS.lock().unwrap() = None;
    chaves(servico, false)?;
    Ok(apelido(&nova))
}

#[cfg(test)]
mod testes {
    use super::*;

    const A: Chave = [7u8; 32];
    const B: Chave = [9u8; 32];
    const MAPA: &str = "{\n \"tokens\": {\n  \"«REU_01»\": { \"valor\": \"Fulano Fictício de Tal\", \"tipo\": \"REU\" }\n }\n}";

    #[test]
    fn cifra_e_abre_sem_deixar_o_nome_no_arquivo() {
        let e = selar(&A, MAPA.as_bytes()).unwrap();
        assert!(eh_cofre(&e) && !eh_cofre(MAPA) && !eh_cofre("texto qualquer"));
        assert!(!e.contains("Fulano") && !e.contains("REU_01") && !e.contains("tokens"));
        assert_eq!(apelido_do(&e).unwrap(), apelido(&A));
        assert_eq!(String::from_utf8(abrir(&A, &e).unwrap()).unwrap(), MAPA);
        // cada gravação sai diferente (o número de uso único é sorteado)
        assert_ne!(e, selar(&A, MAPA.as_bytes()).unwrap());
    }

    #[test]
    fn a_chave_errada_e_o_arquivo_mexido_nao_abrem() {
        let e = selar(&A, MAPA.as_bytes()).unwrap();
        assert!(abrir(&B, &e).unwrap_err().contains("não abre"));
        let mut v: Value = serde_json::from_str(&e).unwrap();
        let mut d = B64.decode(v["dados"].as_str().unwrap()).unwrap();
        d[3] ^= 1;
        v["dados"] = json!(B64.encode(&d));
        assert!(abrir(&A, &v.to_string()).is_err());
        assert!(abrir(&A, "{\"incognitus_cofre\":1}").is_err());
    }

    #[test]
    fn a_chave_de_recuperacao_vai_e_volta() {
        let g = em_grupos(&A);
        assert_eq!(g.len(), 16 * 4 + 15);
        assert!(g.starts_with("0707-0707-"));
        assert_eq!(de_grupos(&g).unwrap(), A);
        assert_eq!(de_grupos(&format!("  {}\n", g.to_lowercase().replace('-', " "))).unwrap(), A);
        assert!(de_grupos("0707-0707").is_err() && de_grupos(&g.replace('0', "z")).is_err());
        assert_eq!(apelido(&A).len(), 8);
    }
}
