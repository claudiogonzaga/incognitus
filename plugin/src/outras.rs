//! As outras IAs do 🧠 (17/11/2026, pedido do usuário: "no botão de gerar a minuta deve ser possível configurar qual
//! IA, qual modelo da IA e o nível de esforço"): a OpenAI, o Google Gemini e a DeepInfra, como no cérebro do Graphé
//! (IA_PROVEDORES). A Anthropic segue em claude.rs.
//!
//! A janela monta a conversa no formato da Anthropic, passa-a pelo porteiro e só então a traduz para o formato do
//! provedor (modulo/ias.js); aqui o pedido ganha a chave do chaveiro e o streaming, e a resposta, que chega aos pedaços,
//! é remontada na resposta inteira do provedor (a mesma da chamada sem streaming). A janela a traduz de volta.
//! Novas tentativas como as da Anthropic: 429 e 5xx até 3 vezes; a conexão que cai no meio é refeita uma vez.
use serde_json::{json, Map, Value};
use std::time::{Duration, Instant};

use crate::claude::Resposta;

#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Provedor {
    OpenAi,
    Gemini,
    DeepInfra,
}

impl Provedor {
    pub fn de(nome: &str) -> Option<Self> {
        match nome {
            "openai" => Some(Self::OpenAi),
            "gemini" => Some(Self::Gemini),
            "deepinfra" => Some(Self::DeepInfra),
            _ => None,
        }
    }
    /// A conta da chave no chaveiro.
    pub fn conta(self) -> &'static str {
        match self {
            Self::OpenAi => "openai",
            Self::Gemini => "gemini",
            Self::DeepInfra => "deepinfra",
        }
    }
    pub fn rotulo(self) -> &'static str {
        match self {
            Self::OpenAi => "OpenAI",
            Self::Gemini => "Google Gemini",
            Self::DeepInfra => "DeepInfra",
        }
    }
    fn base(self) -> &'static str {
        match self {
            Self::OpenAi => "https://api.openai.com/v1",
            Self::Gemini => "https://generativelanguage.googleapis.com/v1beta",
            Self::DeepInfra => "https://api.deepinfra.com/v1/openai",
        }
    }
    fn onde_chave(self) -> &'static str {
        match self {
            Self::OpenAi => "platform.openai.com/api-keys",
            Self::Gemini => "aistudio.google.com/apikey",
            Self::DeepInfra => "deepinfra.com/dash/keys",
        }
    }
}

/// A chave colada, limpa e conferida (sem espaço; OpenAI começa por "sk-", Gemini por "AIza").
pub fn conferir_chave(p: Provedor, texto: &str) -> Result<String, String> {
    let chave = texto.trim();
    let errada = chave.len() < 20
        || chave.chars().any(char::is_whitespace)
        || (p == Provedor::OpenAi && !chave.starts_with("sk-"))
        || (p == Provedor::Gemini && !chave.starts_with("AIza"));
    if errada {
        let forma = match p {
            Provedor::OpenAi => "ela começa com sk- e não tem espaços",
            Provedor::Gemini => "ela começa com AIza e não tem espaços",
            Provedor::DeepInfra => "ela não tem espaços",
        };
        return Err(format!("Isso não parece uma chave da {}: {forma}. Copie de novo em {}.", p.rotulo(), p.onde_chave()));
    }
    Ok(chave.to_string())
}

/// O nome do modelo entra no endereço do Gemini: só letras, números, ponto, hífen, sublinhado e barra (DeepInfra).
fn modelo_valido(m: &str) -> bool {
    !m.is_empty() && m.len() <= 120 && m.chars().all(|c| c.is_ascii_alphanumeric() || "._-/:".contains(c)) && !m.contains("..")
}

fn corta(s: &str, n: usize) -> String {
    s.chars().take(n).collect()
}

enum Falha {
    Queda(String),
    Outra(String),
}

