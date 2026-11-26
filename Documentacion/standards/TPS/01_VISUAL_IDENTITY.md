# 01 · Identidad visual

## Núcleo de marca

La paleta inicial se conserva como materia de marca, no como mandato de pintar todo con cinco tonos:

| Color | Rol de intención | Regla de uso |
|---|---|---|
| `#31401A` | oliva, campo | campo profundo y anclaje de identidad |
| `#BF9A54` | oro, acción | acción/marca cuando el tema lo permite |
| `#4D3A25` | madera | lugar de portada y profundidad cálida |
| `#656643` | verde grisáceo | tinta secundaria y matiz silencioso |
| `#CBBDA0` | lino | superficie/tinta legible en temas oscuros |

La implementación vigente ofrece seis temas configurables y deriva roles mediante `ThemePalette`; no reduce toda la app a la paleta de marca fija. Los temas exactos y algoritmo actual están en `ThemeSpec`/`ThemePalette` y `visual-system.md`.

## Roles y contraste

- MUST pintar mediante roles semánticos de `DesignTokens.Palette`, no hex arbitrarios en vistas.
- Texto normal: contraste mínimo 4.5:1. Texto grande y componentes/gráficos esenciales: mínimo 3:1, conforme a los tests de paleta existentes.
- El acento para formas puede alcanzar 3:1; texto sobre acento usa rol de tinta que alcance 4.5:1. No usar color como único indicador.
- `ERROR`, `POSITIVE`, `WARNING/WORKING` deben seguir siendo distinguibles del acento y entre sí; comunicar estado también con texto/forma.
- Los números anteriores son contratos de producto; su validación automatizada actual cubre combinaciones de paleta, no necesariamente capturas y todos los estados en dispositivos.

## Color de portada

`ArtworkPalette` y `ArtworkTheme` existentes son la base a conservar. El color derivado complementa la identidad, se publica sólo si cambia, y ante portada monocroma/no apta usa el acento del tema. La portada no puede cambiar el fondo base ni sacrificar legibilidad. Cambios deben conservar contrastes, estabilidad cromática y fallback probado.

No hay límite universal independiente de la paleta para saturación/luminosidad: el código actual filtra por saturación/valor y repara contraste. MUST medir contraste del rol final sobre cada superficie; SHOULD evitar animar cambios de matiz de forma abrupta. Un futuro límite perceptual requiere pruebas comparativas, no un umbral inventado.

## Composición

- La jerarquía muestra primero contenido musical, luego contexto y controles secundarios.
- Superficies se diferencian con rampa derivada de tema, espaciado y reglas; no añadiendo sombras por defecto.
- Reservar el acento dinámico para destacar portada/sonido/acción. No colorear cada fila por separado.
- Variar la composición según tarea y espacio disponible; consistencia significa reglas compartidas, no pantallas idénticas.

## Evidencia y pendientes

La paleta, roles, extracción y reglas actuales están documentados en [`visual-system.md`](../../docs/android/visual-system.md). La inspección encontró informes generales de QA, pero no se usaron como prueba del estado actual. Capturas de comparación, legibilidad con escala de fuente y TalkBack en dispositivo siguen pendientes según la propia fuente visual.
