//! As outras IAs do 🧠 (17/11/2026, pedido do usuário: "no botão de gerar a minuta deve ser possível configurar qual
//! IA, qual modelo da IA e o nível de esforço"): a OpenAI, o Google Gemini e a DeepInfra, como no cérebro do Graphé
//! (IA_PROVEDORES). A Anthropic segue em claude.rs.
//!
//! A janela monta a conversa no formato da Anthropic, passa-a pelo porteiro e só então a traduz para o formato do
//! provedor (modulo/ias.js); aqui o pedido ganha a chave do chaveiro e o streaming, e a resposta, que chega aos pedaços,
//! é remontada na resposta inteira do provedor (a mesma da chamada sem streaming). A janela a traduz de volta.
//! Novas tentativas como as da Anthropic: 429 e 5xx até 3 vezes; a conexão que cai no meio é refeita uma vez.
//!
//! O Qwen do servidor (26/11/2026, pedido do usuário: "usar o Qwen deste Mac"): o Ollama de um computador da rede, sem
//! chave (a senha, se o servidor pedir, fica no chaveiro como as chaves). O endereço vem da janela. Pela API do Ollama
//! (/api/chat, NDJSON, com options.num_ctx); o endereço que termina em /v1 é um servidor compatível com a OpenAI (SSE).
//! A resposta é remontada no formato da OpenAI, como a das outras. Sem novas tentativas: cada pedido leva minutos.
use serde_json::{json, Map, Value};
use std::time::{Duration, Instant};

use crate::claude::Resposta;

#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Provedor {
    OpenAi,
    Gemini,
    DeepInfra,
    /// o Qwen do servidor: o endereço vem da janela, a senha (opcional) do chaveiro
    Local,
}

impl Provedor {
    pub fn de(nome: &str) -> Option<Self> {
        match nome {
            "openai" => Some(Self::OpenAi),
            "gemini" => Some(Self::Gemini),
            "deepinfra" => Some(Self::DeepInfra),
            "local" => Some(Self::Local),
            _ => None,
        }
    }
    /// A conta da chave no chaveiro.
    pub fn conta(self) -> &'static str {
        match self {
            Self::OpenAi => "openai",
            Self::Gemini => "gemini",
            Self::DeepInfra => "deepinfra",
            Self::Local => "local",
        }
    }
    pub fn rotulo(self) -> &'static str {
        match self {
            Self::OpenAi => "OpenAI",
            Self::Gemini => "Google Gemini",
            Self::DeepInfra => "DeepInfra",
            Self::Local => "IA do servidor (Qwen)",
        }
    }
    fn base(self) -> &'static str {
        match self {
            Self::OpenAi => "https://api.openai.com/v1",
            Self::Gemini => "https://generativelanguage.googleapis.com/v1beta",
            Self::DeepInfra => "https://api.deepinfra.com/v1/openai",
            Self::Local => "",
        }
    }
    fn onde_chave(self) -> &'static str {
        match self {
            Self::OpenAi => "platform.openai.com/api-keys",
            Self::Gemini => "aistudio.google.com/apikey",
            Self::DeepInfra => "deepinfra.com/dash/keys",
            Self::Local => "",
        }
    }
}

