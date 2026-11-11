# Relatório da Frente 6B — o Incognitus como módulo do Graphé Desktop

*11/11/2026. Branch `modulo`, saída de `428add1` (HEAD do main). Só commits locais: nada de push, tag ou release.*

O Incognitus passou a seguir o contrato de módulo (`GrapheDesktop/MODULOS.md`, versão 1) sem mudar o que faz:
`plugin/` (o crate `tauri-plugin-anonimizacao`), `modulo/` (a janela e o `modulo.json`) e `src-tauri/` (o app avulso,
que é o plugin mais a janela). O teste de ponta a ponta **não rodou** (o Incognitus instalado estava aberto o tempo
todo); o resto das provas está abaixo, com comando e saída.

## 1. Commits

| Commit | Tipo | O quê |
|---|---|---|
| `4eb19de` | movimentação | `git mv src modulo` — 12 arquivos, todos `R100` (0 linhas alteradas) |
| `ef2aa4f` | movimentação | `git mv` de `ferramentas.rs`, `motor.rs`, `pdf.rs`, `reid.rs` e `lib.rs` para `plugin/src/`, e de `src-tauri/recursos/fontes/` para `plugin/recursos/fontes/` — 7 arquivos, todos `R100` |
| `01375cb` | conteúdo | o plugin: `plugin/Cargo.toml`, `build.rs`, `permissions/default.toml`, as mudanças no `lib.rs` e no `motor.rs`, o `modulo/modulo.json`, o `modulo/icone.png`, o `scripts/montar.mjs` e o `.gitignore` |
| `711b84b` | conteúdo | o app avulso (`src-tauri/`) e a janela (`modulo/app.js`, `index.html`, `estilo.css`) |
| `078ab79` | conteúdo | scripts, `testes/diag_sidecars.py`, `build.yml` e README |
| (este) | conteúdo | este relatório |

Conferência das movimentações: `git diff -M100% --name-status 428add1 ef2aa4f` dá `19 R100` e
`19 files changed, 0 insertions(+), 0 deletions(-)`. Os dois commits de movimentação, sozinhos, não compilam (é o
combinado: nenhuma linha alterada).

**Dica para a junção:** juntar em duas etapas — primeiro `git merge ef2aa4f` (só renomeações: o git leva as mudanças do
main em `src/app.js`, `src/motor/…` e `src-tauri/src/*.rs` para os arquivos novos), depois `git merge modulo`. De uma vez
só, o git casa o `src-tauri/src/lib.rs` antigo com o `lib.rs` novo do app avulso, e não com o `plugin/src/lib.rs`.

### O que mudou nos trechos que a revisão da anonimização pode tocar

- `modulo/app.js`: **9 linhas**. A linha 6 (`const invoke = …`, agora com o prefixo do plugin), uma linha no começo do
  `checarApp` e três no `iniciar` (o `info` e a classe `hospedado`). Nenhuma chamada de `invoke(...)` mudou de texto: os
  nomes dos comandos são os mesmos. Nada na fila, na lista de avisos ou no `processar`.
- `modulo/index.html`: 4 linhas (a classe `so-avulso` na seção "Aplicativo" dos Ajustes e no "do aplicativo e ").
- `modulo/estilo.css`: 1 linha nova.
- `motor/` (motor.js, manifesto, .sig, casos.json, fonte/): **intocado** (`git diff --stat 428add1 HEAD -- motor
  scripts/trava.mjs scripts/assinar_motor.mjs` não lista nada).
- `scripts/extrair_motor.mjs`: 1 linha (`EMBUTIDO`: `src/motor` → `modulo/motor`).
- `testes/e2e.py`: **nenhuma linha** (ver §5). `testes/gerar_pdfs.py`: nenhuma.
- `pdf.rs`, `reid.rs`, `ferramentas.rs`: nenhuma linha. `motor.rs`: 5 linhas (os `include` de `../../src/motor/` para
  `../../modulo/motor/` e dois comentários). `atualizar.rs`: nenhuma linha, no mesmo arquivo.

## 2. Tabela dos comandos

