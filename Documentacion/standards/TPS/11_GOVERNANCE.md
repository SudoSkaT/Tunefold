# 11 · Gobernanza del estándar

## Obligación

Toda funcionalidad que toque interfaz, playback visible, accesibilidad, animación, visualización o rendimiento MUST consultar TPS, identificar tokens/componentes existentes, definir estados/flujo, considerar coste y accesibilidad, seleccionar pruebas, actualizar documentación pertinente y completar checklist de PR. No es necesario modificar cada documento TPS en todo cambio.

## Excepciones

La excepción MUST aparecer en descripción de PR con: requisito/tipo, motivo, impacto, riesgo, alternativa considerada, responsable y plazo/condición de revisión si es temporal. Excepciones no pueden anular correctitud, estabilidad, límites de licencia ni seguridad de datos.

## Cambios al estándar

Modificar la fuente TPS pertinente y `CHANGELOG.md`; enlazar fuente técnica canónica en vez de copiar contratos detallados. Si cambia valor implementado, modificar código y tests apropiados además de docs. Al resolver discrepancia, dejar una sola fuente de verdad y anotar la migración. No borrar historia.

## Clasificación

**IMPLEMENTED**: presente en código; **DOCUMENTED**: requisito escrito; **VALIDATED**: prueba/evidencia actual; **PENDING**: falta una de las anteriores. Una regla documentada no implica implementación. Un test de unidad no implica evaluación de dispositivo.

## Propuestas automatizadas sin dependencia nueva

1. Ampliar tests actuales `DesignTokensTest` para superficies/componentes y fijar excepción overflow legacy; automatizar target mínimo de nuevos controles.
2. Test de paletas y de estados accesibles (contraste semántico) con herramientas actuales.
3. Escaneo estático/grep o script propio pequeño sobre literales de color/dp en Views como alerta, no bloqueo inicial: excepciones legítimas (geometría, Canvas, formatos) requieren revisión.
4. Usar árbol UI/screenshot scripts ya incluidos en `android/README.md` para bounds/foco; añadir TalkBack manual hasta que exista automatización fiable.
5. Usar Android FrameMetrics/JankStats ya disponible en SDK o Perfetto en una tarea de medición antes de añadir dependencia; no inventar componente/infra de telemetría.
6. Componentes duplicados: revisión de inventario y búsqueda de clase/rol; AST lint sólo si volumen real justifica mantenimiento.

Los detectores no encontrados hoy son propuestas futuras; no se instala herramienta en esta intervención. Color/dimensiones arbitrarios no son detectables con cero falsos positivos mediante regex, por tanto requieren revisión hasta definir allowlist.
