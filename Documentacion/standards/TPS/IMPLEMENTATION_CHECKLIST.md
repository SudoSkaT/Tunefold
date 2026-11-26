# Checklist de implementación TPS

- [ ] Leí `AGENTS.md`, `Documentacion/engineering/CORE.md` y este TPS.
- [ ] Identifiqué superficie/plataforma y fuente canónica relacionada.
- [ ] Reutilicé tokens/componentes existentes; cada excepción tiene motivo.
- [ ] Definí acciones, estados normal/loading/empty/error/disabled pertinentes y recuperación.
- [ ] El estado autoritativo permanece en la capa correspondiente; callbacks async tienen cancelación/generación.
- [ ] Evalué lifecycle, allocations, trabajo en hilo principal, invalidación y visibilidad.
- [ ] Revisé targets, etiquetas, contraste, escalado de texto y movimiento reducido.
- [ ] Para audio visual, conservé semántica/timestamp/amplitud y no bloqueé reproducción.
- [ ] Añadí o actualicé tests apropiados y elegí validación de dispositivo si corresponde.
- [ ] Documenté resultados como IMPLEMENTED/DOCUMENTED/VALIDATED/PENDING.
- [ ] Actualicé documento canónico sin duplicar contrato y registré excepción si aplica.