Os nomes não mudaram; o que muda é o prefixo. Na janela, o `invoke` do `app.js` põe o prefixo sozinho.

| Antes | Agora | No conjunto `api` |
|---|---|---|
| (novo, obrigatório pelo contrato) | `plugin:anonimizacao\|info` | sim |
| `info_app` | `plugin:anonimizacao\|info_app` | |
| `preparar_arquivo` | `plugin:anonimizacao\|preparar_arquivo` | sim |
| `extrair_texto` | `plugin:anonimizacao\|extrair_texto` | sim |
| `ocr_pagina` | `plugin:anonimizacao\|ocr_pagina` | sim |
| `limpar_arquivo` | `plugin:anonimizacao\|limpar_arquivo` | sim |
| `gravar_saida` | `plugin:anonimizacao\|gravar_saida` | |
| `pasta_padrao` | `plugin:anonimizacao\|pasta_padrao` | |
| `listar_pdfs` | `plugin:anonimizacao\|listar_pdfs` | |
| `expandir_caminhos` | `plugin:anonimizacao\|expandir_caminhos` | |
| `ler_texto` | `plugin:anonimizacao\|ler_texto` | |
| `docx_partes` | `plugin:anonimizacao\|docx_partes` | |
| `gravar_reidentificado` | `plugin:anonimizacao\|gravar_reidentificado` | |
| `gravar_texto` | `plugin:anonimizacao\|gravar_texto` | |
| `revelar` | `plugin:anonimizacao\|revelar` | |
| `ajustes_ler` | `plugin:anonimizacao\|ajustes_ler` | |
| `ajustes_gravar` | `plugin:anonimizacao\|ajustes_gravar` | |
| `motor_ativo` | `plugin:anonimizacao\|motor_ativo` | sim |
| `motor_checar` | `plugin:anonimizacao\|motor_checar` | |
| `motor_baixar` | `plugin:anonimizacao\|motor_baixar` | |
| `motor_instalar` | `plugin:anonimizacao\|motor_instalar` | |
| `motor_voltar` | `plugin:anonimizacao\|motor_voltar` | |
| `registrar` | `plugin:anonimizacao\|registrar` | |
| `e2e_plano` | `plugin:anonimizacao\|e2e_plano` (só responde no app avulso) | |
| `e2e_gravar` | `plugin:anonimizacao\|e2e_gravar` (idem) | |
| `e2e_fim` | `plugin:anonimizacao\|e2e_fim` (idem) | |
| `app_checar` | `app_checar` (app avulso, sem prefixo) | |
| `app_baixar` | `app_baixar` (app avulso, sem prefixo) | |
| `app_instalar` | `app_instalar` (app avulso, sem prefixo) | |

O conjunto `default` (`anonimizacao:default`) tem os 26 comandos do plugin. O `api` (`anonimizacao:api`) tem 6. O ACL
gerado (`src-tauri/gen/schemas/acl-manifests.json`) confirma: `default: 26` e
`sets: {'api': ['allow-info', 'allow-preparar-arquivo', 'allow-extrair-texto', 'allow-ocr-pagina', 'allow-limpar-arquivo', 'allow-motor-ativo']}`.

## 3. Provas: linha de base e depois

Tudo com `CARGO_TARGET_DIR="$HOME/Library/Caches/incognitus-modulo-alvo"` e, depois do aviso de máquina,
`CARGO_BUILD_JOBS=3`, um comando cargo de cada vez. A suíte roda com `GRAPHE_DIR` apontando para a pasta do Graphé (só
leitura), porque a worktree não tem um `../Graphe` ao lado.

