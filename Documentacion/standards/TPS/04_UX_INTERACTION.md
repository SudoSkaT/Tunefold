# 04 · UX e interacción

## Reglas transversales

- MUST dejar claro qué cambió después de cada acción, incluidos buffering, guardado, descarga y error.
- MUST mantener controles estables y accesibles mientras cambia el contenido.
- MUST conservar la intención vigente frente a callbacks tardíos; usar generaciones/épocas según contrato de playback.
- SHOULD mantener búsqueda, pestaña y posición de lista al volver de una superficie cuando el contexto siga vigente.
- MUST ofrecer salida/reintento para errores recuperables y prevenir doble envío de acciones asíncronas.
- Una acción sin soporte (por ejemplo seek en stream Android) no debe parecer funcional.

## Patrones de superficies

| Superficie | Prioridad de información | Estados y comportamiento requerido |
|---|---|---|
| Home | identidad/contexto → contenido recomendado → acciones | carga y vacío explican cómo empezar; error conserva navegación |
| Search | campo/consulta → resultados → estado | consulta persistente durante carga; vacío distingue sin consulta/sin resultados |
| Biblioteca | colección y filtros → filas → estado de reproducción | lista larga virtualizada; conservar scroll/selección |
| Listas/mixes | identidad → orden/contenido → acciones | acciones de fila inequívocas; errores parciales explicados |
| Now Playing | portada → título/artista → posición/visual → transporte → secundarias | detenerse/buffering/pausa no deben fingir audio actual; layout cede primero decoración |
| Mini Player | track y estado → control principal | abre reproductor al tocar superficie; no intercepta botón de play |
| Descargas | progreso/resultado → acción/reintento | estados preparing/saving/saved/failed visibles; no bloquear playback |
| Configuración | grupos y valor actual → explicación | selección actual explícita; cambios persistentes explicables |
| Notificación | track y controles esenciales | estado consistente con PlaybackSession/ForegroundPlaybackService |
| Diálogo/menú | contexto → opciones → salida | conservar foco y descartar sin mutación inesperada |

Este mapa establece jerarquía y estados, no exige una composición geométrica idéntica entre todas las pantallas.

## Feedback

Feedback visible al inicio de la interacción, antes de que termine trabajo remoto. Operaciones prolongadas muestran progreso o estado sin repintar contenido ajeno. Errores deben identificar si acción falló, qué se conservó y cómo recuperarse. Acciones difíciles de revertir requieren confirmación; play/pause y navegación no.

## Comportamiento implementado vs objetivo

Android posee Home, Search, Library, Now Playing, mini player, Settings, descargas, notificación y servicio. El baseline enumera superficies, pero no constituye una prueba exhaustiva de todos los flujos de UX. Revisión por flujo y pruebas instrumentadas de restauración de contexto siguen siendo requerimiento cuando esos flujos cambien.
