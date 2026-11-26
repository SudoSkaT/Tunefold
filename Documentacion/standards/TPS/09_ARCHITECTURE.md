# 09 · Arquitectura y propiedad de estado

## Fronteras

| Capa | Propiedad | No debe asumir |
|---|---|---|
| UI/superficies | representar estado, adaptar layout, emitir intención | ser autoridad playback, hacer I/O síncrono o interpretar PCM |
| Dominio | modelos/reglas puras | Android, widgets, proveedor concreto |
| App/orquestación | coordinar casos de uso y estado de aplicación | dibujado o DSP de bajo nivel |
| PlaybackSession/Controller + servicio | estado/acciones/ciclo de reproducción Android | ownership de estado local de cada vista |
| Infraestructura | DB, dirs, red, decoder, dispositivos | presentación |
| Analysis/DSP/visualization | mediciones/procesamiento/representación según contrato | bloquear hilo de audio o alterar control de reproducción por estética |

Para escritorio TUI, las fronteras vigentes se definen en [`engineering/ARCHITECTURE.md`](../../engineering/ARCHITECTURE.md) y `MODULE_INDEX.md`. Android Java/JNI es una app real y el módulo `platform` Rust entrega decoder/audio; no asumir que las reglas de UI TUI aplican literalmente a Views Android.

## MUST

- Consumir snapshots/versionados y filtrar callbacks tardíos por generación/época conforme al contrato existente.
- Control visual llama a acciones públicas del controller; no modifica estado interno en paralelo.
- Mantener cancelación y lifecycle; observer attach/detach simétrico.
- Audio thread nunca espera trabajo de UI. UI no tiene llamadas de red/disco sincrónicas.
- No cambiar JNI, ring, seek, reproducción, decoder o DSP como parte de una actualización visual/documental sin alcance específico y validación separada.
- Datos de usuario siguen `src/infrastructure/dirs.rs`; migraciones copian, no borran.
- Respetar aislamiento `youtube` y política/licencia de dependencias.

## Estado y concurrencia

Propiedad única; una vista puede conservar estado de presentación efímero (scroll/foco) pero no una copia autoritativa de reproducción. Trabajo async lleva cancelación y clave de identidad/generación. Locks no cruzan I/O ni callbacks externos. El pipeline audio no depende de surface activa.

## Fuente técnica

Contratos vigentes: `Documentacion/docs/architecture/playback-lifecycle.md`, `audio-engine.md`, `dsp-pipeline.md`, `engineering/decisions/0005-dsp-realtime-contract.md`, `engineering/systems/android.md`. TPS los enlaza y evita duplicar su algoritmo.