| Prova | Linha de base (`428add1`) | Depois (`078ab79`) |
|---|---|---|
| `npm run suite` | `RESULTADO: 106 ok, 0 falhas, de 106 (motor 1.10.1)` · `INCOGNITUS: 106 ok, 0 falhas, de 106 (motor 1.10.1, api 2)` · `PARIDADE: 106/106` · `CARIMBOS: 13 ok, 0 falhas, de 13` | **igual, linha por linha** |
| a suíte como o CI a roda (`--casos <embutido>/casos.json`, sem o Graphé) | `INCOGNITUS: 106 ok, 0 falhas` · `CARIMBOS: 13 ok` (com `src/motor/casos.json`) | igual (com `modulo/motor/casos.json`) |
| `npm run trava` | `trava: nada encontrado em motor/motor.js` | igual (rodou antes de cada um dos commits) |
| `cargo test` | `11 passed; 0 failed; 1 ignored` (um crate) | `cargo test … -p incognitus -p tauri-plugin-anonimizacao`: app `4 passed; 2 ignored`, plugin `11 passed` — **15 passed, 0 failed, 2 ignored** |
| `python3 testes/diag_sidecars.py` | `RESULTADO: sidecars ok` (256 letras no OCR) | igual |

Os 11 testes de antes continuam, com os mesmos nomes: 9 foram com os arquivos para o plugin (`motor` 2, `pdf` 3,
`reid` 4) e 2 ficaram no app (`atualizar`; o `baixa_do_portal` segue `ignored`, é de rede). Os 4 novos:

- plugin `pastas_do_avulso_e_do_hospedado`: a regra das pastas nos dois modos;
- plugin `modulo_json_confere_com_o_plugin`: `id`, `nome`, `versao`, `plugin.nome`, sidecars, os arquivos da `api`
  existem em `modulo/`, todo comando do `build.rs` está no `default`, e o `api` é exatamente os 6 da tabela;
- app `a_janela_main_fala_com_o_plugin`: o app avulso de verdade (`tauri.conf.json`, a capability e `init()`), no
  runtime de mentira do Tauri (`tauri::test`), **sem abrir janela**. Pela janela `main`: `info` (avulso, versão do
  plugin igual à do app, dados na raiz), `ajustes_gravar`/`ajustes_ler` (o `ajustes.json` na raiz dos dados, como hoje),
  `motor_ativo` (embutido, sha256 do manifesto), `e2e_plano` (nulo), `app_checar` (chega ao `atualizar.rs` e falha na
  rede: "sem conexão"), o nome antigo sem prefixo (`motor_ativo`) não existe mais, e uma janela de outro rótulo é
  recusada ("not allowed"). É a prova de que as permissões estão ligadas: erro de permissão só aparece ao chamar;
- app `hospedado_guarda_os_dados_em_modulos`: o mesmo plugin com `init_como_modulo()`: `info().hospedado` verdadeiro,
  `ajustes.json` em `<dados>/modulos/incognitus/`, a pasta temporária em `<cache>/modulos/incognitus/tmp`, e nada na
  raiz.

**Os sidecars de dentro do app.** O `lib.rs` não tem modo de diagnóstico além do `SKIA_E2E`, que não pode rodar agora.
No lugar, o teste `os_sidecars_pela_janela` (`ignored`, roda à mão) chama os comandos do plugin pela janela `main` do
app, com os sidecars e os recursos que o build do app pôs na pasta-alvo, sobre os PDFs fictícios do `gerar_pdfs.py`:

```
SKIA_TESTE_PDFS=<pasta dos PDFs fictícios> cargo test --manifest-path src-tauri/Cargo.toml -p incognitus -- --ignored os_sidecars --nocapture
ferramentas: [String("pdftotext version 25.02.0"), String("pdftoppm version 25.02.0"), String("tesseract 5.5.0")]
OCR da página 1: 256 letras
test testes::os_sidecars_pela_janela ... ok
test result: ok. 1 passed; 0 failed; 0 ignored; 0 measured; 5 filtered out; finished in 26.01s
```

Ele confere: `info_app` com as três versões, `preparar_arquivo` (a cópia em `cache/tmp/<id>/entrada.pdf`),
`extrair_texto` (3 `\f` no `texto.pdf`), `ocr_pagina` (página 1 do `escaneado.pdf`, com "BOLETIM"), `limpar_arquivo` e
a pasta temporária vazia no fim.

