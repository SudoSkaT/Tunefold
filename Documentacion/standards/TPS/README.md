# TuneFold Product Standard (TPS)

Versión inicial 1.0 · Estado: estándar documental vigente · Alcance: producto TuneFold, con reglas de interfaz especialmente aplicables a Android.

TPS es la fuente normativa para decisiones de producto, interfaz, interacción, movimiento, visualización, accesibilidad, rendimiento y revisión. Se complementa con fuentes técnicas canónicas; no reemplaza contratos del motor ni pretende describir como implementado lo que sólo exige.

## Cómo leerlo

- **MUST**: requisito obligatorio para nuevas implementaciones y cambios pertinentes.
- **SHOULD**: recomendado; una desviación debe tener motivo en el PR.
- **MAY**: opcional.
- **Implemented**: existe en código observado. **Documented**: especificado aquí. **Validated**: comprobado mediante la evidencia citada. **Pending**: falta implementación o comprobación.

Los documentos de identidad y diseño definen los objetivos del producto. Los valores realmente desplegados siguen viviendo en [`DesignTokens.java`](../../../android/app/src/main/java/com/tunefold/app/DesignTokens.java), [`ThemeSpec.java`](../../../android/app/src/main/java/com/tunefold/app/ThemeSpec.java), [`ThemePalette.java`](../../../android/app/src/main/java/com/tunefold/app/ThemePalette.java), [`ArtworkPalette.java`](../../../android/app/src/main/java/com/tunefold/app/ArtworkPalette.java) y [`ArtworkTheme.java`](../../../android/app/src/main/java/com/tunefold/app/ArtworkTheme.java). Si divergen, abrir un cambio TPS y corregir la fuente de intención o el código con pruebas; no reinterpretar silenciosamente los valores.

## Contenido

1. [Principios de producto](00_PRODUCT_PRINCIPLES.md)
2. [Identidad visual](01_VISUAL_IDENTITY.md)
3. [Tokens](02_DESIGN_TOKENS.md)
4. [Componentes](03_COMPONENT_SYSTEM.md)
5. [UX e interacción](04_UX_INTERACTION.md)
6. [Motion design](05_MOTION_DESIGN.md)
7. [Visualización de audio](06_AUDIO_VISUALIZATION.md)
8. [Rendimiento](07_PERFORMANCE.md)
9. [Accesibilidad](08_ACCESSIBILITY.md)
10. [Arquitectura](09_ARCHITECTURE.md)
11. [Calidad y pruebas](10_QUALITY_ASSURANCE.md)
12. [Gobernanza](11_GOVERNANCE.md)
13. [Checklist de implementación](IMPLEMENTATION_CHECKLIST.md)
14. [Checklist de revisión](REVIEW_CHECKLIST.md)
15. [Cambios](CHANGELOG.md)

## Fuentes canónicas relacionadas

- Android visual y paleta: [`Documentacion/docs/android/visual-system.md`](../../docs/android/visual-system.md).
- Arquitectura Android: [`Documentacion/engineering/systems/android.md`](../../engineering/systems/android.md), [`Documentacion/docs/architecture/playback-lifecycle.md`](../../docs/architecture/playback-lifecycle.md).
- Visualización TUI y DSP: [`Documentacion/engineering/systems/visualization.md`](../../engineering/systems/visualization.md), [`Documentacion/engineering/systems/dsp.md`](../../engineering/systems/dsp.md), [`Documentacion/docs/architecture/dsp-pipeline.md`](../../docs/architecture/dsp-pipeline.md).
- Verificación: [`Documentacion/engineering/VERIFICATION.md`](../../engineering/VERIFICATION.md) y [`Documentacion/docs/testing/audio-validation.md`](../../docs/testing/audio-validation.md).
- Política de dependencias/licencias: [`Documentacion/docs/policies.md`](../../docs/policies.md).
- Auditoría de evidencia: [`TPS_BASELINE_AUDIT.md`](TPS_BASELINE_AUDIT.md).

Los informes bajo `Documentacion/docs/audit/`, `Documentacion/docs/roadmap/` y `Documentacion/android/*_REPORT.md` son históricos; no son contrato de implementación.