/// Manda o pedido (já no formato do provedor, com "model") e devolve a resposta inteira, remontada do streaming.
/// `url_prova`: só no teste de ponta a ponta, o endereço de um provedor de mentira.
pub async fn enviar(cli: &reqwest::Client, p: Provedor, chave: &str, corpo: &str, url_prova: Option<&str>) -> Result<Resposta, String> {
    let mut pedido: Value = serde_json::from_str(corpo).map_err(|e| format!("O pedido montado pela janela não é JSON: {e}"))?;
    let obj = pedido.as_object_mut().ok_or("O pedido montado pela janela não é um objeto JSON.")?;
    let modelo = obj.get("model").and_then(Value::as_str).unwrap_or("").to_string();
    if !modelo_valido(&modelo) {
        return Err(format!("Nome de modelo inválido: \"{}\".", corta(&modelo, 60)));
    }
    let base = url_prova.map(str::to_string).unwrap_or_else(|| p.base().to_string());
    let url = match p {
        Provedor::Gemini => {
            obj.remove("model");
            format!("{base}/models/{modelo}:streamGenerateContent?alt=sse")
        }
        _ => {
            obj.insert("stream".into(), Value::Bool(true));
            obj.insert("stream_options".into(), json!({"include_usage": true}));
            format!("{base}/chat/completions")
        }
    };
    let corpo = pedido.to_string();
    let t0 = Instant::now();
    let (tentativas, espera) = if url_prova.is_some() { (3u32, Duration::from_millis(20)) } else { (3u32, Duration::from_millis(1500)) };
    let mut tentativa = 0u32;
    let mut quedas = 0u32;
    let mut ultimo = String::new();
    while tentativa < tentativas {
        if tentativa > 0 {
            tokio::time::sleep(espera * tentativa).await;
        }
        let mut req = cli.post(&url).header("content-type", "application/json").header("accept", "text/event-stream");
        req = match p {
            Provedor::Gemini => req.header("x-goog-api-key", chave),
            _ => req.header("authorization", format!("Bearer {chave}")),
        };
        let resp = match req.body(corpo.clone()).send().await {
            Ok(r) => r,
            Err(e) => return Err(format!("A {} não respondeu ({e}). Tente de novo; se repetir, confira a internet.", p.rotulo())),
        };
        let code = resp.status().as_u16();
        if code == 200 {
            match ler(resp, p, Duration::from_secs(180)).await {
                Ok(json) => return Ok(Resposta { json, ms: t0.elapsed().as_millis() as u64, code, tentativas: tentativa + 1, quedas }),
                Err(Falha::Queda(_)) if quedas < 1 => {
                    quedas += 1;
                    tokio::time::sleep(espera).await;
                    continue;
                }
                Err(Falha::Queda(e)) | Err(Falha::Outra(e)) => return Err(e),
            }
        }
        let texto = resp.text().await.unwrap_or_default();
        let baixo = texto.to_lowercase();
        if code == 401 || code == 403 || baixo.contains("api key not valid") || baixo.contains("invalid_api_key") {
            return Err(format!("A chave da {} foi recusada (código {code}). Guarde a chave certa nas Configurações.", p.rotulo()));
        }
        if code == 404 || (code == 400 && baixo.contains("model") && (baixo.contains("not found") || baixo.contains("does not exist"))) {
            return Err(format!("Modelo \"{modelo}\" não reconhecido pela {}: {}", p.rotulo(), corta(&texto, 200)));
        }
        if code == 402 || baixo.contains("insufficient_quota") || baixo.contains("billing") {
            return Err(format!("A conta da {} está sem crédito ou sem cota: {}", p.rotulo(), corta(&texto, 200)));
        }
        if code == 429 || code >= 500 {
            ultimo = format!("código {code}: {}", corta(&texto, 200));
            tentativa += 1;
            continue;
        }
        return Err(format!("A {} retornou {code}: {}", p.rotulo(), corta(&texto, 300)));
    }
    Err(format!("A {} ficou indisponível depois de {tentativas} tentativas. Último erro: {ultimo}", p.rotulo()))
}

