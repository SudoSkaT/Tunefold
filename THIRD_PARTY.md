# THIRD PARTY NOTICES

Tunefold se distribuye bajo GPL-3.0 (ver `LICENSE`). Este documento detalla los
componentes de terceros que se integran o se enlazan en el proyecto y sus
licencias, incluidas las de copyleft que condicionan la distribución del binario
completo.

## Dependencias directas

| Crate | Comentario | Licencia |
|---|---|---|
| `ratatui` 0.29 | TUI | MIT |
| `crossterm` 0.28 | terminal | MIT |
| `tokio` 1 | runtime async | MIT |
| `reqwest` 0.12 | HTTP/2 (rustls) | MIT OR Apache-2.0 |
| `futures-util` 0.3 | utilidades async | MIT OR Apache-2.0 |
| `rodio` 0.22 | reproducción (backends Symphonia) | MIT OR Apache-2.0 |
| `serde` / `serde_json` 1 | serialización | MIT OR Apache-2.0 |
| `async-trait` 0.1 | trait async ergonómico | MIT OR Apache-2.0 |
| `thiserror` 2 | errores | MIT OR Apache-2.0 |
| `tracing` 0.1 | logs estructurados | MIT |
| `rustfft` 6 | FFT (análisis de audio) | MIT OR Apache-2.0 |
| `sqlx` 0.8 | SQLite (runtime rustls) | MIT OR Apache-2.0 |
| `dotenvy` 0.15 | carga `.env` | MIT |
| `chrono` 0.4 | fechas | MIT OR Apache-2.0 |
| `anyhow` 1 | errores ad-hoc | MIT OR Apache-2.0 |
| `image` 0.25 | decodificación de miniaturas | MIT OR Apache-2.0 |
| `unicode-normalization` 0.1 | normalización de texto | MIT OR Apache-2.0 |
| `sha2` 0.10 | resumenes (verificación de streams) | MIT OR Apache-2.0 |

## Copyleft en el binario completo

- **GPL-3.0 — `rustypipe` 0.11.4 (vendorizado)**: vendido como
  `vendor/rustypipe` con parches locales (deofuscación reducida, PO tokens,
  cliente VISIONOS). Solo se compila con la feature `youtube` (no está en el
  binario oficial). El código modificado hace que el binario con esa feature sea
  obra derivada GPL-3.0, condición que Tunefold acepta y propaga (GPL-3.0).
- **MPL-2.0 — `symphonia-core` / `symphonia-codec` / `symphonia-bundle-*`**:
  codecs de audio transitivos de `rodio`. MPL-2.0 es copyleft a nivel de
  archivo: se permite la vinculación sin relicenciar, pero los archivos
  modificados deben siguir disponibles con su licencia original. Las licencias
  completas van embebidas en el binario donde aplica.

## Oferta de código fuente

La fuente de `vendor/rustypipe` (parcheada) está en este repositorio. Como exige
la GPL-3.0 §6, cualquier binario que incluya la feature `youtube` debe ofrecer
este código fuente bajo los términos de la GPL-3.0.

## Regenerar este listado

Los agregados transitivos se controlan en CI con los checks de licencia
(ver `.github/workflows/ci.yml`). Para un volcado completo en local:

```sh
cargo install cargo-license   # o cargo deny
cargo license --features youtube --all-features
```

El criterio de aceptación de licencias de cada versión está documentado en
`PRIVACY.md` / `docs/policies.md` (lista verde MIT/Apache-2.0; MPL-2.0
permitida; GPL solo para `rustypipe` vendido).