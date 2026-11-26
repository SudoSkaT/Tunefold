# 10 · Calidad y aseguramiento

## Niveles

- **Unit**: reglas puras, tokens, paleta, escala, estados, cadencia.
- **Integration**: JNI/decoder/ring/service/controller, cancelación y contrato de sesión.
- **UI**: navegación, acciones, feedback, estados y restauración de contexto.
- **Accessibility**: contraste, árbol TalkBack, targets, texto ampliado, reduce motion.
- **Performance**: profiling repetible y comparativo por ruta afectada.
- **Regression**: playback, lifecycle, búsqueda/lista, errores, orientación y tema.
- **Device**: salida audible, notificación/foreground, tamaños/refresh reales y batería cuando el cambio la afecta.

No cada cambio necesita todos los niveles; el PR debe explicar qué nivel valida el riesgo. La matriz técnica canónica está en `Documentacion/engineering/VERIFICATION.md`.

## Criterio de evidencia

Resultado registra comando, plataforma/dispositivo, variante/build, resultado y límites. Tests existentes no prueban automáticamente TalkBack, render real, consumo de batería ni sonido audible. Capturas/reportes históricos no se cuentan como ejecución actual.

## Comandos de referencia observados

| Cambio | Validación existente |
|---|---|
| Rust general | `cargo fmt --check`; `cargo clippy --all-targets -- -D warnings`; `cargo test --lib` |
| DSP/análisis/visualización TUI | `cargo test --lib dsp::`; `cargo test --lib visualization::`; `cargo test --lib ui::` según cambio |
| Android Rust/JNI | `cargo test --lib --features android` |
| Android app | desde `android/`: `./gradlew --offline test` (requiere SDK/JDK local) |
| Audio dispositivo | pasos de `Documentacion/docs/testing/audio-validation.md` |
| Sólo docs | comprobar enlaces/rutas y contradicciones |

Los comandos son inventario de verificación existente, no afirmación de que se ejecutaron durante esta intervención. CI actual ejecuta fmt, clippy/test default+youtube y licencia; no ejecuta tests Gradle Android en cada CI.

## Criterios de aceptación

Correctitud sin regresión; estados completos; targets/contrastes válidos; lifecycle/cancelación; trabajo crítico medido cuando se alegue mejora; instrucción y documentación actualizadas. Si falta dispositivo, marcar `PENDING` con escenario exacto.
