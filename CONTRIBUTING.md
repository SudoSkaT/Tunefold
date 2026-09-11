# Cómo contribuir a Tunefold

Gracias por querer contribuir. Antes de abrir un PR, lee `docs/policies.md`:
CI aplica esas políticas y un PR que las viole no se fusiona sin revisión.

## Flujo de trabajo

1. **Discute antes**: para cambios grandes (nueva dependencia, cambios de
   comportamiento en red, cambios de formato de BD) abre un issue primero.
2. **Bifurca y ramifica**: `git checkout -b feat/nombre-descriptivo`.
3. **Implementa** siguiendo los estándares del repositorio (estilo de código
   existente, sin comentarios redundantes).
4. **Verifica localmente** (las políticas exigen estas tres cosas):

   ```sh
   cargo fmt --check
   cargo clippy --all-targets --all-features -- -D warnings
   cargo test --lib            # sin features
   cargo test --lib --features youtube
   ```

   (Si `vendor/rustypipe` no compila en tu plataforma, puedes omitir el último
   paso y avisarlo en el PR.)
5. **Documenta**: actualiza `README.md`, `THIRD_PARTY.md` (dependencias nuevas)
   y `docs/policies.md` si procede. `PRIVACY.md` si cambian comunicaciones de
   red.
6. **Abre el PR** describiendo el problema, la solución y cómo la verificaste.

## Informe de bugs

Incluye: versión (`tunefold --version`), plataforma/terminal, comando o acción
reproducible, y salida de log si aplica. Los issues de seguridad NO se abren
como issues públicos: ver `SECURITY.md`.

## Notas

- El código se escribe en español o inglés dentro del mismo archivo según
  dominios técnicos; respeta el idioma del archivo que toques.
- No se aceptan cambios que reintroduzcan el acceso directo a YouTube fuera de
  `src/providers/youtube/` (única frontera permitida).
- Los commits: imperativo, un solo cambio lógico (ver políticas).