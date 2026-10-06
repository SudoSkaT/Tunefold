//! Tests de integración de [`HttpRangeStream`] contra el servidor fake.
//!
//! Cubren los escenarios del spec: stream continuo multi-ventana > 1 MiB
//! (obligatorio), truncado con recuperación, 403 posicional (restricción),
//! 403 total, 200 que ignora Range, Content-Range inválido, timeout y EOF.

use std::sync::atomic::AtomicUsize;
use std::sync::Arc;
use std::time::{Duration, Instant};

use super::fake_server::{FakeServer, Scenario};
use super::{HttpRangeStream, RangePolicy, TransportFailure};

/// Datos deterministas de tamaño `n` (patrón repetido verificable).
fn payload(n: usize) -> Arc<Vec<u8>> {
    Arc::new((0..n).map(|i| (i % 251) as u8).collect())
}

fn test_policy(window_kib: u64) -> RangePolicy {
    RangePolicy {
        initial_window: 32 * 1024,
        window_size: window_kib * 1024,
        max_retries: 2,
        retry_delay: Duration::from_millis(10),
        request_timeout: Duration::from_secs(5),
    }
}

async fn open(server: &FakeServer, policy: RangePolicy) -> HttpRangeStream {
    HttpRangeStream::open(reqwest::Client::new(), server.url(), Vec::new(), policy)
        .await
        .expect("apertura del stream")
}

async fn drain(stream: &mut HttpRangeStream) -> Result<Vec<u8>, TransportFailure> {
    let mut out = Vec::new();
    while let Some(chunk) = stream.next_chunk(64 * 1024).await? {
        out.extend_from_slice(&chunk);
    }
    Ok(out)
}

/// OBLIGATORIO (spec §25): archivo > 1 MiB servido en múltiples ventanas;
/// el consumidor percibe un stream continuo sin EOF prematuro.
#[tokio::test]
async fn streams_across_many_windows_over_one_mib_without_premature_eof() {
    let n = 2 * 1024 * 1024 + 777;
    let data = payload(n);
    let server = FakeServer::start(Scenario::Normal(data.clone())).await;

    let mut s = open(&server, test_policy(128)).await; // ventanas de 128 KiB
    assert_eq!(s.total(), n as u64);
    let got = drain(&mut s).await.expect("lectura completa");
    assert_eq!(got.len(), n);
    assert!(got == *data, "los bytes son idénticos al original");
    // Múltiples peticiones encadenadas: inicial + ~16 ventanas.
    assert!(server.request_count() >= n / (128 * 1024));
}

#[tokio::test]
async fn seek_to_refetches_the_requested_byte_range() {
    let data = payload(512 * 1024);
    let server = FakeServer::start(Scenario::Normal(data.clone())).await;
    let mut stream = open(&server, test_policy(64)).await;

    let first = stream.next_chunk(16).await.unwrap().unwrap();
    assert_eq!(first, data[..16]);
    stream.seek_to(128 * 1024 + 7).await.unwrap();
    let sought = stream.next_chunk(32).await.unwrap().unwrap();
    assert_eq!(sought, data[128 * 1024 + 7..128 * 1024 + 39]);
}

/// El techo posicional se reporta como `Restricted` DESPUÉS de entregar el
/// prefijo servible íntegro.
#[tokio::test]
async fn positional_restriction_serves_prefix_then_reports_restricted() {
    let threshold = 400 * 1024u64;
    let data = payload(2 * 1024 * 1024);
    let server = FakeServer::start(Scenario::ForbiddenBeyond {
        data: data.clone(),
        threshold,
    })
    .await;

    let mut s = open(&server, test_policy(128)).await;
    let mut got = Vec::new();
    loop {
        match s.next_chunk(128 * 1024).await {
            Ok(Some(chunk)) => got.extend_from_slice(&chunk),
            Ok(None) => panic!("no debió llegar EOF limpio: el techo está en {threshold}"),
            Err(e @ TransportFailure::Restricted { .. }) => {
                assert!(
                    got.len() as u64 >= threshold - 256 * 1024,
                    "el prefijo entregado ({}) cubre hasta cerca del techo",
                    got.len()
                );
                assert_eq!(
                    e.category(),
                    crate::media::FailureCategory::StreamRestricted
                );
                break;
            }
            Err(other) => panic!("fallo inesperado: {other}"),
        }
    }
    assert_eq!(got, data[..got.len()], "prefijo correcto byte a byte");
}