**O app avulso compila:** `cargo build --manifest-path src-tauri/Cargo.toml` →
`Finished dev profile [unoptimized + debuginfo] target(s) in 3m 45s`, sem aviso do compilador. O `Cargo.lock` muda só
no que a divisão pede: entra o `tauri-plugin-anonimizacao` e as dependências do motor passam do app para ele (nenhuma
versão de terceiros mudou).

**O motor embutido idêntico a `motor/`** (sha256, e `cmp` nos quatro arquivos):

```
34c2fa3b2512fea3fd3965e683399c0e1572c70889cc054bc8c219b054f1cf98  motor/motor.js
34c2fa3b2512fea3fd3965e683399c0e1572c70889cc054bc8c219b054f1cf98  modulo/motor/motor.js
b79b947dd159ea0777685830bd4ac73edc58c4ef0dd0b6f186b0b577f8258bb6  motor/manifesto.json
b79b947dd159ea0777685830bd4ac73edc58c4ef0dd0b6f186b0b577f8258bb6  modulo/motor/manifesto.json
91df88d2fea4c14ac5e8490ea118c8cd0028769a3a4c82b0563b1a213a9f4566  motor/manifesto.json.sig
91df88d2fea4c14ac5e8490ea118c8cd0028769a3a4c82b0563b1a213a9f4566  modulo/motor/manifesto.json.sig
c99b0b7a2eb3d204ecffddfa25c59dc4108df8d73731a398206cdc69b8c5b2d4  motor/casos.json
c99b0b7a2eb3d204ecffddfa25c59dc4108df8d73731a398206cdc69b8c5b2d4  modulo/motor/casos.json
```

**O extrator grava no lugar novo, igual.** Rodei o `node scripts/extrair_motor.mjs` numa **cópia** da worktree, no
rascunho da sessão (para não arriscar o `motor/` daqui), depois de apagar `modulo/motor/{motor.js,casos.json,manifesto.json}`
da cópia: `motor 1.10.1 · 389 declarações · 390 KB · sha256 34c2fa3b2512… (sem mudança)`, os quatro arquivos regravados
em `modulo/motor/` idênticos (`cmp`) aos de `motor/` desta worktree, e nenhuma pasta `src/` criada. Observação: o
`graphe.js` de hoje (sha256 `ce605012…`) já não é o do manifesto (`origem.graphe_sha256` `fca6bc7e…`), mas o motor
extraído dele sai igual, byte a byte.

**Recursos nos mesmos caminhos.** Na pasta-alvo, depois do build: `pdftotext`, `pdftoppm`, `tesseract` ao lado do
executável; `lib/` (42 bibliotecas), `tessdata/` e `LICENCAS-terceiros/` (com o `DejaVu-LICENSE.txt`, copiado de
`plugin/recursos/fontes/LICENSE_DEJAVU`: apaguei o da linha de base antes e ele voltou idêntico). Os rpaths dos binários
não foram tocados.

## 4. O que falhou e por quê

- **O teste de ponta a ponta não rodou.** `pgrep -f /Applications/Incognitus.app` achou o app aberto do começo ao fim
  (pid 49545, e outra sessão usando os sidecars dele). Não fechei, como combinado.
- **Primeira tentativa do `cargo test` não compilou:** `error: symbol _EMBED_INFO_PLIST is already defined`. Causa minha:
  o `tauri::generate_context!()` aparecia duas vezes (no `run()` e nos testes). Corrigido com uma função só,
  `contexto()`, usada pelos dois.
- **O binário de depuração da pasta-alvo combinada não acha os recursos se for aberto direto.** Não é falha do código:
  no Mac o Tauri só lê os recursos ao lado do executável quando o caminho tem `target/<perfil>`; em
  `…/incognitus-modulo-alvo/debug/` ele procura `../Resources`. Por isso a linha do e2e (§5) compila na pasta padrão.
  Os testes do app contornam isso com dois atalhos dentro da pasta-alvo (`debug/Resources → .` e os três sidecars em
  `debug/deps/`), criados pelo próprio teste.
