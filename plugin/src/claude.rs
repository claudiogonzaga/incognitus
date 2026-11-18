//! A chamada à API da Anthropic, do lado do Rust (vem do Graphé Desktop, src-tauri/src/claude.rs). O pedido chega pronto do núcleo (o mesmo texto que o Graphé mandaria,
//! sem a chave). Aqui ele ganha a chave do chaveiro e "stream": true, vai à API, e a resposta, que chega aos pedaços, é
//! remontada na mensagem inteira, igual à da chamada sem streaming, para o núcleo seguir como no Graphé. O streaming é
//! por causa dos pedidos longos: o servidor manda sinais de vida enquanto a IA pensa, e a conexão não cai calada.
//!
//! As novas tentativas e as mensagens de erro são as do _claudeHttp_ do Graphé:
//!   · 429, 5xx e "overloaded": até 3 tentativas, com espera crescente (nada é cobrado nelas);
//!   · 400 que recusa o fallback (é beta): o pedido segue sem ele, sem contar tentativa;
//!   · 401/403 (a chave), 404 ou 400 de modelo e 400 sem crédito: erro na hora, com a explicação;
//!   · sem resposta, ou a Anthropic que interrompe a resposta com um erro: erro, sem nova tentativa.
//! Uma regra é só daqui (17/11/2026): a conexão que cai no meio da resposta (a rede derruba, a resposta vem cortada ou
//! fica sem sinal) é refeita uma vez, sozinha. A Anthropic pode ter cobrado o pedaço que caiu, mas refazer só este
//! pedido sai mais barato do que a pessoa recomeçar a rodada inteira. O Graphé não repete nesse caso.
//! A recusa da triagem de segurança (stop_reason "refusal") volta como resposta: quem a transforma em erro é o
//! núcleo, como no Graphé.

use serde::Serialize;
use serde_json::{Map, Value};
use std::collections::{BTreeMap, HashMap};
use std::time::{Duration, Instant};

pub const URL_PADRAO: &str = "https://api.anthropic.com/v1/messages";
/// Os cabeçalhos que o núcleo pode mandar. O resto, e qualquer x-api-key, fica de fora: a chave é a do chaveiro.
const CABECALHOS_ACEITOS: [&str; 2] = ["anthropic-version", "anthropic-beta"];

pub struct Opcoes {
    pub url: String,
    pub tentativas: u32,
    /// A espera antes da 1ª nova tentativa; a n-ésima espera n vezes isto.
    pub espera: Duration,
    /// Sem nenhum byte por este tempo, a conexão é dada como caída (a Anthropic manda "ping" enquanto a IA pensa).
    pub sem_sinal: Duration,
    /// Quantas vezes o pedido é refeito quando a conexão cai no meio da resposta.
    pub quedas: u32,
}

impl Default for Opcoes {
    fn default() -> Self {
        Opcoes { url: URL_PADRAO.into(), tentativas: 3, espera: Duration::from_millis(1500), sem_sinal: Duration::from_secs(180), quedas: 1 }
    }
}

#[derive(Debug, Serialize)]
pub struct Resposta {
    pub json: Value,
    pub ms: u64,
    pub code: u16,
    pub tentativas: u32,
    /// Quantas vezes a conexão caiu no meio da resposta e o pedido foi refeito.
    pub quedas: u32,
}

/// O que deu errado na leitura da resposta: a conexão que caiu (o pedido pode ser refeito) ou o resto (não se repete).
enum Falha {
    Queda(String),
    Outra(String),
}

pub fn cliente() -> Result<reqwest::Client, String> {
    reqwest::Client::builder()
        .connect_timeout(Duration::from_secs(30))
        // sinal de vida no TCP: roteador e filtro de rede derrubam a conexão longa que parece parada
        .tcp_keepalive(Duration::from_secs(30))
        .build()
        .map_err(|e| format!("não consegui preparar a conexão HTTP: {e}"))
}

fn corta(s: &str, n: usize) -> String {
    s.chars().take(n).collect()
}