async fn ler(mut resp: reqwest::Response, p: Provedor, sem_sinal: Duration) -> Result<Value, Falha> {
    let mut m = Montagem::novo(p);
    let mut buf: Vec<u8> = Vec::new();
    let mut dados = String::new();
    loop {
        let pedaco = match tokio::time::timeout(sem_sinal, resp.chunk()).await {
            Err(_) => return Err(Falha::Queda(format!("A conexão com a {} ficou {} s sem sinal. Tente de novo.", p.rotulo(), sem_sinal.as_secs()))),
            Ok(Err(e)) => return Err(Falha::Queda(format!("A conexão com a {} caiu no meio da resposta ({e}). Tente de novo.", p.rotulo()))),
            Ok(Ok(None)) => break,
            Ok(Ok(Some(b))) => b,
        };
        buf.extend_from_slice(&pedaco);
        while let Some(i) = buf.iter().position(|&b| b == b'\n') {
            let bruta: Vec<u8> = buf.drain(..=i).collect();
            m.linha(&String::from_utf8_lossy(&bruta), &mut dados).map_err(Falha::Outra)?;
        }
    }
    if !buf.is_empty() {
        let resto = String::from_utf8_lossy(&buf).to_string();
        m.linha(&resto, &mut dados).map_err(Falha::Outra)?;
    }
    m.linha("", &mut dados).map_err(Falha::Outra)?;
    if !m.terminou() {
        return Err(Falha::Queda(format!("A resposta da {} veio cortada: a conexão terminou antes do fim. Tente de novo.", p.rotulo())));
    }
    Ok(m.resposta())
}

/// A resposta sendo remontada, evento a evento.
pub(crate) struct Montagem {
    p: Provedor,
    // OpenAI e DeepInfra (chat completions)
    texto: String,
    recusa: String,
    chamadas: Vec<(String, String, String)>,
    fim: Option<String>,
    uso: Option<Value>,
    modelo: String,
    feito: bool,
    // Gemini
    partes: Vec<Value>,
    fim_gemini: Option<String>,
    uso_gemini: Option<Value>,
    bloqueio: Option<Value>,
}

impl Montagem {
    pub(crate) fn novo(p: Provedor) -> Self {
        Montagem {
            p,
            texto: String::new(),
            recusa: String::new(),
            chamadas: vec![],
            fim: None,
            uso: None,
            modelo: String::new(),
            feito: false,
            partes: vec![],
            fim_gemini: None,
            uso_gemini: None,
            bloqueio: None,
        }
    }

    /// Uma linha do streaming: "data:" acumula; a linha em branco fecha o evento.
    pub(crate) fn linha(&mut self, linha: &str, dados: &mut String) -> Result<(), String> {
        let linha = linha.trim_end_matches(['\n', '\r']);
        if linha.is_empty() {
            if !dados.is_empty() {
                let d = std::mem::take(dados);
                self.aplicar(&d)?;
            }
        } else if let Some(v) = linha.strip_prefix("data:") {
            if !dados.is_empty() {
                dados.push('\n');
            }
            dados.push_str(v.strip_prefix(' ').unwrap_or(v));
        }
        Ok(())
    }

    fn aplicar(&mut self, dados: &str) -> Result<(), String> {
        if dados.trim() == "[DONE]" {
            self.feito = true;
            return Ok(());
        }
        let v: Value = serde_json::from_str(dados).map_err(|e| format!("A {} mandou um pedaço ilegível da resposta ({e}).", self.p.rotulo()))?;
        if let Some(e) = v.get("error") {
            let msg = e.get("message").and_then(Value::as_str).map(str::to_string).unwrap_or_else(|| e.to_string());
            return Err(format!("A {} interrompeu a resposta: {}", self.p.rotulo(), corta(&msg, 300)));
        }
        match self.p {
            Provedor::Gemini => self.aplicar_gemini(&v),
            _ => self.aplicar_openai(&v),
        }
        Ok(())
    }

