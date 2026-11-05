#!/usr/bin/env node
// Σκιά · Skiá — assina o manifesto do motor com Ed25519.
//   node scripts/assinar_motor.mjs --criar-chave   cria o par de chaves (uma vez): a privada em _segredo/ (fora do git,
//                                                  permissão 600, nunca impressa); a pública em motor/chave_publica.txt
//                                                  (vai embutida no app)
//   node scripts/assinar_motor.mjs [pasta]         assina <pasta>/manifesto.json (padrão motor/) → manifesto.json.sig
//                                                  (base64 dos 64 bytes da assinatura sobre os bytes exatos do arquivo)
//                                                  e confere com a pública, e confere o sha256 do motor.js
import fs from 'node:fs';
import path from 'node:path';
import crypto from 'node:crypto';
import { fileURLToPath } from 'node:url';

const SKIA = path.resolve(path.dirname(fileURLToPath(import.meta.url)), '..');
const PRIVADA = path.join(SKIA, '_segredo', 'motor_ed25519.pem');
const PUBLICA = path.join(SKIA, 'motor', 'chave_publica.txt');
const parar = (m) => { console.error('PAROU: ' + m); process.exit(1); };

function publicaCrua(chavePublica) {
  // SPKI do Ed25519: 12 bytes de cabeçalho + 32 da chave
  return chavePublica.export({ type: 'spki', format: 'der' }).subarray(12);
}

if (process.argv.includes('--criar-chave')) {
  if (fs.existsSync(PRIVADA)) parar('a chave já existe em _segredo/ (não sobrescrevo)');
  const { publicKey, privateKey } = crypto.generateKeyPairSync('ed25519');
  fs.mkdirSync(path.dirname(PRIVADA), { recursive: true, mode: 0o700 });
  fs.writeFileSync(PRIVADA, privateKey.export({ type: 'pkcs8', format: 'pem' }), { mode: 0o600 });
  fs.writeFileSync(PUBLICA, publicaCrua(publicKey).toString('base64') + '\n');
  console.log('chave criada: a privada em _segredo/ (não vai ao git); a pública em motor/chave_publica.txt');
  process.exit(0);
}

const pasta = path.resolve(process.argv[2] || path.join(SKIA, 'motor'));
if (!fs.existsSync(PRIVADA)) parar('falta a chave privada em _segredo/ (rode com --criar-chave)');
const manif = fs.readFileSync(path.join(pasta, 'manifesto.json'));
const m = JSON.parse(manif.toString('utf8'));
const motor = fs.readFileSync(path.join(pasta, 'motor.js'));
if (crypto.createHash('sha256').update(motor).digest('hex') !== m.sha256) parar('o sha256 do motor.js não é o do manifesto');
const privada = crypto.createPrivateKey(fs.readFileSync(PRIVADA));
const assinatura = crypto.sign(null, manif, privada);
const publica = crypto.createPublicKey(privada);
if (publicaCrua(publica).toString('base64') !== fs.readFileSync(PUBLICA, 'utf8').trim()) parar('a chave privada não é a par da motor/chave_publica.txt');
if (!crypto.verify(null, manif, publica, assinatura)) parar('a assinatura não confere');
fs.writeFileSync(path.join(pasta, 'manifesto.json.sig'), assinatura.toString('base64') + '\n');
console.log('assinado: ' + path.relative(SKIA, path.join(pasta, 'manifesto.json.sig')) + ' (motor ' + m.versao + ', ' + m.data + ')');
