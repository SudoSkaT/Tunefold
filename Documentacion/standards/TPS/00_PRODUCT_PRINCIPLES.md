# 00 · Principios de producto

## Propósito y alcance

TuneFold combina reproductor TUI Rust y aplicación Android (Java/JNI). TPS gobierna la calidad del producto completo; las reglas de dimensiones, color y componentes visuales descritas aquí aplican a Android cuando se indique. La TUI conserva sus propios widgets y restricciones de terminal. Esta distinción evita asumir una UI multiplataforma que no existe.

## Prioridad para resolver conflictos

MUST aplicar este orden: correctitud/estabilidad → rendimiento/energía → función/usabilidad → accesibilidad → consistencia visual → expresión estética. No se acepta una regresión superior para ganar en una prioridad inferior. Los contratos de playback, DSP, JNI, datos y licencias siguen las fuentes canónicas.

## Dirección de producto

- **Retro experimental**: señales de instrumentos y equipos musicales con carácter editorial, sin imitar controles de hardware que confundan.
- **Arte cromático mutable**: color de portada como acento contextual sobre una identidad de marca estable y legible.
- **Precisión de instrumento**: acciones reconocibles, estados honestos y feedback inmediato; ninguna decoración debe ocultar datos o controles.
- La portada y la música son contenido principal. La interfaz facilita exploración y control, no compite por atención.

## Reglas

- MUST preservar significado y precisión de duración, amplitud, estado de reproducción y acciones.
- MUST mantener una única autoridad por estado. PlaybackSession/PlaybackController gobiernan reproducción Android; una vista sólo representa snapshots y envía intenciones.
- MUST diseñar estados completos: normal, cargando, vacío, error recuperable, deshabilitado y activo cuando correspondan.
- SHOULD preferir una regla local simple a una abstracción compartida sin dos usos reales.
- MUST describir alcance y evidencia antes de llamar “problema” a una percepción estética o de rendimiento.

## Mapa de estado

| Área | Estado de referencia | Fuente |
|---|---|---|
| Reproducción Android | PlaybackSession / PlaybackController + servicio foreground | `Documentacion/docs/architecture/playback-lifecycle.md` |
| Tema | UiTheme instala ThemePalette en DesignTokens | `Documentacion/docs/android/visual-system.md` |
| Portada | ArtworkPalette deriva; ArtworkTheme publica | mismas fuentes y código Android |
| Visualizador TUI | análisis real y visualización desacoplada del audio | `Documentacion/engineering/systems/visualization.md` |
| Interfaz Android | vistas Java construidas por superficies/Activity | `android/app/src/main/java/com/tunefold/app/` |

## Decisiones que requieren justificación

Desviarse de un token, introducir una dependencia, mantener estado duplicado, ejecutar trabajo en el hilo principal, añadir animación continua, modificar amplitud medida o cambiar un contrato de reproducción exige motivo, alternativa considerada, riesgo y evidencia en el PR. Los requisitos pendientes de este estándar no se deben presentar como funciones ya disponibles.