    fn aplicar_openai(&mut self, v: &Value) {
        if let Some(m) = v.get("model").and_then(Value::as_str) {
            self.modelo = m.to_string();
        }
        if let Some(u) = v.get("usage").filter(|u| !u.is_null()) {
            self.uso = Some(u.clone());
        }
        for c in v.get("choices").and_then(Value::as_array).into_iter().flatten() {
            if let Some(d) = c.get("delta") {
                if let Some(t) = d.get("content").and_then(Value::as_str) {
                    self.texto.push_str(t);
                }
                if let Some(t) = d.get("refusal").and_then(Value::as_str) {
                    self.recusa.push_str(t);
                }
                for tc in d.get("tool_calls").and_then(Value::as_array).into_iter().flatten() {
                    let i = tc.get("index").and_then(Value::as_u64).unwrap_or(self.chamadas.len() as u64) as usize;
                    if self.chamadas.len() <= i {
                        self.chamadas.resize(i + 1, (String::new(), String::new(), String::new()));
                    }
                    let ch = &mut self.chamadas[i];
                    if let Some(id) = tc.get("id").and_then(Value::as_str) {
                        ch.0 = id.to_string();
                    }
                    if let Some(f) = tc.get("function") {
                        if let Some(n) = f.get("name").and_then(Value::as_str) {
                            ch.1.push_str(n);
                        }
                        if let Some(a) = f.get("arguments").and_then(Value::as_str) {
                            ch.2.push_str(a);
                        }
                    }
                }
            }
            if let Some(f) = c.get("finish_reason").and_then(Value::as_str) {
                self.fim = Some(f.to_string());
            }
        }
    }

    fn aplicar_gemini(&mut self, v: &Value) {
        if let Some(u) = v.get("usageMetadata") {
            self.uso_gemini = Some(u.clone());
        }
        if let Some(b) = v.get("promptFeedback").filter(|b| b.get("blockReason").is_some()) {
            self.bloqueio = Some(b.clone());
        }
        let Some(c) = v.get("candidates").and_then(Value::as_array).and_then(|a| a.first()) else { return };
        for parte in c.pointer("/content/parts").and_then(Value::as_array).into_iter().flatten() {
            let Some(po) = parte.as_object() else { continue };
            let texto = po.get("text").and_then(Value::as_str);
            let pensou = po.get("thought").and_then(Value::as_bool).unwrap_or(false);
            // texto seguido do mesmo tipo (pensamento ou resposta) se junta; a assinatura fica no pedaço que a trouxe
            if let (Some(t), Some(ult)) = (texto, self.partes.last_mut().and_then(Value::as_object_mut)) {
                let ult_pensou = ult.get("thought").and_then(Value::as_bool).unwrap_or(false);
                if ult.contains_key("text") && ult_pensou == pensou && !ult.contains_key("functionCall") {
                    let atual = ult.get("text").and_then(Value::as_str).unwrap_or("").to_string();
                    ult.insert("text".into(), Value::String(atual + t));
                    if let Some(s) = po.get("thoughtSignature") {
                        ult.insert("thoughtSignature".into(), s.clone());
                    }
                    continue;
                }
            }
            self.partes.push(Value::Object(po.clone()));
        }
        if let Some(f) = c.get("finishReason").and_then(Value::as_str) {
            self.fim_gemini = Some(f.to_string());
        }
    }

    pub(crate) fn terminou(&self) -> bool {
        match self.p {
            Provedor::Gemini => self.fim_gemini.is_some() || self.bloqueio.is_some(),
            _ => self.feito || self.fim.is_some(),
        }
    }

    /// A resposta inteira, como a da chamada sem streaming.
    pub(crate) fn resposta(&self) -> Value {
        match self.p {
            Provedor::Gemini => {
                let mut r = Map::new();
                r.insert(
                    "candidates".into(),
                    json!([{ "content": { "role": "model", "parts": self.partes }, "finishReason": self.fim_gemini }]),
                );
                if let Some(u) = &self.uso_gemini {
                    r.insert("usageMetadata".into(), u.clone());
                }
                if let Some(b) = &self.bloqueio {
                    r.insert("promptFeedback".into(), b.clone());
                }
                Value::Object(r)
            }
            _ => {
                let chamadas: Vec<Value> = self
                    .chamadas
                    .iter()
                    .filter(|c| !c.1.is_empty())
                    .map(|(id, nome, args)| json!({"id": id, "type": "function", "function": {"name": nome, "arguments": args}}))
                    .collect();
                let mut msg = json!({"role": "assistant", "content": if self.texto.is_empty() { Value::Null } else { Value::String(self.texto.clone()) }});
                if !chamadas.is_empty() {
                    msg["tool_calls"] = Value::Array(chamadas);
                }
                if !self.recusa.is_empty() {
                    msg["refusal"] = Value::String(self.recusa.clone());
                }
                json!({"model": self.modelo, "choices": [{"index": 0, "message": msg, "finish_reason": self.fim}], "usage": self.uso})
            }
        }
    }
}