- **Máquina:** dois comandos cargo meus rodaram ao mesmo tempo uma vez (um `cargo build` em segundo plano e o
  `cargo test` seguinte), antes do aviso. Depois dele, um de cada vez, com `CARGO_BUILD_JOBS=3` e a espera por `rustc`
  alheio (esperou 0 s).

## 5. O ponta a ponta (`testes/e2e.py`)

Conferido por leitura contra o código novo, e **não precisou de nenhuma linha**: o executável padrão continua em
`src-tauri/target/debug/incognitus`; `SKIA_E2E` e `SKIA_DADOS` são lidos pelo plugin, `SKIA_MOTOR_URL` também,
`SKIA_APP_URL` e `SKIA_DOWNLOADS` pelo app; no app avulso os dados seguem em `<SKIA_DADOS>/dados` e o cache em
`<SKIA_DADOS>/cache` (o teste usa `dados/dados/motor`, `dados/cache/tmp` e `dados/cache/fonts.conf`); `tessdata/` segue
ao lado do executável de depuração. O que executei do caminho dele sem abrir o app: os comandos pela janela (testes do
§3), o `diag_sidecars.py`, a suíte e `python3 -m py_compile testes/e2e.py`.

**Para rodá-lo (uma linha, com o Incognitus instalado FECHADO):**

```
cd /Users/claudioangelo/Documents/CODIGO/frentes.nosync/incognitus-modulo && npx tauri build --debug --no-bundle && python3 testes/e2e.py
```

Ela compila na pasta padrão (`src-tauri/target`, uns 4 GB, no disco interno), porque o binário precisa do `target/` no
caminho para achar os recursos (§4). Na worktree não há `_segredo/` nem um `../Graphe` ao lado: aí o e2e testa só a
recusa da assinatura inválida e pula o passo 5 (a comparação com o `main.py` do serviço). No checkout principal, depois
da junção, a mesma linha (sem o `cd`) testa também a instalação do motor assinado e o passo 5.

## 6. O que só o CI vai provar

- O build do **Windows** (o plugin compila lá, o `.msi`, e o e2e no app instalado). Nada do Windows foi compilado aqui.
- O build **universal do Mac** e o e2e no `Incognitus.app` empacotado: o recurso `../plugin/recursos/fontes/LICENSE_DEJAVU`
  dentro do `.dmg` e do `.msi` (em modo de desenvolvimento ele foi copiado certo), e os dois alvos escrevendo
  `plugin/permissions/autogenerated/`.
- O build de produção em si (`tauri build`, com os arquivos da janela servidos de dentro do app): aqui só houve o build
  de depuração do cargo.
- O `cargo test` no runner (os dois testes novos do app criam o atalho `Resources` em `src-tauri/target/debug`).
- No `build.yml` mudaram só duas linhas: `--casos modulo/motor/casos.json` e o `cargo test` com
  `-p incognitus -p tauri-plugin-anonimizacao`. O job `publicar`, o `needs: build` e os nomes dos artefatos ficaram.

## 7. Decisões que tomei (pelo caminho mais conservador)

1. **`info_app` ficou**, ao lado do `info` do contrato: o `info` é rápido e diz quem é o módulo; o `info_app` continua
   devolvendo `{ versao, ferramentas, plataforma }` e rodando os sidecars, como a janela espera.
2. **`pasta_padrao` e `listar_pdfs` estão no `default`**, embora o `app.js` não os chame: eram chamáveis antes, e a
   revisão da janela pode usá-los. Dá para tirá-los do `default` com duas palavras a menos no `default.toml`.
3. **O modo de teste (`SKIA_E2E`) só vale no app avulso.** Hospedado, `e2e_plano` devolve nulo e `e2e_fim` não fecha o
   app de quem hospeda. Se o Graphé Desktop quiser o e2e da janela hospedada, é uma função (`plano_do`, no `lib.rs`).
4. **A fonte do PDF foi para `plugin/recursos/fontes/`** (movimentação pura): o `include_bytes!` do `pdf.rs` continua
   valendo sem mudar, e o crate do plugin não depende de arquivo dentro de `src-tauri/`.
