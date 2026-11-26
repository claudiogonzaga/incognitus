# O Qwen do servidor

A IA "Qwen do servidor" da Minuta pelo modelo (desde a 0.9.7) é o Ollama de um Mac do gabinete, sem chave. Ela é a
mesma IA nas duas etapas, escolher e preencher.

```
navegador (versão web, convidado com login do Google)
  └─ /incognitus/webapp/ia-local/api/chat   função do Pages: confere a sessão, põe o segredo (variável QWEN_SEGREDO)
       └─ túnel da Cloudflare (qwen.<domínio>)
            └─ porteiro (127.0.0.1:11436): confere o segredo, só /api/chat, /api/tags e /api/version
                 └─ Ollama (qwen3.5:9b)

aplicativo (Tauri)
  └─ o endereço configurado na Minuta pelo modelo (o Ollama direto, ou o porteiro com senha na rede local)
```

Só o texto anonimizado vai ao Qwen. O mapa nunca sai do computador de quem usa (nem do navegador, na web).

## O porteiro

`porteiro_qwen.py` usa só a biblioteca padrão do Python 3.9.

- **Túnel** (sempre ligado): escuta em `127.0.0.1:11436` e exige o cabeçalho `X-Qwen-Segredo`. O segredo fica em
  `~/.qwen-servidor/segredo` e é o mesmo da variável `QWEN_SEGREDO` do projeto do Pages.
- **Rede local** (opcional, desligada por padrão): com `QWEN_REDE=0.0.0.0:11435`, ele também escuta na rede e exige
  `Authorization: Bearer <senha>`, com a senha em `~/.qwen-servidor/senha`. No aplicativo, a senha vai em "Senha do
  servidor", na Minuta pelo modelo. Atenção: na rede local o tráfego é HTTP, sem cifra.
- O pedido é refeito só com os campos conhecidos. O modelo tem de estar instalado. O contexto vai até 65.536 tokens e a
  saída até 16.384.
- A resposta sai aos pedaços (NDJSON), com uma linha em branco a cada 10 s enquanto o Qwen lê os autos, para que o
  túnel e a função não derrubem a conexão.
- O log tem só a hora, o caminho, o código, os tamanhos e a duração. Nada do conteúdo vai para ele.

Para instalar como serviço do macOS (sobe com o Mac):

```bash
sudo bash servidor/instalar_porteiro.sh http://<ip-do-ollama>:11434
```

## O túnel

Uma regra no `config.yml` do cloudflared, antes da regra final `http_status:404`:

```yaml
  - hostname: qwen.<domínio>
    service: http://127.0.0.1:11436
```

E o DNS: `cloudflared tunnel route dns <túnel> qwen.<domínio>`.

## Desempenho medido (Mac M1 16 GB, qwen3.5:9b nvfp4 no motor MLX do Ollama)

- Leitura: cerca de 87 tokens/s. Escrita: cerca de 10 tokens/s. Cerca de 3,3 caracteres por token nos autos.
- O motor MLX não aceita `format: "json"` (saída estruturada). O app pede o JSON no prompt e o extrai da resposta.
- Por isso, no Qwen do servidor a Minuta pelo modelo manda até 60 mil caracteres dos autos (o começo e os atos mais
  recentes), no tamanho que cabe no contexto configurado (padrão: 32.768 tokens). Desliga o raciocínio (`think: false`)
  e põe temperatura 0,2.
