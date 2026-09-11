# Políticas de privacidad de Tunefold

## Qué envía Tunefold y a quién

Tunefold es un cliente de terminal local: no tiene servidor propio, no recopila
telemetría ni exige cuenta. Los datos que generas (biblioteca, historial,
preferencias de UI) viven en tu máquina:

| Dato | Ubicación canónica | Tipo |
|---|---|---|
| Base de datos (biblioteca/historial/preferencias) | `~/.local/share/tunefold/music.db` | Datos de usuario |
| Caché de miniaturas | `~/.cache/tunefold/thumbnails/` | Caché |
| Caché de búsquedas/reportes de la feature YouTube | `~/.cache/tunefold/rustypipe/` | Caché |
| Configuración (`.env`) | `~/.config/tunefold/.env` | Config |
| Logs/estado | `~/.local/state/tunefold/` | Reciclable |

Salvo lo indicado abajo, Tunefold no conserva ni transmite identificadores: el
historial y las búsquedas quedan en tu disco.

## Comunicaciones de red

- **YouTube / YouTube Music** (solo binarios compilados con `--features
  youtube`): búsqueda, reproducción y recomendaciones usan las APIs no oficiales
  de YouTube. Tus búsquedas y `visitor_data` van a los servidores de Google, que
  aplican sus propios términos de servicio y políticas de privacidad.
- **LRCLIB** (`lrclib.net`): letras sincronizadas. Se envía el título/artista
  (y duración) de la canción en reproducción, identificado con el User-Agent
  `Tunefold/<versión>`. LRCLIB es un servicio ajeno; consulta su política de
  privacidad.
- **Actualizaciones** (`tunefold update`): solo si se ejecuta el comando
  explícito, consulta las releases públicas del repositorio oficial.

## Conservación y borrado

- La caché (`~/.cache/tunefold/`) puede borrarse en cualquier momento sin
  pérdida funcional: `rm -rf ~/.cache/tunefold`.
- Migración desde versiones previas: Tunefold **copia** los datos legacy
  (`data/music.db`) a su destino canónico, nunca los borra.
- No hay tracking, analíticas ni "llamadas a casa" desde el binario oficial.

## Cambios

Esta política se revisa en cada release; los cambios relevantes se notifican en
el changelog de la release.