#[tokio::test]
async fn always_forbidden_fails_open_as_url_rejected() {
    let server = FakeServer::start(Scenario::AlwaysForbidden).await;
    let err = HttpRangeStream::open(
        reqwest::Client::new(),
        server.url(),
        Vec::new(),
        test_policy(64),
    )
    .await
    .expect_err("debe fallar la apertura");
    assert!(matches!(err, TransportFailure::UrlRejected(_)));
}

/// Un cuerpo truncado es un fallo TRANSITORIO: el reintento recupera la
/// ventana y el stream continúa hasta EOF limpio.
#[tokio::test]
async fn truncated_response_is_retried_and_stream_continues() {
    let data = payload(512 * 1024);
    let server = FakeServer::start(Scenario::TruncateFirst {
        data: data.clone(),
        count: 1,
        served: Arc::new(AtomicUsize::new(0)),
    })
    .await;

    let mut s = open(&server, test_policy(256)).await;
    let got = drain(&mut s).await.expect("el truncado se recupera");
    assert_eq!(got, *data);
    assert!(server.request_count() > 3, "hubo reintento(s)");
}

/// Servidor que ignora Range: modo monolítico desde 0 (una sola respuesta
/// cubre todo); un 200 fuera de posición sería InvalidResponse.
#[tokio::test]
async fn range_ignoring_server_is_handled_from_zero_only() {
    let data = payload(300 * 1024);
    let server = FakeServer::start(Scenario::IgnoreRange(data.clone())).await;

    let mut s = open(&server, test_policy(64)).await;
    let got = drain(&mut s).await.expect("cuerpo completo servido");
    assert_eq!(got, *data);
    assert_eq!(s.total(), data.len() as u64);
}

#[tokio::test]
async fn malformed_content_range_fails_fast_without_retries() {
    let data = payload(128 * 1024);
    let server = FakeServer::start(Scenario::BadContentRange(data)).await;
    let started = Instant::now();
    let err = HttpRangeStream::open(
        reqwest::Client::new(),
        server.url(),
        Vec::new(),
        test_policy(64),
    )
    .await
    .expect_err("206 sin CR válido debe fallar");
    assert!(
        matches!(err, TransportFailure::InvalidResponse(_)),
        "clasificación: {err}"
    );
    assert!(
        started.elapsed() < Duration::from_secs(2),
        "sin reintentos para respuesta inválida"
    );
}

#[tokio::test]
async fn slow_server_times_out_with_bounded_retries() {
    let data = payload(256 * 1024);
    let server = FakeServer::start(Scenario::Slow(Duration::from_millis(500), data.clone())).await;
    let policy = RangePolicy {
        initial_window: 32 * 1024,
        window_size: 128 * 1024,
        max_retries: 1,
        retry_delay: Duration::from_millis(10),
        request_timeout: Duration::from_millis(120),
    };
    let started = Instant::now();
    let err = HttpRangeStream::open(reqwest::Client::new(), server.url(), Vec::new(), policy)
        .await
        .expect_err("timeout esperado");
    if !matches!(err, TransportFailure::Timeout(_)) {
        panic!("clase inesperada: {err:#?}");
    }
    // Acotado: apertura (120ms×2 intentos) + backoff, no minutos.
    assert!(started.elapsed() < Duration::from_secs(5));
}

/// Ventana final parcial: un archivo cuyo tamaño no cae en borde de ventana
/// termina limpio en EOF sin error ni bloqueo.
#[tokio::test]
async fn final_partial_window_ends_cleanly() {
    let n = 100 * 1024 + 13; // no múltiplo de ninguna ventana
    let data = payload(n);
    let server = FakeServer::start(Scenario::Normal(data.clone())).await;
    let mut s = open(&server, test_policy(37)).await; // ventanas impares
    let got = drain(&mut s).await.expect("lectura completa");
    assert_eq!(got.len(), n);
    assert_eq!(got, *data);
}

