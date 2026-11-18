# Sincronização da Mesa de trabalho entre computadores (arquitetura; ainda não implementada)

18/11/2026 — pedido do Promotor: o Incognitus dele e os da equipe precisam, no futuro, enxergar a mesma mesa
("mesmo que não seja tão perfeito quanto o Google Sheets"). A sincronização fica para depois; o formato dos dados já
nasce pronto para ela.

## O que já existe (desde 18/11/2026)

`mesa.json`, nos dados do app, guarda os casos da Mesa de trabalho:

```json
{ "versao": 1, "dispositivo": "d-3f9a…", "casos": { "<id>": {
    "id": "<uuid do caso>", "criado": "2026-11-18T12:00:00.000Z",
    "campos": { "status": { "v": "A MINUTAR", "t": "2026-11-18T12:00:00.000Z", "por": "Fulano" }, "…": {} },
    "local":  { "mapa": "/caminho/…", "docs": [ { "nome": "…", "pdf": "…", "original": "…" } ] } } } }
```

- **`id`**: um UUID por caso, criado uma vez. É por ele que dois computadores sabem que falam da mesma linha.
- **`campos`**: cada campo guarda o valor (`v`), a hora da mudança (`t`, UTC) e quem mudou (`por`). Campos de hoje: pje,
  status, requerido, autor, prazo, urgencia, proximoAto, template, feedback, feedbackIa, badges, aprendizado, modeloIa,
  dataMinuta, lapide, arquivado.
- **`local`**: os caminhos dos arquivos neste computador (o PDF anonimizado, o mapa, a minuta). **Nunca sincroniza**: cada
  computador tem os seus.

## A regra de junção (quando houver sincronização)

Por campo, vence a mudança mais nova (`t` maior; empate, o `dispositivo` maior). Não há conflito de linha inteira: se uma
pessoa muda o STATUS e outra escreve o FEEDBACK do mesmo caso, as duas mudanças ficam. Apagar é marcar `arquivado`
(o registro continua, para a outra ponta saber que foi arquivado, e não o recriar).

## O transporte: três caminhos, do mais simples ao mais completo

1. **Pasta compartilhada (recomendado para começar).** Cada computador grava só o **seu** arquivo,
   `mesa-<dispositivo>.json`, numa pasta que a equipe já sincroniza (Google Drive para computador, OneDrive, SharePoint).
   Todos leem os arquivos de todos e juntam pela regra acima. Como ninguém escreve no arquivo do outro, o Drive/OneDrive
   nunca gera "cópia em conflito". Sem servidor, sem login novo; o atraso é o do próprio Drive (segundos a minutos).
2. **Planilha do Google.** Uma aba por equipe, uma linha por caso, as colunas do Graphé. Aproveita o login do Google que o
   app já tem (precisaria da permissão de escrever em planilhas) e a equipe ainda enxerga a mesa no navegador.
3. **Rede local (Wi-Fi).** Os apps se acham na rede e trocam os registros direto. É o mais rápido, mas só funciona com os
   computadores ligados ao mesmo tempo na mesma rede, e pede cuidado com o firewall do órgão.

## O que NÃO pode ir na sincronização sem decisão do Promotor

- **O mapa** (nomes reais ↔ tokens), as minutas com nomes reais e os PDFs: hoje nunca saem do computador. Se a equipe
  precisar deles, o caminho é a própria pasta do caso compartilhada, não a mesa.
- **REQUERIDO** e os textos de feedback são dados reais do caso: só devem ir para uma pasta/planilha que a equipe já usa
  para dados de processo.
