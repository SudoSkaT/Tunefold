# Checklist de revisión TPS

- [ ] La funcionalidad respeta prioridad TPS: correctitud, eficiencia, función, accesibilidad, consistencia, estética.
- [ ] No existe un componente/token reutilizable que se haya ignorado sin razón.
- [ ] Los valores y estados son honestos; no se confunde animación con dato real.
- [ ] UI no es propietaria de playback ni hace I/O bloqueante; audio permanece independiente.
- [ ] Diseño cubre ventanas, orientación, fuente grande y estados vacíos/error aplicables.
- [ ] Acciones icon-only tienen descripción y target ≥48 dp en controles nuevos.
- [ ] Contrastes semánticos y selección no dependen sólo del color.
- [ ] Motion puede reducirse y no mantiene trabajo en reposo/oculto.
- [ ] Las alegaciones de rendimiento citan mediciones comparables; de lo contrario se expresan como pendientes.
- [ ] Tests y validación elegidos cubren riesgo; evidencia y limitaciones anotadas.
- [ ] Excepciones tienen motivo, impacto, riesgo, responsable y alternativa.
- [ ] Documentación y enlaces siguen fuentes de verdad; no se introdujo dependencia no justificada.
