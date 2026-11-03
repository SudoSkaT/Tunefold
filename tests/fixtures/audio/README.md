# Fixtures de audio

Archivos sintéticos (tono de 440 Hz, 1.2 s, 44.1 kHz, estéreo) generados con
FFmpeg para probar el lector AAC/ADTS. No contienen música de terceros.

| Archivo | Generación |
|---|---|
| `plain.aac` | `ffmpeg -f lavfi -i "sine=frequency=440:duration=1.2:sample_rate=44100" -ac 2 -c:a aac plain.aac` |
| `pce.aac` | igual que `plain.aac` añadiendo `-aac_pce 1` |
| `plain.m4a` | igual que `plain.aac` con extensión `.m4a` (contenedor MP4) |
| `pce.m4a` | igual que `pce.aac` con extensión `.m4a` |

`-aac_pce 1` fuerza `channel_configuration = 0` y un Program Config Element,
que es el caso que el vendor de Symphonia no soportaba.

Los tests verifican PCM decodificado, no igualdad bit a bit con una referencia:
el codec AAC es con pérdida y distintas versiones de FFmpeg pueden producir
bytes distintos.