fn motivo(e: &reqwest::Error) -> String {
    if e.is_timeout() {
        "tempo esgotado".into()
    } else if e.is_connect() {
        "sem conexão com a Anthropic".into()
    } else {
        // O reqwest só diz "error decoding response body"; a causa de verdade (conexão fechada pelo outro lado,
        // reiniciada pela rede…) vem atrás, na cadeia do erro. Ela vai junto, para a queda não ficar sem explicação.
        use std::error::Error as _;
        let mut partes = vec![e.to_string()];
        let mut causa = e.source();
        while let Some(c) = causa {
            let t = c.to_string();
            if !partes.contains(&t) {
                partes.push(t);
            }
            causa = c.source();
        }
        corta(&partes.join(": "), 240)
    }
}

/// Manda o pedido e devolve a mensagem inteira da resposta.
pub async fn enviar(
    cli: &reqwest::Client,
    op: &Opcoes,
    chave: &str,
    corpo: &str,
    cabecalhos: &BTreeMap<String, String>,
) -> Result<Resposta, String> {
    let mut pedido: Value = serde_json::from_str(corpo).map_err(|e| format!("O pedido montado pelo núcleo não é JSON: {e}"))?;
    let obj = pedido.as_object_mut().ok_or("O pedido montado pelo núcleo não é um objeto JSON.")?;
    obj.insert("stream".into(), Value::Bool(true));
    let modelo = obj.get("model").and_then(Value::as_str).unwrap_or("").to_string();
    let mut cab: BTreeMap<String, String> = cabecalhos
        .iter()
        .map(|(k, v)| (k.to_ascii_lowercase(), v.clone()))
        .filter(|(k, _)| CABECALHOS_ACEITOS.contains(&k.as_str()))
        .collect();
    let t0 = Instant::now();
    let mut tentativa = 0u32;
    let mut quedas = 0u32;
    let mut ultimo_erro = String::new();
    while tentativa < op.tentativas {
        if tentativa > 0 {
            tokio::time::sleep(op.espera * tentativa).await;
        }
        let mut req = cli
            .post(&op.url)
            .header("x-api-key", chave)
            .header("content-type", "application/json")
            .header("accept", "text/event-stream");
        for (k, v) in &cab {
            req = req.header(k.as_str(), v.as_str());
        }
        let resp = match req.body(pedido.to_string()).send().await {
            Ok(r) => r,
            Err(e) => return Err(format!("O Claude não respondeu ({}). Tente de novo; se repetir, confira a internet.", motivo(&e))),
        };
        let code = resp.status().as_u16();
        if code == 200 {
            match ler_streaming(resp, op.sem_sinal).await {
                Ok(json) => return Ok(Resposta { json, ms: t0.elapsed().as_millis() as u64, code, tentativas: tentativa + 1, quedas }),
                Err(Falha::Queda(_)) if quedas < op.quedas => {
                    quedas += 1;
                    tokio::time::sleep(op.espera).await;
                    continue;
                }
                Err(Falha::Queda(e)) => {
                    return Err(if quedas > 0 {
                        format!("{e} O app já tinha refeito o pedido sozinho e a conexão caiu outra vez: confira a internet.")
                    } else {
                        e
                    })
                }
                Err(Falha::Outra(e)) => return Err(e),
            }
        }
        let texto = resp.text().await.unwrap_or_default();
        let baixo = texto.to_lowercase();
        // O fallback é beta: se esta conta não o aceita, o pedido segue sem ele.
        if code == 400 && pedido.get("fallbacks").is_some() && (baixo.contains("fallback") || baixo.contains("beta")) {
            if let Some(o) = pedido.as_object_mut() {
                o.remove("fallbacks");
            }
            cab.remove("anthropic-beta");
            continue;
        }
        if code == 401 || code == 403 {
            return Err(format!(
                "A chave da Anthropic foi recusada (inválida ou expirada, código {code}). Guarde a chave certa nas Configurações."
            ));
        }
        if code == 404 || (code == 400 && baixo.contains("model")) {
            return Err(format!("Modelo \"{modelo}\" não reconhecido pela Anthropic: {}", corta(&texto, 200)));
        }
        if code == 400 && (baixo.contains("credit balance") || baixo.contains("billing")) {
            return Err("A conta da Anthropic está sem crédito. Recarregue em console.anthropic.com → Billing.".into());
        }
        if code >= 500 || code == 429 || baixo.contains("overloaded") {
            ultimo_erro = format!("código {code}: {}", corta(&texto, 200));
            tentativa += 1;
            continue;
        }
        return Err(format!("Anthropic retornou {code}: {}", corta(&texto, 300)));
    }
    Err(format!("Anthropic indisponível após {} tentativas. Último erro: {ultimo_erro}", op.tentativas))
}

