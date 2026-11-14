#[cfg(feature = "youtube")]
#[tokio::test]
async fn playlist_not_default() {
    let dir = tempfile::tempdir().unwrap();
    let p = crate::providers::youtube::YoutubeProvider::with_options(crate::providers::youtube::YoutubeOptions {
        disable_env_proxy: true,
        cache_dir: Some(dir.path().to_path_buf()),
    });
    let r = p.playlist("PL123").await;
    println!("RESULT: {:?}", r);
    assert!(r.is_err());
    // Should not be "playlist not supported"
    let msg = r.unwrap_err().to_string();
    assert!(!msg.contains("playlist not supported"), "{}", msg);
}
