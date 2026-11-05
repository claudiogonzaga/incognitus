# skia-motor

As regras de anonimização que o **Σκιά · Skiá** usa: o motor de anonimização de autos do Graphé, extraído para rodar
sozinho, no computador de quem anonimiza. Este repositório só publica as regras; os autos nunca passam por aqui.

| arquivo | o que é |
|---|---|
| `motor.js` | o motor (sem comentários, sem dado de pessoa, sem ID, sem prompt — o build para se achar algo assim) |
| `manifesto.json` | versão do motor, data, sha256 do `motor.js` e dos casos de teste |
| `manifesto.json.sig` | assinatura Ed25519 do `manifesto.json` (base64) |
| `casos.json` | os casos de teste (dados 100% fictícios) que o motor tem de passar |
| `chave_publica.txt` | a chave pública que confere a assinatura (a mesma que vai dentro do app) |

O Skiá só troca de motor se a assinatura confere com a chave embutida no app, se o sha256 do `motor.js` é o do
manifesto e se o motor novo passa em todos os casos de teste. A versão anterior fica guardada no computador para voltar.

Conferir à mão (Node 18+):

```bash
node -e "const c=require('crypto'),f=require('fs');const k=c.createPublicKey({key:Buffer.concat([Buffer.from('302a300506032b6570032100','hex'),Buffer.from(f.readFileSync('chave_publica.txt','utf8').trim(),'base64')]),format:'der',type:'spki'});console.log(c.verify(null,f.readFileSync('manifesto.json'),k,Buffer.from(f.readFileSync('manifesto.json.sig','utf8').trim(),'base64'))?'assinatura ok':'ASSINATURA NÃO CONFERE')"
```