/// Lê os eventos do streaming (linhas "data: {...}", um evento por linha em branco) e remonta a mensagem.
async fn ler_streaming(mut resp: reqwest::Response, sem_sinal: Duration) -> Result<Value, Falha> {
    let mut m = Montagem::default();
    let mut buf: Vec<u8> = Vec::new();
    let mut dados = String::new();
    loop {
        let pedaco = match tokio::time::timeout(sem_sinal, resp.chunk()).await {
            Err(_) => {
                return Err(Falha::Queda(format!(
                    "A conexão com a Anthropic ficou {} s sem sinal e foi dada como caída. Tente de novo.",
                    sem_sinal.as_secs()
                )))
            }
            Ok(Err(e)) => {
                return Err(Falha::Queda(format!("A conexão com a Anthropic caiu no meio da resposta ({}). Tente de novo.", motivo(&e))))
            }
            Ok(Ok(None)) => break,
            Ok(Ok(Some(b))) => b,
        };
        buf.extend_from_slice(&pedaco);
        while let Some(i) = buf.iter().position(|&b| b == b'\n') {
            let bruta: Vec<u8> = buf.drain(..=i).collect();
            m.linha(&String::from_utf8_lossy(&bruta), &mut dados).map_err(Falha::Outra)?;
        }
        if m.fim {
            break;
        }
    }
    if !buf.is_empty() {
        let resto = String::from_utf8_lossy(&buf).to_string();
        m.linha(&resto, &mut dados).map_err(Falha::Outra)?;
    }
    m.linha("", &mut dados).map_err(Falha::Outra)?;
    if !m.fim {
        return Err(Falha::Queda(CORTADA.into()));
    }
    m.mensagem().map_err(Falha::Outra)
}

const CORTADA: &str = "A resposta da Anthropic veio cortada: a conexão terminou antes do fim. Tente de novo.";

/// A mensagem sendo remontada, evento a evento.
#[derive(Default)]
struct Montagem {
    msg: Option<Map<String, Value>>,
    blocos: Vec<Value>,
    json_parcial: HashMap<usize, String>,
    fim: bool,
}

fn indice(v: &Value) -> Result<usize, String> {
    v.get("index").and_then(Value::as_u64).map(|i| i as usize).ok_or_else(|| "evento da Anthropic sem o índice do bloco".to_string())
}