/// Os modelos que a conta da pessoa pode usar (só os de texto), em ordem.
pub async fn listar_modelos(cli: &reqwest::Client, p: Provedor, chave: &str) -> Result<Vec<String>, String> {
    let req = match p {
        Provedor::Gemini => cli.get(format!("{}/models?pageSize=1000", p.base())).header("x-goog-api-key", chave),
        _ => cli.get(format!("{}/models", p.base())).header("authorization", format!("Bearer {chave}")),
    };
    let r = req.timeout(Duration::from_secs(30)).send().await.map_err(|e| format!("A {} não respondeu ({e}).", p.rotulo()))?;
    let code = r.status().as_u16();
    let texto = r.text().await.unwrap_or_default();
    if code != 200 {
        return Err(format!("A {} não listou os modelos (código {code}): {}", p.rotulo(), corta(&texto, 200)));
    }
    let v: Value = serde_json::from_str(&texto).map_err(|e| format!("lista de modelos ilegível: {e}"))?;
    Ok(filtrar_modelos(p, &v))
}

pub(crate) fn filtrar_modelos(p: Provedor, v: &Value) -> Vec<String> {
    let mut out: Vec<String> = match p {
        Provedor::Gemini => v
            .get("models")
            .and_then(Value::as_array)
            .into_iter()
            .flatten()
            .filter(|m| {
                m.get("supportedGenerationMethods").and_then(Value::as_array).is_some_and(|a| a.iter().any(|x| x == "generateContent"))
            })
            .filter_map(|m| m.get("name").and_then(Value::as_str).map(|n| n.trim_start_matches("models/").to_string()))
            .filter(|n| n.starts_with("gemini") && !n.contains("embedding") && !n.contains("image") && !n.contains("tts") && !n.contains("audio"))
            .collect(),
        _ => v
            .get("data")
            .and_then(Value::as_array)
            .into_iter()
            .flatten()
            .filter_map(|m| m.get("id").and_then(Value::as_str).map(str::to_string))
            .filter(|n| {
                let fora = ["embedding", "audio", "realtime", "tts", "transcribe", "whisper", "dall-e", "image", "moderation", "search", "instruct", "babbage", "davinci"];
                if fora.iter().any(|f| n.to_lowercase().contains(f)) {
                    return false;
                }
                p != Provedor::OpenAi || n.starts_with("gpt-") || n.starts_with("chatgpt-") || (n.starts_with('o') && n.chars().nth(1).is_some_and(|c| c.is_ascii_digit()))
            })
            .collect(),
    };
    out.sort();
    out.dedup();
    out
}

#[cfg(test)]
mod testes {
    use super::*;

    fn montar(p: Provedor, eventos: &[&str]) -> Montagem {
        let mut m = Montagem::novo(p);
        let mut d = String::new();
        for e in eventos {
            m.linha(&format!("data: {e}"), &mut d).unwrap();
            m.linha("", &mut d).unwrap();
        }
        m
    }

    #[test]
    fn remonta_a_resposta_da_openai_com_texto_e_consulta() {
        let m = montar(
            Provedor::OpenAi,
            &[
                r#"{"model":"gpt-x","choices":[{"index":0,"delta":{"role":"assistant","content":"Olá, "}}]}"#,
                r#"{"choices":[{"index":0,"delta":{"content":"«REU_01»"}}]}"#,
                r#"{"choices":[{"index":0,"delta":{"tool_calls":[{"index":0,"id":"call_1","type":"function","function":{"name":"ler_modelo","arguments":"{\"id\":"}}]}}]}"#,
                r#"{"choices":[{"index":0,"delta":{"tool_calls":[{"index":0,"function":{"arguments":"\"M1\"}"}}]}}]}"#,
                r#"{"choices":[{"index":0,"delta":{},"finish_reason":"tool_calls"}]}"#,
                r#"{"choices":[],"usage":{"prompt_tokens":100,"completion_tokens":20,"prompt_tokens_details":{"cached_tokens":60}}}"#,
                "[DONE]",
            ],
        );
        assert!(m.terminou());
        let r = m.resposta();
        assert_eq!(r["choices"][0]["message"]["content"], "Olá, «REU_01»");
        assert_eq!(r["choices"][0]["message"]["tool_calls"][0]["function"]["arguments"], "{\"id\":\"M1\"}");
        assert_eq!(r["choices"][0]["message"]["tool_calls"][0]["id"], "call_1");
        assert_eq!(r["choices"][0]["finish_reason"], "tool_calls");
        assert_eq!(r["usage"]["prompt_tokens_details"]["cached_tokens"], 60);
    }

