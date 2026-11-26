# 08 · Accesibilidad

## Requisitos MUST

- Targets táctiles ≥48×48 dp para nuevas acciones; targets mayores para acciones primarias cuando espacio lo permita. No copiar target overflow legacy de 40 dp.
- Cada control icon-only tiene contentDescription inequívoca, actualizada al cambiar la acción (por ejemplo Play/Pause) y no redundante.
- Contraste mínimo de texto 4.5:1; gráficos/controles esenciales 3:1. Estado nunca sólo por color.
- Texto respeta `scaledDensity`/escala del sistema. No recortar controles ni datos con fuentes grandes; captions no se reducen por debajo del mínimo si eso impide lectura.
- Orden TalkBack sigue jerarquía visual/acción; elementos decorativos no generan ruido; etiquetas incluyen estado y unidad cuando sea necesaria.
- Movimiento reducido ofrece equivalente estático y conserva información/función.
- Focus visible/teclado o controles de acceso compatibles cuando sean pertinentes a la superficie.
- Estados de carga/error se anuncian sin anuncios repetitivos por cada tick.

## Revisión por patrón

| Patrón | Comprobación |
|---|---|
| Transporte | nombre/estado; target; acción accesible; no depender de color |
| Progreso | valor y duración anunciados; seek sólo si soportado; no confundir visual decorativo con control |
| Lista | nombre, artista, estado/selección; filas tienen orden estable |
| Navegación | destino y seleccionado anunciados; foco no desaparece al cambiar sección |
| Carga/error/vacío | estado verbal + siguiente acción; recuperación por foco |
| Visualizador | si se excluye de TalkBack, contenido esencial (playing/silencio/estado) disponible fuera del dibujo |

## Verificación y límites actuales

Existen tests de paleta, geometría y reglas; componente visual declara contentDescription y excluye barras de accesibilidad. La fuente visual identifica TalkBack, escalas grandes y dispositivos grandes como pendientes de validación. No afirmar conformidad global sin probar árbol de accesibilidad, contraste en capturas y fuente 1.3×/1.5×/2×. Usar scripts Android existentes y revisión manual en hardware/emulador; automatizar reglas geométricas donde sea fiable.