5. **Versão: tudo em 0.4.1.** Não é uma versão nova; quem decide o número é a sessão "Incognitus". O plugin usa a
   própria versão no produtor do PDF e no `info`, e dois testes param se a do plugin, a do `modulo.json` e a do app
   divergirem.
6. **Sem workspace do cargo.** O plugin é dependência por caminho do app e o `Cargo.lock` do app é o único; os testes do
   plugin rodam com `-p tauri-plugin-anonimizacao` pelo manifesto do app. O plugin não define `[profile]` nem `[patch]`.
7. **`SKIA_DADOS` vale nos dois modos** (hospedado: `<SKIA_DADOS>/dados/modulos/incognitus`).
8. **`plugin/permissions/autogenerated/` e `schemas/` fora do git**, como no Akoé: o `build.rs` os regrava.
9. **`recursos` do `modulo.json` iguais aos de hoje**, como pedido: `tessdata/`, `lib/`, `LICENCAS-terceiros/` e
   `LICENCAS-terceiros/DejaVu-LICENSE.txt` na raiz da pasta de recursos. O contrato pede `LICENCAS-terceiros/<id>/` só
   a módulo novo. Atenção do hospedeiro: `lib/` e `tessdata/` na raiz podem chocar com outro módulo (o script dele para
   se houver choque), e os rpaths dos sidecars (`@executable_path/../Resources/lib`) não deixam mudar a `lib/` de lugar
   sem remontá-los.
10. **`scripts/montar.mjs`** (novo, 21 linhas) é o `montar` do `modulo.json`: um comando só para o Mac e o Windows, que
    chama o `sidecars.py` com `mac` ou `windows`. O `sidecars.py` não mudou (a chave do cache do CI continua a mesma).
    **Não o executei** (baixaria o conda-forge de novo): só `node --check`.
11. **`publicar_motor.sh`**: só os três caminhos `src/motor` → `modulo/motor`. Não o executei.

## 8. O que o Graphé Desktop precisa para usar o módulo

1. **Submódulo e plugin.** `externos/incognitus` na tag do lançamento que tiver esta branch; no `Cargo.toml`,
   `tauri-plugin-anonimizacao = { path = "../externos/incognitus/plugin" }`; no builder,
   `.plugin(tauri_plugin_anonimizacao::init_como_modulo())`, com o `tauri-plugin-dialog` registrado uma vez.
2. **`modulo/modulo.json`** dá o resto ao `scripts/modulos.mjs`: a janela (`index.html`, 1040×760, mínimo 760×560, cor
   `#c8703f`), as `permissoes` da janela `modulo-incognitus` (`core:default`, `dialog:default`,
   `anonimizacao:default`), a `api`, os três sidecars (em `externos/incognitus/src-tauri/binaries/<nome>-<alvo>[.exe]`,
   depois de `node scripts/montar.mjs` na raiz do submódulo; precisa de Python e, no Windows, do `pefile`) e os
   recursos.
3. **A janela do módulo**: recebe arquivos arrastados (o padrão do Tauri, `dragDropEnabled`, tem de ficar ligado). No
   Mac o app avulso usa `titleBarStyle: Overlay` e `hiddenTitle`, e a página reserva 30 px no alto para os botões da
   janela; com barra de título comum sobra essa faixa (só estética; decidir no hospedeiro). A CSP precisa do
   `worker-src 'self' blob:` (o motor roda num Worker montado de um Blob).
