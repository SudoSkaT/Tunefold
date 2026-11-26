# 03 · Sistema de componentes

## Contrato común

Antes de crear un componente, MUST buscar equivalente en `android/app/src/main/java/com/tunefold/app/` y en [`COMPONENT_LIBRARY.md`](../../COMPONENT_LIBRARY.md). Reutilizar lógica y tokens existentes; no crear framework de UI ni componente genérico de un único uso.

Todo componente interactivo define responsabilidad, variantes, estado disabled/pressed/selected/loading/error aplicable, feedback, descripción accesible, target táctil, comportamiento con ventanas/fuentes grandes y coste de actualización. Vistas de dibujo deben reutilizar `Paint`, `Path`, buffers y geometría cuando convenga; escuchar sólo cambios pertinentes y retirar listeners al detach.

## Inventario y reglas actuales

| Familia | Implementaciones observadas | Especificación de uso |
|---|---|---|
| Transporte | `PrimaryPlaybackButton`, `SecondaryActionButton`, `IconButton`, `Icon` | acción primaria destaca; icon-only con contentDescription; target nuevo ≥48 dp |
| Iconos | `IconPath`, `IconDrawable` | rejilla 24, trazo 2; no mezclar familias sin motivo |
| Fila musical | `TrackRow`, `TrackList`, `TrackGroup` | altura/token y ventana virtualizada; separar título, atribución, duración/estado |
| Superficies | `SectionSurface`, `SectionHeader` y `*Surface` | jerarquía por padding/regla, no decoración redundante |
| Navegación | `NavigationItem`, `ShelfTabBar`, `MainActivity` | destino y selección se expresan por señal no cromática además de color |
| Reproducción | `NowPlayingView`, `MiniPlayer`, `ProgressControl`, `ProgressLineView` | playback es intención/snapshot; progreso no implica soporte de seek |
| Arte/estado | `ArtworkView`, `ArtworkTheme`, `StateDotView` | liberación/listeners correctos; fallback y estado accesibles |
| Audio visual | `LiveAudioBars`, `SoundBarsView` | lector de datos separado del dibujado; detalles en 06 |
| Búsqueda/configuración | `SearchSurface`, `SettingsSurface` | estados vacío/carga/error y recuperación claros |
| Diálogos/menús | creados por superficies | confirmar acciones destructivas o difíciles de revertir; devolver foco al cerrar |

## Reglas por componente

- Botón: una acción primaria, pressed/disabled definidos, etiqueta verbal accesible; no cambiar de función por estado sin comunicarlo.
- Progreso: indicar tiempo transcurrido/restante con formato consistente. Seek sólo si el motor lo soporta; el Android streaming actual declara `seek=off`.
- Fila: toda la fila y el menú no deben producir acciones ambiguas; conservar identidad del track al reordenar/reciclar.
- Navegación: preservar contexto cuando cambiar sección no requiera reset; estado activo no depende sólo del color.
- Búsqueda: conservar consulta durante carga, ofrecer limpiar/cancelar y error recuperable.
- Carga/vacío/error: comunicar qué ocurre y siguiente acción útil; nunca usar animación como única señal.
- Diálogo: foco legible, acción primaria explícita, cancelación disponible cuando sea segura.

## Rendimiento y lifecycle

MUST evitar I/O en UI thread y reconstrucciones completas por cambios de un valor; limitar listas grandes; cancelar trabajos y retirar observadores al perder dueño. No mantener ticks cuando la superficie no está visible. No establecer accesibilidad NO para visualización esencial sin equivalente textual (el visualizador decorativo/duplicativo puede excluirse si los controles/estado se anuncian por separado).

Las dimensiones actuales se listan en `COMPONENT_LIBRARY.md` y 02; esa biblioteca contiene algunos valores que contradicen el suelo táctil (overflow 40dp). TPS fija ≥48dp para nueva UI y registra la discrepancia en auditoría, sin afirmar que ya fue corregida.