impl Montagem {
    /// Uma linha do streaming. "data:" acumula; a linha em branco fecha o evento. "event:" repete o "type" dos dados,
    /// e ":" é comentário.
    fn linha(&mut self, linha: &str, dados: &mut String) -> Result<(), String> {
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
        let v: Value = serde_json::from_str(dados).map_err(|e| format!("A Anthropic mandou um pedaço ilegível da resposta ({e})."))?;
        match v.get("type").and_then(Value::as_str).unwrap_or("") {
            "message_start" => {
                let mut msg = v.get("message").and_then(Value::as_object).cloned().ok_or("o message_start veio sem a mensagem")?;
                msg.insert("content".into(), Value::Array(vec![]));
                self.msg = Some(msg);
            }
            "content_block_start" => {
                let i = indice(&v)?;
                let bloco = v.get("content_block").cloned().ok_or("o content_block_start veio sem o bloco")?;
                if self.blocos.len() <= i {
                    self.blocos.resize(i + 1, Value::Null);
                }
                self.blocos[i] = bloco;
            }
            "content_block_delta" => {
                let i = indice(&v)?;
                let delta = v.get("delta").and_then(Value::as_object).ok_or("o content_block_delta veio sem o delta")?;
                let bloco = self.blocos.get_mut(i).and_then(Value::as_object_mut).ok_or("veio um pedaço de um bloco que não começou")?;
                match delta.get("type").and_then(Value::as_str).unwrap_or("") {
                    "input_json_delta" => self
                        .json_parcial
                        .entry(i)
                        .or_default()
                        .push_str(delta.get("partial_json").and_then(Value::as_str).unwrap_or("")),
                    "citations_delta" => {
                        if let Some(c) = delta.get("citation") {
                            if let Some(a) = bloco.entry("citations").or_insert_with(|| Value::Array(vec![])).as_array_mut() {
                                a.push(c.clone());
                            }
                        }
                    }
                    // text_delta (text), thinking_delta (thinking), signature_delta (signature) e os que vierem: o
                    // texto do delta se junta ao campo de mesmo nome do bloco.
                    _ => {
                        for (k, val) in delta {
                            if k == "type" {
                                continue;
                            }
                            if let Some(s) = val.as_str() {
                                let atual = bloco.get(k).and_then(Value::as_str).unwrap_or("").to_string();
                                bloco.insert(k.clone(), Value::String(atual + s));
                            }
                        }
                    }
                }
            }
            "content_block_stop" => {
                let i = indice(&v)?;
                if let Some(parcial) = self.json_parcial.remove(&i) {
                    let input: Value = if parcial.trim().is_empty() {
                        Value::Object(Map::new())
                    } else {
                        serde_json::from_str(&parcial).map_err(|e| format!("A entrada de uma consulta da IA veio ilegível ({e})."))?
                    };
                    if let Some(b) = self.blocos.get_mut(i).and_then(Value::as_object_mut) {
                        b.insert("input".into(), input);
                    }
                }
            }
            "message_delta" => {
                let msg = self.msg.as_mut().ok_or("o message_delta veio antes do message_start")?;
                if let Some(d) = v.get("delta").and_then(Value::as_object) {
                    for (k, val) in d {
                        msg.insert(k.clone(), val.clone());
                    }
                }
                if let Some(u) = v.get("usage").and_then(Value::as_object) {
                    if let Some(uso) = msg.entry("usage").or_insert_with(|| Value::Object(Map::new())).as_object_mut() {
                        for (k, val) in u {
                            uso.insert(k.clone(), val.clone());
                        }
                    }
                }
            }
            "message_stop" => self.fim = true,
            "error" => {
                let e = v.get("error");
                let tipo = e.and_then(|e| e.get("type")).and_then(Value::as_str).unwrap_or("erro");
                let texto = e.and_then(|e| e.get("message")).and_then(Value::as_str).unwrap_or("");
                return Err(if tipo == "overloaded_error" {
                    "A Anthropic ficou sobrecarregada no meio da resposta. Tente de novo em alguns minutos.".into()
                } else {
                    format!("A Anthropic interrompeu a resposta ({tipo}: {}).", corta(texto, 200))
                });
            }
            _ => {} // ping e o que vier de novo
        }
        Ok(())
    }

    fn mensagem(self) -> Result<Value, String> {
        if !self.fim {
            return Err(CORTADA.into());
        }
        let mut msg = self.msg.ok_or("A resposta da Anthropic veio sem a mensagem.")?;
        msg.insert("content".into(), Value::Array(self.blocos.into_iter().filter(|b| !b.is_null()).collect()));
        Ok(Value::Object(msg))
    }
}

#[cfg(test)]
mod testes {
    use super::*;
    use serde_json::json;
    use std::io::{Read, Write};
    use std::net::TcpListener;
    use std::sync::{Arc, Mutex};

    /// Um servidor de mentira: responde, em ordem, uma resposta crua por conexão, e guarda cada pedido (cabeçalhos e
    /// corpo).
    struct Servidor {
        url: String,
        pedidos: Arc<Mutex<Vec<(String, String)>>>,
    }

    fn servidor(respostas: Vec<String>) -> Servidor {
        let l = TcpListener::bind("127.0.0.1:0").unwrap();
        let url = format!("http://{}/v1/messages", l.local_addr().unwrap());
        let pedidos = Arc::new(Mutex::new(vec![]));
        let guardados = pedidos.clone();
        std::thread::spawn(move || {
            for resposta in respostas {
                let (mut s, _) = l.accept().unwrap();
                let mut dados = vec![];
                let mut b = [0u8; 16384];
                let (cab, corpo) = loop {
                    let n = s.read(&mut b).unwrap();
                    dados.extend_from_slice(&b[..n]);
                    if let Some(i) = dados.windows(4).position(|w| w == b"\r\n\r\n") {
                        let cab = String::from_utf8_lossy(&dados[..i]).to_string();
                        let tam = cab
                            .lines()
                            .find_map(|l| l.to_ascii_lowercase().strip_prefix("content-length:").map(|v| v.trim().parse::<usize>().unwrap()))
                            .unwrap_or(0);
                        while dados.len() < i + 4 + tam {
                            let n = s.read(&mut b).unwrap();
                            dados.extend_from_slice(&b[..n]);
                        }
                        break (cab, String::from_utf8_lossy(&dados[i + 4..i + 4 + tam]).to_string());
                    }
                };
                guardados.lock().unwrap().push((cab, corpo));
                s.write_all(resposta.as_bytes()).unwrap();
            }
        });
        Servidor { url, pedidos }
    }