4. **A `api` na janela principal** (`anonimizacao:api`, enquanto o módulo estiver instalado):

   ```js
   const P = 'plugin:anonimizacao|';
   const invoke = window.__TAURI__.core.invoke;
   // o motor em uso: o baixado e conferido (assinatura, sha256, API) vem em `js`; o embutido se lê da pasta da janela
   const ativo = await invoke(P + 'motor_ativo');   // { origem: 'embutido' | 'baixado', manifesto, js, anterior }
   const js = ativo.js || await (await fetch('modulos/incognitus/motor/motor.js')).text();
   // ativo.manifesto: { nome, versao, api: 2, data, sha256, … } — sha256 é o do motor.js
   // os autos (o que hoje é o `pdf` do nucleo/autos.mjs):
   const { id } = await invoke(P + 'preparar_arquivo', { caminho });   // cópia temporária; o original não é aberto
   const saida = await invoke(P + 'extrair_texto', { id });           // pdftotext -layout, com \f por página
   const ocr = await invoke(P + 'ocr_pagina', { id, pagina: n });     // tesseract da página n (2 de cada vez, no plugin)
   await invoke(P + 'limpar_arquivo', { id });                        // sempre no fim, com ou sem erro
   ```

   O motor carregado expõe `SkiaMotor` (`lerPdftotext`, `aplicarOcr`, `anonimizar`, …), como o `nucleo/autos.mjs` já
   usa. Os arquivos que o hospedeiro pode carregar da pasta da janela (`api.arquivos`): `motor/motor.js`,
   `motor/manifesto.json`, `motor/manifesto.json.sig`, `motor/casos.json`, `casos.js` e `worker.js` (os dois últimos
   servem para montar o mesmo Worker do Incognitus: `new Blob([js, casos.js, worker.js])`).
5. **Fora da janela** (as provas em Node, a de paridade): o motor da versão declarada é
   `externos/incognitus/modulo/motor/motor.js` (idêntico a `externos/incognitus/motor/motor.js`).
6. **Atualização das regras.** Fica na janela do módulo (Ajustes → Atualizar regras), nos dois modos; não está na
   `api`. Depois de uma atualização o plugin não avisa ninguém: o hospedeiro chama `motor_ativo` de novo a cada leitura
   de autos (é barato) para pegar o motor em uso.
7. **Estado compartilhado.** A janela do módulo e a principal falam com o mesmo plugin: a mesma pasta temporária
   (`<cache>/modulos/incognitus/tmp`, limpa ao abrir e ao sair) e a mesma fila de OCR (2 de cada vez).
8. **Choque de nomes hoje:** o Graphé Desktop ainda tem os seus `pdftotext`, `pdftoppm` e `tesseract`
   (`src-tauri/src/ferramentas.rs`). Eles saem quando o módulo entrar (PLANO §0); até lá o script acusa o choque.
9. `info` devolve `{ modulo: { id: 'incognitus', nome: 'Incognitus', versao }, hospedado, sistema, dados }`; `dados` é a
   pasta do módulo (serve ao "apagar também os dados").

## 9. O que falta

- Rodar o e2e (§5) e o CI nos dois sistemas (§6).
- **A versão nova.** A tag `v0.4.1` que existe não tem o módulo: o Graphé Desktop só pode fixar o submódulo numa tag
  depois desta branch. O número muda em `package.json`, `package-lock.json`, `src-tauri/Cargo.toml` (e o `Cargo.lock`),
  `src-tauri/tauri.conf.json`, `plugin/Cargo.toml` e `modulo/modulo.json`.
- Abrir a janela **hospedada** de verdade: aqui o modo hospedado foi provado pelo plugin (pastas e `info`), não pela
  tela. O que a janela esconde hospedada (a seção "Aplicativo" dos Ajustes, o "do aplicativo e " da opção de procurar
  atualizações, e a checagem de versão do app) foi conferido só por leitura.
- O `.odt` no Reidentificar (PLANO §0, etapa 3) não é desta frente.

## 10. O que precisa do Promotor ou da sessão "Incognitus"

- **Fechar o Incognitus instalado e rodar a linha do §5.**
- **Juntar ao main** (em duas etapas, §1), depois da suíte, do `cargo test` (agora com
  `-p incognitus -p tauri-plugin-anonimizacao`) e do e2e.
- **Decidir o número da versão** e lançar (push e tag disparam o CI; nada disso foi feito aqui).
- No checkout principal nada foi escrito. Li dele `node_modules/`, `src-tauri/binaries/` e
  `src-tauri/recursos/{mac,tessdata,LICENCAS-terceiros}` (copiados para a worktree, fora do git). O `_segredo/` não foi
  lido. Nenhum outro repositório foi alterado.