/// A chave colada, limpa e conferida (sem espaço; OpenAI começa por "sk-", Gemini por "AIza").
pub fn conferir_chave(p: Provedor, texto: &str) -> Result<String, String> {
    let chave = texto.trim();
    if p == Provedor::Local {
        // a senha do Qwen do servidor (a do porteiro dele), se ele pedir
        if chave.len() < 8 || chave.chars().any(char::is_whitespace) {
            return Err("A senha do servidor tem 8 caracteres ou mais, sem espaços.".into());
        }
        return Ok(chave.to_string());
    }
    let errada = chave.len() < 20
        || chave.chars().any(char::is_whitespace)
        || (p == Provedor::OpenAi && !chave.starts_with("sk-"))
        || (p == Provedor::Gemini && !chave.starts_with("AIza"));
    if errada {
        let forma = match p {
            Provedor::OpenAi => "ela começa com sk- e não tem espaços",
            Provedor::Gemini => "ela começa com AIza e não tem espaços",
            Provedor::DeepInfra | Provedor::Local => "ela não tem espaços",
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
    if p == Provedor::Local {
        return Err("O Qwen do servidor vai por enviar_local, com o endereço.".into());
    }
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

// ── o Qwen do servidor ──────────────────────────────────────────────────────────────────────────────────────────

/// Sem sinal por até 15 minutos: num M1, o Qwen lê os autos a uns 90 tokens por segundo antes da primeira palavra (o
/// porteiro do servidor manda uma linha em branco a cada 10 s; o Ollama direto, não).
const SEM_SINAL_LOCAL: Duration = Duration::from_secs(900);

/// O endereço do Qwen do servidor, conferido: http ou https, com o computador, sem usuário e senha, consulta nem âncora.
/// Devolve a base (sem a barra do fim) e se é compatível com a OpenAI (termina em /v1) ou o Ollama (o resto).
pub(crate) fn base_local(endereco: &str) -> Result<(String, bool), String> {
    let e = endereco.trim().trim_end_matches('/');
    if e.is_empty() {
        return Err("Falta o endereço do Qwen do servidor: preencha-o na Minuta pelo modelo (ex.: http://192.168.0.10:11434).".into());
    }
    let ruim = || format!("Endereço inválido para o Qwen do servidor: \"{}\". Use, por exemplo, http://192.168.0.10:11434.", corta(e, 80));
    let u = reqwest::Url::parse(e).map_err(|_| ruim())?;
    let ok = (u.scheme() == "http" || u.scheme() == "https")
        && u.host_str().is_some_and(|h| !h.is_empty())
        && u.username().is_empty()
        && u.password().is_none()
        && u.query().is_none()
        && u.fragment().is_none();
    if !ok {
        return Err(ruim());
    }
    let openai = u.path().trim_end_matches('/').ends_with("/v1");
    Ok((e.to_string(), openai))
}

fn erro_local(code: u16, texto: &str, modelo: &str) -> String {
    let msg = serde_json::from_str::<Value>(texto)
        .ok()
        .and_then(|v| v.get("error").map(|e| e.get("message").and_then(Value::as_str).or_else(|| e.as_str()).map(str::to_string).unwrap_or_else(|| e.to_string())))
        .unwrap_or_else(|| texto.to_string());
    let baixo = msg.to_lowercase();
    match code {
        401 | 403 => format!("O Qwen do servidor recusou o acesso (código {code}): guarde a senha certa na Minuta pelo modelo."),
        400 | 404 if baixo.contains("model") || baixo.contains("modelo") => {
            format!("O Qwen do servidor não tem o modelo \"{modelo}\": {}", corta(&msg, 200))
        }
        429 => format!("O Qwen do servidor está ocupado: {}", corta(&msg, 200)),
        _ => format!("O Qwen do servidor retornou {code}: {}", corta(&msg, 300)),
    }
}

/// Manda o pedido (já no formato do Ollama, ou no da OpenAI se o endereço termina em /v1) e devolve a resposta inteira
/// no formato da OpenAI. `senha`: a do porteiro do servidor, se houver (vai como Bearer).
pub async fn enviar_local(cli: &reqwest::Client, endereco: &str, senha: Option<&str>, corpo: &str) -> Result<Resposta, String> {
    let (base, openai) = base_local(endereco)?;
    let mut pedido: Value = serde_json::from_str(corpo).map_err(|e| format!("O pedido montado pela janela não é JSON: {e}"))?;
    let obj = pedido.as_object_mut().ok_or("O pedido montado pela janela não é um objeto JSON.")?;
    let modelo = obj.get("model").and_then(Value::as_str).unwrap_or("").to_string();
    if !modelo_valido(&modelo) {
        return Err(format!("Nome de modelo inválido: \"{}\".", corta(&modelo, 60)));
    }
    obj.insert("stream".into(), Value::Bool(true));
    let url = if openai {
        obj.insert("stream_options".into(), json!({"include_usage": true}));
        format!("{base}/chat/completions")
    } else {
        format!("{base}/api/chat")
    };
    let corpo = pedido.to_string();
    let t0 = Instant::now();
    let mut req = cli.post(&url).header("content-type", "application/json");
    if let Some(s) = senha.filter(|s| !s.is_empty()) {
        req = req.header("authorization", format!("Bearer {s}"));
    }
    let resp = req.body(corpo).send().await.map_err(|e| {
        format!("O Qwen do servidor não respondeu em {base} ({e}). Confira o endereço e se o servidor está ligado e ao alcance desta rede.")
    })?;
    let code = resp.status().as_u16();
    if code != 200 {
        let texto = resp.text().await.unwrap_or_default();
        return Err(erro_local(code, &texto, &modelo));
    }
    let json = if openai {
        match ler(resp, Provedor::Local, SEM_SINAL_LOCAL).await {
            Ok(j) => j,
            Err(Falha::Queda(e)) | Err(Falha::Outra(e)) => return Err(e),
        }
    } else {
        ler_ollama(resp, SEM_SINAL_LOCAL).await?
    };
    Ok(Resposta { json, ms: t0.elapsed().as_millis() as u64, code, tentativas: 1, quedas: 0 })
}

async fn ler_ollama(mut resp: reqwest::Response, sem_sinal: Duration) -> Result<Value, String> {
    let mut m = MontagemOllama::default();
    let mut buf: Vec<u8> = Vec::new();
    loop {
        let pedaco = match tokio::time::timeout(sem_sinal, resp.chunk()).await {
            Err(_) => return Err(format!("O Qwen do servidor ficou {} s sem sinal. Tente de novo.", sem_sinal.as_secs())),
            Ok(Err(e)) => return Err(format!("A conexão com o Qwen do servidor caiu no meio da resposta ({e}). Tente de novo.")),
            Ok(Ok(None)) => break,
            Ok(Ok(Some(b))) => b,
        };
        buf.extend_from_slice(&pedaco);
        while let Some(i) = buf.iter().position(|&b| b == b'\n') {
            let bruta: Vec<u8> = buf.drain(..=i).collect();
            m.linha(&String::from_utf8_lossy(&bruta))?;
        }
    }
    if !buf.is_empty() {
        let resto = String::from_utf8_lossy(&buf).to_string();
        m.linha(&resto)?;
    }
    if !m.feito {
        return Err("A resposta do Qwen do servidor veio cortada: a conexão terminou antes do fim. Tente de novo.".into());
    }
    Ok(m.resposta())
}

/// A resposta do Ollama (/api/chat, uma linha JSON por pedaço) sendo remontada no formato da OpenAI.
#[derive(Default)]
pub(crate) struct MontagemOllama {
    texto: String,
    chamadas: Vec<Value>,
    fim: Option<String>,
    modelo: String,
    entrada: u64,
    saida: u64,
    feito: bool,
}

impl MontagemOllama {
    /// Uma linha do NDJSON. A linha em branco é o sinal de vida do porteiro do servidor.
    pub(crate) fn linha(&mut self, linha: &str) -> Result<(), String> {
        let l = linha.trim();
        if l.is_empty() {
            return Ok(());
        }
        let v: Value = serde_json::from_str(l).map_err(|e| format!("O Qwen do servidor mandou um pedaço ilegível da resposta ({e})."))?;
        if let Some(e) = v.get("error") {
            let msg = e.as_str().map(str::to_string).unwrap_or_else(|| e.to_string());
            return Err(format!("O Qwen do servidor interrompeu a resposta: {}", corta(&msg, 300)));
        }
        if let Some(m) = v.get("model").and_then(Value::as_str) {
            self.modelo = m.to_string();
        }
        if let Some(msg) = v.get("message") {
            // o raciocínio (message.thinking) não entra na resposta
            if let Some(t) = msg.get("content").and_then(Value::as_str) {
                self.texto.push_str(t);
            }
            for tc in msg.get("tool_calls").and_then(Value::as_array).into_iter().flatten() {
                let f = tc.get("function").cloned().unwrap_or(Value::Null);
                let nome = f.get("name").and_then(Value::as_str).unwrap_or("").to_string();
                let args = match f.get("arguments") {
                    Some(Value::String(s)) => s.clone(),
                    Some(a) => a.to_string(),
                    None => "{}".to_string(),
                };
                let id = format!("call_{}", self.chamadas.len() + 1);
                self.chamadas.push(json!({"id": id, "type": "function", "function": {"name": nome, "arguments": args}}));
            }
        }
        if v.get("done").and_then(Value::as_bool).unwrap_or(false) {
            self.feito = true;
            let fim = if v.get("done_reason").and_then(Value::as_str) == Some("length") {
                "length"
            } else if !self.chamadas.is_empty() {
                "tool_calls"
            } else {
                "stop"
            };
            self.fim = Some(fim.to_string());
            self.entrada = v.get("prompt_eval_count").and_then(Value::as_u64).unwrap_or(0);
            self.saida = v.get("eval_count").and_then(Value::as_u64).unwrap_or(0);
        }
        Ok(())
    }

    /// A resposta inteira, como a da OpenAI sem streaming.
    pub(crate) fn resposta(&self) -> Value {
        let conteudo = if self.texto.is_empty() { Value::Null } else { Value::String(self.texto.clone()) };
        let mut msg = json!({"role": "assistant", "content": conteudo});
        if !self.chamadas.is_empty() {
            msg["tool_calls"] = Value::Array(self.chamadas.clone());
        }
        json!({"model": self.modelo, "choices": [{"index": 0, "message": msg, "finish_reason": self.fim}],
               "usage": {"prompt_tokens": self.entrada, "completion_tokens": self.saida}})
    }
}

/// Os modelos que o servidor tem (/api/tags do Ollama, ou /models do compatível com a OpenAI).
pub async fn listar_modelos_local(cli: &reqwest::Client, endereco: &str, senha: Option<&str>) -> Result<Vec<String>, String> {
    let (base, openai) = base_local(endereco)?;
    let url = if openai { format!("{base}/models") } else { format!("{base}/api/tags") };
    let mut req = cli.get(&url).timeout(Duration::from_secs(20));
    if let Some(s) = senha.filter(|s| !s.is_empty()) {
        req = req.header("authorization", format!("Bearer {s}"));
    }
    let r = req.send().await.map_err(|e| format!("O Qwen do servidor não respondeu em {base} ({e})."))?;
    let code = r.status().as_u16();
    let texto = r.text().await.unwrap_or_default();
    if code != 200 {
        return Err(erro_local(code, &texto, ""));
    }
    let v: Value = serde_json::from_str(&texto).map_err(|e| format!("lista de modelos ilegível: {e}"))?;
    let (lista, campo) = if openai { ("data", "id") } else { ("models", "name") };
    let mut out: Vec<String> = v
        .get(lista)
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
        .filter_map(|m| m.get(campo).and_then(Value::as_str).map(str::to_string))
        .collect();
    out.sort();
    out.dedup();
    Ok(out)
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
    fn remonta_a_resposta_do_ollama_com_o_sinal_de_vida() {
        // as linhas como o Ollama do servidor as mandou (qwen3.5:9b, 26/11/2026), com as linhas em branco do porteiro
        let mut m = MontagemOllama::default();
        for l in [
            "\n",
            "\n",
            r#"{"model":"qwen3.5:9b","created_at":"2026-11-26T17:54:30Z","message":{"role":"assistant","content":"{\n  \"resumo\": "},"done":false}"#,
            r#"{"model":"qwen3.5:9b","created_at":"2026-11-26T17:54:31Z","message":{"role":"assistant","content":"\"«REU_01» respondeu\"\n}"},"done":false}"#,
            r#"{"model":"qwen3.5:9b","message":{"role":"assistant","content":""},"done":true,"done_reason":"stop","prompt_eval_count":2977,"eval_count":43}"#,
        ] {
            m.linha(l).unwrap();
        }
        assert!(m.feito);
        let r = m.resposta();
        assert_eq!(r["choices"][0]["message"]["content"], "{\n  \"resumo\": \"«REU_01» respondeu\"\n}");
        assert_eq!(r["choices"][0]["finish_reason"], "stop");
        assert_eq!(r["usage"]["prompt_tokens"], 2977);
        assert_eq!(r["model"], "qwen3.5:9b");
    }

    #[test]
    fn ollama_com_ferramenta_corte_e_erro() {
        let mut m = MontagemOllama::default();
        m.linha(r#"{"message":{"role":"assistant","content":"","tool_calls":[{"function":{"name":"ler_modelo","arguments":{"id":"M1"}}}]},"done":false}"#).unwrap();
        m.linha(r#"{"message":{"content":""},"done":true,"done_reason":"stop"}"#).unwrap();
        let r = m.resposta();
        assert_eq!(r["choices"][0]["message"]["tool_calls"][0]["function"]["arguments"], "{\"id\":\"M1\"}");
        assert_eq!(r["choices"][0]["finish_reason"], "tool_calls");
        let mut c = MontagemOllama::default();
        c.linha(r#"{"message":{"content":"meio"},"done":false}"#).unwrap();
        assert!(!c.feito);
        c.linha(r#"{"message":{"content":""},"done":true,"done_reason":"length"}"#).unwrap();
        assert_eq!(c.resposta()["choices"][0]["finish_reason"], "length");
        let mut e = MontagemOllama::default();
        assert!(e.linha(r#"{"error":"o Ollama respondeu 500: sem memória"}"#).unwrap_err().contains("sem memória"));
    }

    #[test]
    fn confere_o_endereco_do_servidor() {
        assert_eq!(base_local("http://192.168.0.10:11434/").unwrap(), ("http://192.168.0.10:11434".to_string(), false));
        assert_eq!(base_local(" http://servidor.local:1234/v1 ").unwrap(), ("http://servidor.local:1234/v1".to_string(), true));
        assert!(base_local("https://qwen.exemplo.br").is_ok());
        for ruim in ["", "192.168.0.10:11434", "ftp://x", "http://usuario:senha@x:11434", "http://x:11434/?a=1", "file:///etc/passwd"] {
            assert!(base_local(ruim).is_err(), "{ruim}");
        }
        assert!(modelo_valido("qwen3.5:9b"));
        assert!(conferir_chave(Provedor::Local, "curta").is_err());
        assert!(conferir_chave(Provedor::Local, "uma-senha-de-prova-comprida").is_ok());
        assert!(erro_local(403, r#"{"error":"proibido"}"#, "qwen3.5:9b").contains("recusou"));
        assert!(erro_local(400, r#"{"error":"modelo não instalado neste servidor: x"}"#, "x").contains("não tem o modelo"));
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