/// Un `seek` que cae dentro de la ventana ya descargada se resuelve SIN
/// pedirla de nuevo: los seeks del probe de MP4 son de corto alcance y hacia
/// delante, así que repetir la descarga desperdiciaba casi toda la ventana.
#[tokio::test]
async fn seek_inside_the_downloaded_window_reuses_it_without_a_request() {
    let data = payload(1024 * 1024);
    let server = FakeServer::start(Scenario::Normal(data.clone())).await;
    // Una sola ventana grande cubre todo el archivo.
    let mut s = open(&server, test_policy(512)).await;
    let before = server.request_count();

    // La ventana inicial son 32 KiB (initial_window): todo lo que siga dentro
    // de ella debe servirse sin red.
    let target = 8 * 1024;
    s.seek_to(target).await.expect("seek dentro de la ventana");
    let got = s.next_chunk(64).await.unwrap().unwrap();
    assert_eq!(got, data[target as usize..target as usize + 64]);
    assert_eq!(
        server.request_count(),
        before,
        "un seek dentro de la ventana no debe generar peticiones"
    );

    // Un seek al final exacto no necesita red tampoco.
    s.seek_to(s.total()).await.expect("seek al final");
    assert_eq!(
        server.request_count(),
        before,
        "seek al final del archivo no necesita red"
    );
    assert!(s.next_chunk(16).await.unwrap().is_none());
}

/// Un seek FUERA de la ventana descargada sí pide una ventana nueva, y los
/// bytes entregados siguen siendo correctos.
#[tokio::test]
async fn seek_outside_the_downloaded_window_fetches_and_stays_correct() {
    let data = payload(1024 * 1024);
    let server = FakeServer::start(Scenario::Normal(data.clone())).await;
    let mut s = open(&server, test_policy(64)).await;
    let before = server.request_count();

    let target = 700 * 1024;
    s.seek_to(target).await.expect("seek fuera de la ventana");
    assert!(
        server.request_count() > before,
        "un seek fuera de la ventana sí debe pedirla"
    );
    let got = s.next_chunk(128).await.unwrap().unwrap();
    assert_eq!(got, data[target as usize..target as usize + 128]);
}

/// La reutilización no puede alterar la secuencia de bytes ni el EOF.
///
/// Reproduce el patrón real del probe de MP4 en Android: saltos cortos hacia
/// delante que caen dentro de la ventana recién descargada. Cada tramo debe
/// coincidir con el original en su offset lógico, y el recorrido debe llegar
/// al final sin bytes perdidos ni peticiones explosivas.
#[tokio::test]
async fn short_forward_seeks_preserve_byte_order_and_bound_requests() {
    let n = 1024 * 1024 + 37;
    let data = payload(n);
    let server = FakeServer::start(Scenario::Normal(data.clone())).await;
    let mut s = open(&server, test_policy(64)).await;

    let mut hops = 0usize;
    loop {
        let position = s.position() as usize;
        match s.next_chunk(24 * 1024).await.expect("chunk") {
            None => break,
            Some(chunk) => {
                assert_eq!(
                    chunk.as_slice(),
                    &data[position..position + chunk.len()],
                    "tramo descolocado en el offset lógico {position}"
                );
            }
        }
        let position = s.position() as usize;
        if position + 40 * 1024 < n {
            s.seek_to((position + 40 * 1024) as u64)
                .await
                .expect("seek corto hacia delante");
            hops += 1;
        }
    }

    assert_eq!(
        s.position() as usize,
        n,
        "el recorrido con seeks intercalados llega hasta el final"
    );
    assert!(
        s.metrics().seeks >= hops as u64,
        "los seeks se contabilizaron"
    );
    assert!(
        s.metrics().requests <= (n / (24 * 1024)) as u64 + 4,
        "la reutilización acota las peticiones: {} para {} hops",
        s.metrics().requests,
        hops
    );
}
