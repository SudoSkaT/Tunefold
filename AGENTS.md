# AGENTS.md

Guía para agentes y asistentes de código que trabajen en este repositorio.

## Contexto del proyecto

Tunefold (antes **PlayFusion**) es un reproductor musical TUI en Rust. El
repositorio pasó por una auditoría de liberación/propiedad: todo es **GPL-3.0**,
la marca fue renombrada a Tunefold, y el proveedor de YouTube vive **solo** tras
la feature de Cargo `youtube` (excluida del binario oficial por los TdS de
YouTube).

## Estructura relevante

- `src/providers/` — adaptadores externos. `youtube/` está tras
  `#[cfg(feature = "youtube")]`; `lyrics.rs` (LRCLIB) se compila **siempre**.
- `src/api/mod.rs` — punto único de composición de providers (frontera
  permitida entre providers y el resto del sistema).
- `src/infrastructure/dirs.rs` — rutas XDG; **nunca** usar `data/` en código
  nuevo (solo migración no destructiva legacy→canónico).
- `src/main.rs` — CLI de desarrollo; `--version` usa `CARGO_PKG_VERSION`.
- `vendor/rustypipe` — crate GPL-3.0 parcheado localmente (NO editar fuera de
  la justificación documentada en `Cargo.toml`).

## Reglas de hierro

1. **Respeta el rebranding**: nada de `playfusion`/`PlayFusion` en código,
   texto visible o rutas nuevas (salvo el alias legacy de entorno
   `PLAYFUSION_RANGE_WINDOW_KIB` y referencias históricas intencionales).
2. **Nada de YouTube fuera de `src/providers/youtube/`** ni de la feature
   `youtube`. El código default NO debe compilar `rustypipe` ni tocar red de
   YouTube.
3. **GPL-3.0**: no añadir dependencias no permitidas (ver `docs/policies.md`).
4. **Rutas**: los datos de usuario y la BD van por `dirs::*`; jamás rutas
   relativas al CWD.
5. **Datos del usuario**: la migración copia, nunca borra.

## Comandos de verificación obligatorios

```sh
cargo fmt --check
cargo clippy --all-targets --all-features -- -D warnings
cargo test --lib
cargo test --lib --features youtube
cargo check --offline
cargo check --offline --features youtube
```

## Notas

- Los `examples/probe_*` requieren la feature `youtube`
  (`required-features` en Cargo.toml); `bench_hotpaths` no.
- `Cargo.lock` se regenera al renombrar crates; verifica que no queden nombres
  viejos.
- La versión canónica se mantiene en `Cargo.toml` (y `--version`); las releases
  usan tags `vX.Y.Z`.
- Las políticas (privacidad, licencias, semver) están en `docs/policies.md` y
  `PRIVACY.md`; se aplican en CI.