    fn sse(eventos: &[Value]) -> String {
        let corpo: String = eventos.iter().map(|e| format!("event: {}\ndata: {}\n\n", e["type"].as_str().unwrap(), e)).collect();
        format!("HTTP/1.1 200 OK\r\ncontent-type: text/event-stream\r\nconnection: close\r\n\r\n{corpo}")
    }

    fn http(code: u16, corpo: &str) -> String {
        format!("HTTP/1.1 {code} X\r\ncontent-type: application/json\r\ncontent-length: {}\r\nconnection: close\r\n\r\n{corpo}", corpo.len())
    }

    fn opcoes(url: &str) -> Opcoes {
        Opcoes { url: url.into(), tentativas: 3, espera: Duration::from_millis(1), sem_sinal: Duration::from_secs(5), quedas: 1 }
    }

    fn cab() -> BTreeMap<String, String> {
        let mut c = BTreeMap::new();
        c.insert("anthropic-version".into(), "2023-06-01".into());
        c.insert("anthropic-beta".into(), "server-side-fallback-2026-07-01".into());
        c.insert("x-api-key".into(), "vazou".into());
        c.insert("x-outro".into(), "fica de fora".into());
        c
    }

    const PEDIDO: &str = r#"{"model":"claude-opus-5-5","max_tokens":32000,"messages":[{"role":"user","content":"oi"}],"fallbacks":"default"}"#;
    // Em pedaços, para a trava do repositório não a tomar por uma chave de verdade.
    const CHAVE_DE_PROVA: &str = concat!("sk-", "ant-de-prova");

    /// Uma resposta inteira: raciocínio com assinatura, texto em dois pedaços e uma consulta com a entrada partida.
    fn conversa() -> Vec<Value> {
        vec![
            json!({"type":"message_start","message":{"id":"msg_1","type":"message","role":"assistant","model":"claude-opus-5-5","content":[],"stop_reason":null,"stop_sequence":null,"usage":{"input_tokens":10,"cache_read_input_tokens":200,"output_tokens":1}}}),
            json!({"type":"content_block_start","index":0,"content_block":{"type":"thinking","thinking":"","signature":""}}),
            json!({"type":"ping"}),
            json!({"type":"content_block_delta","index":0,"delta":{"type":"thinking_delta","thinking":"pen"}}),
            json!({"type":"content_block_delta","index":0,"delta":{"type":"thinking_delta","thinking":"sa"}}),
            json!({"type":"content_block_delta","index":0,"delta":{"type":"signature_delta","signature":"sig-xyz"}}),
            json!({"type":"content_block_stop","index":0}),
            json!({"type":"content_block_start","index":1,"content_block":{"type":"text","text":""}}),
            json!({"type":"content_block_delta","index":1,"delta":{"type":"text_delta","text":"Olá, "}}),
            json!({"type":"content_block_delta","index":1,"delta":{"type":"text_delta","text":"mundo «Sócrates»"}}),
            json!({"type":"content_block_stop","index":1}),
            json!({"type":"content_block_start","index":2,"content_block":{"type":"tool_use","id":"tu1","name":"ler_modelo","input":{}}}),
            json!({"type":"content_block_delta","index":2,"delta":{"type":"input_json_delta","partial_json":"{\"id\": \"M"}}),
            json!({"type":"content_block_delta","index":2,"delta":{"type":"input_json_delta","partial_json":"1\"}"}}),
            json!({"type":"content_block_stop","index":2}),
            json!({"type":"message_delta","delta":{"stop_reason":"tool_use","stop_sequence":null},"usage":{"output_tokens":42}}),
            json!({"type":"message_stop"}),
        ]
    }

