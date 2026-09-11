# Políticas del proyecto

Este documento define las políticas que CI aplica y que todo cambio debe
respetar. Cambiarlas exige revisión explícita en el PR.

## Versiones (semver)

`MAJOR.MINOR.PATCH`:

- **MAJOR**: cambios incompatibles (CLI, formato de BD, flags de API).
- **MINOR**: funcionalidad nueva compatible; detrás de feature flags si toca
  red.
- **PATCH**: correcciones, sin contratos rotos.

El flag `--version` y el `CARGO_PKG_VERSION` son la única fuente de verdad; los
tags de release usan el prefijo `v` (`v1.5.4`).

## Licencias

Todo el código de Tunefold es **GPL-3.0** (el binario oficial también, por el
componente GPL-3.0 derivado de rustypipe).

- **GPL-3.0**: permitida. Únicamente para `rustypipe` vendido (`vendor/`).
- **MIT / Apache-2.0 / MPL-2.0**: permitidas en dependencias nuevas.
- **Copyleft fuerte distinto de GPL-3.0**: requerirá justificación y revisión
  manual antes de aceptarse.
- **Dependencias sin licencia o de licencia desconocida**: rechazadas.
- `THIRD_PARTY.md` y `NOTICE` se actualizan con cada alta de dependencia.

CI ejecuta el check de licencias de cada PR (ver `.github/workflows/ci.yml`).

## Dependencias

1. No se añaden dependencias por conveniencia; cada crate nuevo se justifica en
   el PR (qué problema resuelve y por qué no se resuelve con lo existente).
2. Preferencia por yokes pequeños, con mantenimiento activo y superficie de
   red mínima (rustls por defecto; sin OpenSSL).
3. Las dependencias que satisfagan estos criterios entran en la lista verde
   (MIT/Apache-2.0) sin revisión adicional; el resto pasa a revisión manual.

## Seguridad

- El binario oficial no acepta parámetros de red arbitrarios ni ejecuta código
  remoto; las URLs que se reproducen se resuelven vía el sistema de reproducción
  local (nodio por defecto).
- Los secretos van en el entorno/`.env` (que está en `.gitignore`); nunca se
  comprometen en el repositorio. Ver `SECURITY.md` para el proceso de reporte.

## Privacidad

Ver `PRIVACY.md`: Tunefold es local-first; las comunicaciones de red se limitan
a los servicios documentados (YouTube si se compila la feature, LRCLIB para
letras).

## Estándares de código

- `cargo fmt --check` y `cargo clippy --all-targets --all-features -- -D
  warnings` deben pasar (ambas configuraciones: por defecto y con `youtube`).
- Los tests del lib (`cargo test --lib`) corren en ambas configuraciones.
- Feature flags nuevos se declaran en `Cargo.toml` y se documentan en el README.

## Commits

Mensajes imperativos en inglés (estilo convencional), p. ej. `feat: add X`,
`fix: correct Y`, `chore: ...`. Un commit = un cambio lógico; se referencia el
motivo si es no obvio.