    #[test]
    fn remonta_a_resposta_do_gemini_e_guarda_a_assinatura() {
        let m = montar(
            Provedor::Gemini,
            &[
                r#"{"candidates":[{"content":{"role":"model","parts":[{"text":"pensando","thought":true}]}}]}"#,
                r#"{"candidates":[{"content":{"role":"model","parts":[{"text":"Olá, "}]}}]}"#,
                r#"{"candidates":[{"content":{"role":"model","parts":[{"text":"mundo","thoughtSignature":"sig1"}]}}]}"#,
                r#"{"candidates":[{"content":{"role":"model","parts":[{"functionCall":{"name":"ler_modelo","args":{"id":"M2"}},"thoughtSignature":"sig2"}]},"finishReason":"STOP"}],"usageMetadata":{"promptTokenCount":10,"candidatesTokenCount":5}}"#,
            ],
        );
        assert!(m.terminou());
        let r = m.resposta();
        let partes = r["candidates"][0]["content"]["parts"].as_array().unwrap();
        assert_eq!(partes.len(), 3);
        assert_eq!(partes[0]["thought"], true);
        assert_eq!(partes[1]["text"], "Olá, mundo");
        assert_eq!(partes[1]["thoughtSignature"], "sig1");
        assert_eq!(partes[2]["functionCall"]["args"]["id"], "M2");
        assert_eq!(partes[2]["thoughtSignature"], "sig2");
        assert_eq!(r["usageMetadata"]["promptTokenCount"], 10);
    }

    #[test]
    fn resposta_sem_fim_e_erro_no_meio() {
        assert!(!montar(Provedor::OpenAi, &[r#"{"choices":[{"index":0,"delta":{"content":"meio"}}]}"#]).terminou());
        assert!(!montar(Provedor::Gemini, &[r#"{"candidates":[{"content":{"parts":[{"text":"meio"}]}}]}"#]).terminou());
        let mut m = Montagem::novo(Provedor::OpenAi);
        let mut d = String::new();
        m.linha(r#"data: {"error":{"message":"servidor caiu"}}"#, &mut d).unwrap();
        assert!(m.linha("", &mut d).unwrap_err().contains("servidor caiu"));
    }

    #[test]
    fn confere_chave_e_modelo() {
        assert!(conferir_chave(Provedor::OpenAi, &["sk", "proj", "abcdefghijklmnopqrstuv"].join("-")).is_ok());
        assert!(conferir_chave(Provedor::OpenAi, "AIza-abcdefghijklmnopqrstuv").is_err());
        assert!(conferir_chave(Provedor::Gemini, &format!("AIza{}", "x".repeat(30))).is_ok());
        assert!(conferir_chave(Provedor::DeepInfra, "com espaço no meio da chave de prova").is_err());
        assert!(modelo_valido("gemini-3.1-pro-preview") && modelo_valido("zai-org/GLM-5.2"));
        assert!(!modelo_valido("x/../y") && !modelo_valido("a?b") && !modelo_valido(""));
    }

    #[test]
    fn lista_so_modelos_de_texto() {
        let o = json!({"data":[{"id":"gpt-x"},{"id":"o9-mini"},{"id":"text-embedding-3"},{"id":"whisper-1"},{"id":"gpt-x-audio"},{"id":"omni-moderation"}]});
        assert_eq!(filtrar_modelos(Provedor::OpenAi, &o), vec!["gpt-x", "o9-mini"]);
        let g = json!({"models":[{"name":"models/gemini-x-pro","supportedGenerationMethods":["generateContent"]},
            {"name":"models/text-embedding-004","supportedGenerationMethods":["embedContent"]},
            {"name":"models/gemini-x-image","supportedGenerationMethods":["generateContent"]}]});
        assert_eq!(filtrar_modelos(Provedor::Gemini, &g), vec!["gemini-x-pro"]);
    }
}
