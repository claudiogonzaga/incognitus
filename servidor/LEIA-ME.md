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

Sem o endereço, o porteiro procura o Ollama em `http://127.0.0.1:11434`. Se o Ollama do Mac escuta só no IP do
Tailscale (`OLLAMA_HOST=<ip>:11434`), passe esse endereço. Atenção: nesse caso o Ollama fica aberto, sem senha, a todo
aparelho da rede Tailscale, por fora do porteiro. Se nada mais depende disso, prefira `OLLAMA_HOST=127.0.0.1:11434`.

## O aplicativo na rede local (o MacBook Air M2)

No Mac servidor, com o repositório baixado, ligue a rede local no endereço em que o M2 o alcança. O do Tailscale
(`tailscale ip -4`) é o melhor: o tráfego vai cifrado e funciona fora do gabinete. Para isso, o M2 tem de entrar na mesma
rede Tailscale (mesma conta). O IP da rede do gabinete também
serve, mas sem cifra.

```bash
sudo QWEN_REDE=<ip>:11435 bash servidor/instalar_porteiro.sh
pbcopy < ~/.qwen-servidor/senha        # copia a senha sem mostrá-la
```

Se o endereço ainda não existe quando o Mac liga (o Tailscale sobe depois), o porteiro tenta de novo a cada 30 s, sem
derrubar o túnel.

No Incognitus do M2, na Minuta pelo modelo, com a IA "Qwen do servidor":

- Endereço: `http://<ip>:11435`
- Senha do servidor: cole a senha copiada (fica no chaveiro do M2)
- Modelo: `qwen3.5:9b`; contexto: 32.768

## O túnel

Uma regra no `config.yml` do cloudflared, antes da regra final `http_status:404`:

```yaml
  - hostname: qwen.<domínio>
    service: http://127.0.0.1:11436
```

E o DNS: `cloudflared tunnel route dns <túnel> qwen.<domínio>`.

## O segredo no Pages

O conteúdo de `~/.qwen-servidor/segredo` vai na variável `QWEN_SEGREDO` do projeto `consciencia`, ambiente production,
sem aparecer na tela:

```bash
npx wrangler pages secret put QWEN_SEGREDO --project-name consciencia < ~/.qwen-servidor/segredo
```

Rode o wrangler fora da pasta pessoal (`~`). De lá, ele cria `~/.wrangler`, e essa pasta esconde o login guardado em
`~/Library/Preferences/.wrangler`. A função lê o valor novo a partir da próxima publicação do portal.

## Desempenho medido (Mac M1 16 GB, qwen3.5:9b nvfp4 no motor MLX do Ollama)

- Leitura: cerca de 87 tokens/s. Escrita: cerca de 10 tokens/s. Cerca de 3,3 caracteres por token nos autos.
- O motor MLX não aceita `format: "json"` (saída estruturada). O app pede o JSON no prompt e o extrai da resposta.
- Por isso, no Qwen do servidor a Minuta pelo modelo manda até 60 mil caracteres dos autos (o começo e os atos mais
  recentes), no tamanho que cabe no contexto configurado (padrão: 32.768 tokens). Desliga o raciocínio (`think: false`)
  e põe temperatura 0,2.
