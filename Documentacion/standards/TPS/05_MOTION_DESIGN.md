# 05 · Motion design

## Categorías

- **Funcional**: confirma respuesta táctil/estado, selección, navegación o progreso.
- **Informativa**: comunica actividad que no puede resumirse estáticamente (carga/estado vivo).
- **Decorativa**: aporta ambiente; menor prioridad, eliminable primero.

MUST indicar la categoría en una implementación nueva. Ninguna animación mantiene ocupados audio, hilo UI o GPU sin necesidad. Ninguna bloquea acciones ni altera el orden real de eventos.

## Tokens vigentes

Android define `DURATION_FAST=90ms`, `DURATION_BASE=150ms`, `DURATION_SLOW=260ms`; intervalos de lectura visual y posición son 50ms y 200ms, no duraciones de transición. Son valores implementados; su uso debe respetar lifecycle y preferencia de movimiento reducido.

Usar fast para feedback breve, base para cambio de control/estado y slow para transición de superficie cuando se justifique. No inventar easing diferente por vista; usar curva coherente con plataforma. La fuente actual no define tokens explícitos de curva: creación/cambio centralizado queda pendiente.

## Requisitos

- MUST respetar configuración del sistema para reducir/eliminar movimiento no esencial; proporcionar cambio de estado instantáneo equivalente.
- MUST congelar/terminar motion al salir de pantalla, pausar el audio visual donde no proceda, o destruir vista.
- SHOULD animar propiedades baratas y evitar sombras/blur/relayout en cada frame.
- MUST usar datos temporales reales para progreso/audio; nunca extrapolar amplitud como si fuera una nueva medida.
- Transición de portada puede interpolar color sólo si mantiene contraste, no bloquea extracción ni genera trabajo por frame.

## Estado actual y pendientes

La especificación vigente reporta cadencias y el código usa ticks condicionados por lifecycle. No se encontró evidencia de una política central completa de easing ni validación de todas las animaciones bajo “reducir movimiento”. Antes de ampliar motion, verificar la API de accesibilidad y añadir pruebas; no afirmar soporte global hasta entonces.