    #[tokio::test]
    async fn remonta_a_mensagem_do_streaming_e_manda_a_chave_do_chaveiro() {
        let s = servidor(vec![sse(&conversa())]);
        let r = enviar(&cliente().unwrap(), &opcoes(&s.url), CHAVE_DE_PROVA, PEDIDO, &cab()).await.unwrap();
        assert_eq!(
            r.json,
            json!({"id":"msg_1","type":"message","role":"assistant","model":"claude-opus-5-5",
                "content":[{"type":"thinking","thinking":"pensa","signature":"sig-xyz"},{"type":"text","text":"Olá, mundo «Sócrates»"},
                           {"type":"tool_use","id":"tu1","name":"ler_modelo","input":{"id":"M1"}}],
                "stop_reason":"tool_use","stop_sequence":null,"usage":{"input_tokens":10,"cache_read_input_tokens":200,"output_tokens":42}})
        );
        assert_eq!((r.code, r.tentativas), (200, 1));
        let pedidos = s.pedidos.lock().unwrap();
        let (cabecalhos, corpo) = &pedidos[0];
        let baixo = cabecalhos.to_ascii_lowercase();
        assert!(baixo.contains(&format!("x-api-key: {CHAVE_DE_PROVA}")) && !baixo.contains("vazou") && !baixo.contains("x-outro"));
        assert!(baixo.contains("anthropic-version: 2023-06-01") && baixo.contains("anthropic-beta: server-side-fallback-2026-07-01"));
        let enviado: Value = serde_json::from_str(corpo).unwrap();
        let mut esperado: Value = serde_json::from_str(PEDIDO).unwrap();
        esperado["stream"] = json!(true);
        assert_eq!(enviado, esperado, "o pedido vai como o núcleo montou, só com o streaming");
    }

