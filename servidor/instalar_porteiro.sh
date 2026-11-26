#!/bin/bash
# Instala o porteiro do Qwen do servidor como serviço do macOS (LaunchDaemon: sobe com o Mac, sem precisar de login).
#   sudo bash instalar_porteiro.sh [endereço do Ollama]      (padrão: http://127.0.0.1:11434)
#   sudo QWEN_REDE=<ip>:11435 bash instalar_porteiro.sh       (também a rede local, para o aplicativo, com senha)
# O porteiro roda com o usuário que chamou o sudo, e os arquivos dele ficam em ~/.qwen-servidor (o segredo do túnel
# e a senha da rede são criados se faltarem e nunca são mostrados).
set -euo pipefail
[ "$(id -u)" = 0 ] || { echo "Rode com sudo: sudo bash $0 [endereço do Ollama]"; exit 1; }
USUARIO="${SUDO_USER:?rode com sudo a partir do seu usuário}"
CASA=$(dscl . -read "/Users/$USUARIO" NFSHomeDirectory | awk '{print $2}')
OLLAMA="${1:-http://127.0.0.1:11434}"
REDE="${QWEN_REDE:-}"
if [ -n "$REDE" ] && ! [[ "$REDE" =~ ^[0-9.]+:[0-9]+$ ]]; then echo "QWEN_REDE deve ser <ip>:<porta>, ex.: 100.64.0.5:11435"; exit 1; fi
AQUI="$(cd "$(dirname "$0")" && pwd)"
BASE="$CASA/.qwen-servidor"
ROTULO="local.qwen-servidor.porteiro"
PLIST="/Library/LaunchDaemons/$ROTULO.plist"

sudo -u "$USUARIO" mkdir -p "$BASE/logs"
chmod 700 "$BASE"
[ "$AQUI/porteiro_qwen.py" -ef "$BASE/porteiro.py" ] || sudo -u "$USUARIO" cp "$AQUI/porteiro_qwen.py" "$BASE/porteiro.py"
if [ ! -s "$BASE/segredo" ]; then
  sudo -u "$USUARIO" sh -c "umask 077; /usr/bin/python3 -c 'import secrets; print(secrets.token_urlsafe(32))' > '$BASE/segredo'"
fi
chmod 600 "$BASE/segredo"
if [ -n "$REDE" ]; then
  if [ ! -s "$BASE/senha" ]; then
    sudo -u "$USUARIO" sh -c "umask 077; /usr/bin/python3 -c 'import secrets; print(secrets.token_urlsafe(24))' > '$BASE/senha'"
  fi
  chmod 600 "$BASE/senha"
  LINHA_REDE="<key>QWEN_REDE</key><string>$REDE</string>"
else
  LINHA_REDE=""
fi

cat > "$PLIST" <<EOF
<?xml version="1.0" encoding="UTF-8"?>
<!DOCTYPE plist PUBLIC "-//Apple//DTD PLIST 1.0//EN" "http://www.apple.com/DTDs/PropertyList-1.0.dtd">
<plist version="1.0">
<dict>
  <key>Label</key><string>$ROTULO</string>
  <key>UserName</key><string>$USUARIO</string>
  <key>GroupName</key><string>staff</string>
  <key>ProgramArguments</key><array><string>/usr/bin/python3</string><string>-u</string><string>$BASE/porteiro.py</string></array>
  <key>WorkingDirectory</key><string>$BASE</string>
  <key>EnvironmentVariables</key><dict>
    <key>HOME</key><string>$CASA</string>
    <key>QWEN_OLLAMA</key><string>$OLLAMA</string>
    $LINHA_REDE
  </dict>
  <key>RunAtLoad</key><true/>
  <key>KeepAlive</key><true/>
  <key>ThrottleInterval</key><integer>10</integer>
  <key>ProcessType</key><string>Interactive</string>
  <key>StandardOutPath</key><string>$BASE/logs/porteiro.log</string>
  <key>StandardErrorPath</key><string>$BASE/logs/porteiro.log</string>
</dict>
</plist>
EOF
chmod 644 "$PLIST"; chown root:wheel "$PLIST"

# o porteiro que estiver rodando fora do serviço sai, para liberar a porta
launchctl bootout "system/$ROTULO" 2>/dev/null || true
pkill -u "$USUARIO" -f "[p]orteiro.py" 2>/dev/null || true
sleep 1
launchctl bootstrap system "$PLIST"
sleep 2
if launchctl print "system/$ROTULO" >/dev/null 2>&1 && pgrep -u "$USUARIO" -f "[p]orteiro.py" >/dev/null; then
  echo "Porteiro instalado e rodando ($ROTULO), Ollama em $OLLAMA. Log: $BASE/logs/porteiro.log"
  if [ -n "$REDE" ]; then
    echo "Rede local em http://$REDE (senha em $BASE/senha)."
    echo "Para copiar a senha sem mostrá-la: pbcopy < $BASE/senha"
  fi
else
  echo "O serviço não subiu: veja $BASE/logs/porteiro.log"; exit 1
fi
