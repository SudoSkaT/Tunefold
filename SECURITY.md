# Reporte de vulnerabilidades

Tunefold es software libre GPL-3.0. Tomamos en serio la seguridad de los datos
locales y de las comunicaciones de red.

## Dónde reportar

**No** abras issues públicos para vulnerabilidades activas. Escribe a la
dirección de contacto del mantenedor (ver la sección Contacto del README o la
página del repositorio) y usa sujetos con prefijo `[security]`.

## Qué se espera de ti

- Descripción clara y reproducible del fallo (versión afectada, pasos,
  impacto).
- Un nivel de severidad estimado (spoilers/crash local, escalada local,
  filtración de datos, ejecución remota...).
- En caso de dependencia, número CVE o advisory conocido si existe.

## Compromiso de respuesta

- Acuse de recibo en 72 h hábiles.
- Evaluación de severidad y plan de mitigación en 7 días hábiles.
- Coordinación de divulgación: 90 días desde el arreglo antes de publicarlo,
  salvo explotación activa.

## Superficie a tener en cuenta

- El binario oficial NO incluye la feature `youtube` (red de YouTube), de modo
  que su superficie de red es LRCLIB (letras) y el reproductor local.
- Los binarios con `--features youtube` llaman a APIs no oficiales de YouTube;
  trata sus datos como **no** confidenciales.
- Los datos locales (BD, caché) no están cifrados: protege tu cuenta de usuario.