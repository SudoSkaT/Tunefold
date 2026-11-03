# Validación de audio

## Comandos obligatorios

```sh
cargo fmt --check
cargo clippy --all-targets --all-features -- -D warnings
cargo test --lib
cargo test --lib --features youtube
cargo check --offline
cargo check --offline --features youtube
```

Además, para el vendor y los fixtures:

```sh
cargo test --test audio_decode
cargo test --lib media::transport
```

`cargo test --lib` **no** ejecuta `tests/audio_decode.rs`; ese test es de
integración y necesita la dev-dependency `symphonia` con `aac` e `isomp4`.

## Fixtures AAC

`tests/fixtures/audio/` contiene cuatro archivos de ≈1.2 s, 44.1 kHz estéreo:

| Archivo | Qué cubre |
|---|---|
| `plain.aac` | ADTS normal, `channel_configuration=2` |
| `pce.aac` | ADTS con `channel_configuration=0` y PCE in-band |
| `plain.m4a` | MP4/ASC normal |
| `pce.m4a` | MP4/ASC con PCE dentro de `GASpecificConfig` |

Tests:

- `adts_plain_decodes`, `mp4_plain_decodes` — layout normal.
- `adts_with_program_config_decodes`, `mp4_with_program_config_decodes` —
  PCE; verifican 44.1 kHz, 2 canales y > 1 s decodificado.
- `adts_multi_block_packet_realigns` — sintetiza un paquete ADTS con dos
  `raw_data_block`; exige 2048 frames y valida el `realign()` entre bloques.
- `adts_crc_and_mpeg2_headers_decode` — headers de 9 bytes con CRC,
  `0xFFF9` (MPEG-2) y combinación de ambos.

## Transporte

`src/media/transport/integration.rs` usa `FakeServer` para:

- stream > 1 MiB en múltiples ventanas;
- `TruncateFirst` (206 corto, reintento);
- `Truncate200First` (200 con `Content-Length` completo y cuerpo corto; no se
  acepta como archivo completo);
- `IgnoreRange`, `LiveNoLength`, `ForbiddenBeyond`, `AlwaysForbidden`,
  `BadContentRange`, `Slow`;
- cancelación antes de tocar la red y durante un cuerpo lento;
- seeks cortos/largos y reutilización de ventana.

Los tests unitarios de `RangePolicy` fijan backoff exponencial, techo de 8 s,
jitter y que `Cancelled` no sea transitorio. La cancelación durante el cuerpo
usa `Scenario::SlowBody` (cabeceras + media respuesta, luego silencio).

## Android

No hay pruebas de hardware en esta auditoría. Validar en dispositivo/emulador:

1. Play/pausa/resume/stop y cambio de pista rápido durante prebuffer.
2. Stream que responde 200 truncado: debe fallar y reintentar, no reproducir
   audio incompleto.
3. Track con PCE (ADTS y MP4): 2 canales, 48 kHz en el ring.
4. `stop` durante red lenta: debe volver en menos de ~1 s, no en 30 s.
5. EOF real: avanza la cola una sola vez; stop no avanza.
6. Seek: la UI no debe ofrecerlo; si se invoca el JNI, se rechaza.
7. Logs: `DECODER_DECODE_ERROR`, `DECODER_SPEC_CHANGED`,
   `playback_session_start`, `playback_commit_superseded`.

## Criterios de aceptación

- Ninguna prueba puede depender de red real ni de YouTube.
- Un fallo de red no debe dejar la UI en “buffering” infinito.
- Un cambio de pista no debe publicar EOF/errores de la pista anterior.
- Un stop no debe ser revivido por una resolución que termine después.
- El AAC con PCE debe decodificar sin `unsupported feature`.
