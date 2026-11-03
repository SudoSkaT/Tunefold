# Análisis de fallos de audio

Estado: 2026-10-30. Hallazgos reproducibles y su estado; no se declara un
fallo corregido sin una prueba que lo cubra.

## 1. `aac: only 1 aac frame per adts packet is supported`

- **Estado**: obsoleto respecto al árbol actual.
- El mensaje no existe en `vendor/symphonia-codec-aac`; el vendor ya decodifica
  2–4 bloques por paquete y `cpe.rs` maneja offsets multi-frame.
- Es consistente con un binario antiguo o con una resolución de dependencias
  distinta a `[patch.crates-io]`.
- **Prueba**: `tests/audio_decode.rs::adts_multi_block_packet_realigns`.

## 2. `aac: program config`

- **Causa**: AAC con `channel_configuration = 0` describe el layout en un
  Program Config Element (PCE). El vendor lo rechazaba en ASC y en banda.
- **Corrección**:
  - parser PCE reutilizable en `vendor/symphonia-codec-aac/src/common.rs`;
  - ASC: el PCE se lee dentro de `GASpecificConfig`, después de
    `extensionFlag` (no justo después de `channelConfiguration`);
  - ADTS in-band: el decoder acepta canales desconocidos y configura el
    `AudioBuffer` al ver el primer PCE;
  - `SampleBuffer`/resampler Android se recrean si cambia `SignalSpec`.
- **Prueba**: fixtures `pce.aac` y `pce.m4a`.

## 3. Frecuencia de muestreo ASC incorrecta

- **Causa**: `M4AInfo::read_sampling_frequency` calculaba
  `(0xf << 20) & read_bits(20)`, una expresión que no es el valor de 24 bits.
- **Corrección**: `samplingFrequencyIndex == 15` lee exactamente 24 bits.
- **Riesgo residual**: índices 13/14 siguen rechazados; son reservados.

## 4. `adts: reserved sample rate`

- **Estado**: validación correcta; no se eliminó.
- Solo afecta a headers ADTS con índice de sample rate 13/14, que no son
  frecuencias válidas. Un archivo así está mal formado.

## 5. Audio truncado aceptado como completo

- **Causa**: una respuesta `200` con `Content-Length: N` podía entregar menos
  de N bytes y el transporte lo trataba como EOF.
- **Corrección**: validar `buf.len() == Content-Length`; el fallo es
  transitorio y se reintenta.
- **Prueba**:
  `media::transport::integration::truncated_200_body_is_not_accepted_as_complete_file`.
- Un `206` truncado ya se validaba contra `Content-Range`; se conserva.

## 6. Canción nueva pisada por una resolución vieja

- **Síntoma**: el usuario elige B, A termina de resolver después y arranca
  A; la UI muestra B o un error de A.
- **Causa**: `current`/cola se mutaban y el motor arrancaba sin una noción de
  intención vigente; dos resoluciones podían completar en cualquier orden.
- **Corrección**: época de intención + commit serializado en el backend, y
  generación en el motor. Una orden obsoleta devuelve
  `"reproducción reemplazada por otra orden"` y la UI la suprime.
- **Riesgo residual**: el lock de commit cubre también la persistencia de la
  escucha; es deliberado para que un commit viejo no escriba el `current.id`
  de la canción nueva.

## 7. EOF fantasma al cambiar de pista (Android)

- **Causa**: el decoder devolvía `Ok(())` tanto para fin real como para
  cancelación; la cola/autoplay lo interpretaban como fin de pista.
- **Corrección**: `DecodeOutcome::{EndOfMedia, Cancelled}`; solo el primero
  publica `track_finished`.

## 8. Stop/play bloqueados hasta 30 s (Android)

- **Causa**: `tunefold_oboe_play_stream`/`stop` hacían `join()` del hilo
  decoder mientras una lectura HTTP podía esperar el timeout de 30 s.
- **Corrección**: token de cancelación en `HttpRangeStream` (open, lecturas,
  reintentos y cuerpo) y join fuera del mutex del handle.
- **No cubierto**: si el runtime de Symphonia se bloquea fuera del transporte,
  el join puede seguir esperando; no se observó en Linux, falta hardware.

## 9. Errores de decode ignorados para siempre (Android)

- **Causa**: `Err(DecodeError(_)) => continue` sin contador; un stream roto
  quedaba en “buffering” indefinido.
- **Corrección**: contador total/consecutivo; falla tras 3 errores iniciales
  (sin PCM) u 8 consecutivos; traza los 3 primeros y cada 64.

## 10. Seek Android inseguro

- **Causa**: `tunefold_oboe_seek` limpiaba el `PcmRing` desde el hilo JNI
  mientras el productor podía estar en `push`, y no reposicionaba el decoder.
- **Corrección**: el JNI rechaza el seek y lo registra; Kotlin ya publica
  `supportsSeek() == false`. Implementarlo requiere reabrir la fuente y
  recrear el decoder, no limpiar el anillo.

## 11. `network_error: no se pudo resolver <<>> tras 2 intentos youtube --> youtube timeout`

- **Estado**: síntoma mitigado; causa raíz del `external_id` vacío no confirmada.
- `<<>>` era un identificador vacío interpolado en el mensaje. `Track::identifier()`
  ahora ignora `external_id` vacío/espacios y cae a título|artista o
  `desconocido:<id>`; `ResolveError` muestra "un track sin identidad" si aun
  así llega vacío.
- Pendiente: instrumentar el proveedor para saber por qué emitió un
  `external_id` vacío (source, fase, presencia de id; sin URLs ni tokens) y
  reproducirlo con un track concreto.

## 12. `decode error: el servidor restringe el stream ... HTTP 403 pidiendo byte 1114112`

- **Síntoma**: la reproducción se cortaba tras 64 KiB + dos ventanas de
  512 KiB (byte 1.114.112), con un 403 posicional de googlevideo.
- **Causa**: `stream_url_ok` solo ejecutaba la sonda lejana de 1,5 MiB si el
  `clen` de la URL superaba 1,6 MiB; además dependía de que la URL trajera
  `clen`. Las URLs capadas de archivos de 1,1–1,6 MiB (y las que no declaran
  `clen`) pasaban la verificación y el corte aparecía en plena reproducción.
- **Corrección**: el total se toma del `Content-Range` de la primera respuesta
  (con `clen` como respaldo) y, si supera 1 MiB, se sondea el **último byte**
  del archivo. Si la cola responde 206, cualquier ventana anterior también es
  servible; si responde 403, la URL solo entregaría un prefijo y se descarta.
- **Prueba**:
  `providers::youtube::provider::tests::verification_rejects_a_capped_tail_of_a_short_file`
  y las pruebas de aceptación de archivos completos sin `clen`.
