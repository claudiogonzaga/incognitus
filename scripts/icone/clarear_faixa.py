"""Clareia um pouco a faixa de sombra da logo do Incognitus e realça as letras dentro dela (16/11/2026)."""
import sys
import numpy as np
from PIL import Image, ImageFilter

ALFA, REALCE = float(sys.argv[3]), float(sys.argv[4])
im = Image.open(sys.argv[1]).convert('RGBA')
a = np.asarray(im).astype(float)
rgb, al = a[..., :3], a[..., 3]
L = 0.299 * rgb[..., 0] + 0.587 * rgb[..., 1] + 0.114 * rgb[..., 2]
H, W = L.shape
base = np.median(L[300:360, 150:870][al[300:360, 150:870] > 200])
f = np.ones(H)
for y in range(H):
    m = al[y, 150:870] > 200
    if m.sum() > 50:
        f[y] = min(1.0, np.median(L[y, 150:870][m]) / base)
# suaviza o perfil e só vale onde há sombra de verdade (a faixa do meio)
k = np.ones(15) / 15
f = np.convolve(np.pad(f, 7, mode='edge'), k, mode='valid')
f[:330] = 1; f[700:] = 1
ganho = f ** (-ALFA)
novo = rgb * ganho[:, None, None]
# realce local das letras dentro da faixa: o detalhe (a imagem menos a borrada) cresce onde a sombra é forte
peso = np.clip((1 - f) / (1 - f.min() + 1e-6), 0, 1)
img_n = Image.fromarray(np.clip(novo, 0, 255).astype(np.uint8))
borrada = np.asarray(img_n.filter(ImageFilter.GaussianBlur(10))).astype(float)
novo = borrada + (novo - borrada) * (1 + (REALCE - 1) * peso[:, None, None])
out = np.dstack([np.clip(novo, 0, 255), al]).astype(np.uint8)
Image.fromarray(out, 'RGBA').save(sys.argv[2])