    #[tokio::test]
    async fn tenta_de_novo_na_sobrecarga() {
        let s = servidor(vec![http(529, r#"{"type":"error","error":{"type":"overloaded_error","message":"Overloaded"}}"#), sse(&conversa())]);
        let r = enviar(&cliente().unwrap(), &opcoes(&s.url), "sk-ant-x", PEDIDO, &cab()).await.unwrap();
        assert_eq!(r.tentativas, 2);
        assert_eq!(s.pedidos.lock().unwrap().len(), 2);
    }

    #[tokio::test]
    async fn sem_o_fallback_quando_a_conta_nao_o_aceita() {
        let s = servidor(vec![http(400, r#"{"type":"error","error":{"type":"invalid_request_error","message":"fallbacks: Extra inputs are not permitted"}}"#), sse(&conversa())]);
        let r = enviar(&cliente().unwrap(), &opcoes(&s.url), "sk-ant-x", PEDIDO, &cab()).await.unwrap();
        assert_eq!(r.tentativas, 1, "não conta como nova tentativa");
        let pedidos = s.pedidos.lock().unwrap();
        let segundo: Value = serde_json::from_str(&pedidos[1].1).unwrap();
        assert!(segundo.get("fallbacks").is_none() && !pedidos[1].0.to_ascii_lowercase().contains("anthropic-beta"));
    }

    #[tokio::test]
    async fn erros_que_nao_se_repetem() {
        let casos = [
            (401, r#"{"type":"error","error":{"type":"authentication_error","message":"invalid x-api-key"}}"#, "chave da Anthropic foi recusada"),
            (400, r#"{"type":"error","error":{"type":"invalid_request_error","message":"Your credit balance is too low"}}"#, "sem crédito"),
            (404, r#"{"type":"error","error":{"type":"not_found_error","message":"model: claude-x"}}"#, "não reconhecido"),
            (400, r#"{"type":"error","error":{"type":"invalid_request_error","message":"messages: vazio"}}"#, "Anthropic retornou 400"),
        ];
        for (code, corpo, esperado) in casos {
            let s = servidor(vec![http(code, corpo)]);
            let e = enviar(&cliente().unwrap(), &opcoes(&s.url), "sk-ant-x", r#"{"model":"claude-opus-5-5"}"#, &cab()).await.unwrap_err();
            assert!(e.contains(esperado), "{code}: {e}");
            assert!(!e.contains("sk-ant-x"), "a chave nunca aparece no erro");
            assert_eq!(s.pedidos.lock().unwrap().len(), 1, "{code}: uma tentativa só");
        }
    }

    #[tokio::test]
    async fn desiste_depois_de_tres_tentativas() {
        let s = servidor(vec![http(503, "indisponível"), http(503, "indisponível"), http(503, "indisponível")]);
        let e = enviar(&cliente().unwrap(), &opcoes(&s.url), "sk-ant-x", PEDIDO, &cab()).await.unwrap_err();
        assert!(e.contains("indisponível após 3 tentativas") && e.contains("código 503"), "{e}");
        assert_eq!(s.pedidos.lock().unwrap().len(), 3);
    }

    #[tokio::test]
    async fn a_conexao_que_cai_no_meio_e_refeita_uma_vez() {
        let mut cortada = conversa();
        cortada.truncate(8);
        // caiu uma vez: o pedido é refeito sozinho, igual, e a resposta inteira volta com a queda anotada
        let s = servidor(vec![sse(&cortada), sse(&conversa())]);
        let r = enviar(&cliente().unwrap(), &opcoes(&s.url), "sk-ant-x", PEDIDO, &cab()).await.unwrap();
        assert_eq!((r.quedas, r.tentativas), (1, 1));
        assert_eq!(r.json["content"][1]["text"], "Olá, mundo «Sócrates»", "nada do pedaço que caiu se mistura à resposta");
        {
            let pedidos = s.pedidos.lock().unwrap();
            assert_eq!(pedidos.len(), 2);
            assert_eq!(pedidos[0].1, pedidos[1].1, "o pedido refeito é o mesmo");
        }
        // caiu duas vezes: erro, e não há terceira
        let s = servidor(vec![sse(&cortada), sse(&cortada)]);
        let e = enviar(&cliente().unwrap(), &opcoes(&s.url), "sk-ant-x", PEDIDO, &cab()).await.unwrap_err();
        assert!(e.contains("veio cortada") && e.contains("caiu outra vez"), "{e}");
        assert_eq!(s.pedidos.lock().unwrap().len(), 2);
        // sem a nova tentativa (quedas: 0), o erro é o de antes
        let s = servidor(vec![sse(&cortada)]);
        let mut op = opcoes(&s.url);
        op.quedas = 0;
        let e = enviar(&cliente().unwrap(), &op, "sk-ant-x", PEDIDO, &cab()).await.unwrap_err();
        assert!(e.contains("veio cortada") && !e.contains("outra vez"), "{e}");
        assert_eq!(s.pedidos.lock().unwrap().len(), 1);
    }

    #[tokio::test]
    async fn a_resposta_que_a_anthropic_interrompe_vira_erro() {
        let mut sobrecarga = conversa();
        sobrecarga.truncate(4);
        sobrecarga.push(json!({"type":"error","error":{"type":"overloaded_error","message":"Overloaded"}}));
        let s = servidor(vec![sse(&sobrecarga)]);
        let e = enviar(&cliente().unwrap(), &opcoes(&s.url), "sk-ant-x", PEDIDO, &cab()).await.unwrap_err();
        assert!(e.contains("sobrecarregada no meio da resposta"), "{e}");
        assert_eq!(s.pedidos.lock().unwrap().len(), 1, "o erro que a Anthropic manda no meio da resposta não se repete");
    }

    #[tokio::test]
    async fn a_recusa_volta_como_resposta_para_o_nucleo() {
        let eventos = vec![
            json!({"type":"message_start","message":{"id":"m","type":"message","role":"assistant","model":"claude-opus-5-5","content":[],"stop_reason":null,"usage":{"input_tokens":5,"output_tokens":0}}}),
            json!({"type":"message_delta","delta":{"stop_reason":"refusal","stop_details":{"category":"cyber"}},"usage":{"output_tokens":0}}),
            json!({"type":"message_stop"}),
        ];
        let s = servidor(vec![sse(&eventos)]);
        let r = enviar(&cliente().unwrap(), &opcoes(&s.url), "sk-ant-x", PEDIDO, &cab()).await.unwrap();
        assert_eq!(r.json["stop_reason"], "refusal");
        assert_eq!(r.json["stop_details"]["category"], "cyber");
        assert_eq!(r.json["content"], json!([]));
    }
}
