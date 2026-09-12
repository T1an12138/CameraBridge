use camera_core::{
    CameraDriver, ConnectionConfig, DownloadRequest, MediaQuery, ThumbnailRequest, TransferPhase,
};
use mock_camera::{MockCameraConfig, MockCameraServer};
use nikon_driver::NikonZ6IiDriver;
use std::time::Duration;
use uuid::Uuid;

#[tokio::test]
async fn cancellation_drains_transaction_and_keeps_session_usable() {
    let server = MockCameraServer::spawn(MockCameraConfig {
        asset_count: 1,
        chunk_size: 64,
        response_delay: Duration::from_millis(100),
        ..MockCameraConfig::default()
    })
    .await
    .unwrap();
    let session = NikonZ6IiDriver
        .connect(ConnectionConfig {
            host: server.address().ip().to_string(),
            port: server.address().port(),
            client_name: "cancel sync test".into(),
            client_id: Uuid::new_v4(),
            connect_timeout: Duration::from_secs(2),
            request_timeout: Duration::from_secs(2),
        })
        .await
        .unwrap();
    let asset = session
        .list_media(MediaQuery::default())
        .await
        .unwrap()
        .items
        .into_iter()
        .next()
        .unwrap();
    let directory = tempfile::tempdir().unwrap();
    let destination = directory.path().join(&asset.filename);
    let job = session
        .start_download(DownloadRequest {
            asset_id: asset.id.clone(),
            destination_path: destination.clone(),
            expected_size: asset.size_bytes,
            resume_partial: false,
            overwrite: false,
        })
        .await
        .unwrap();

    tokio::time::sleep(Duration::from_millis(20)).await;
    job.cancel();
    let result = tokio::time::timeout(Duration::from_secs(1), job.wait())
        .await
        .expect("cancelled transaction should settle promptly");
    assert_eq!(result.phase, TransferPhase::Cancelled);
    assert!(!destination.exists());

    let thumbnail = session
        .load_thumbnail(ThumbnailRequest {
            asset_id: asset.id,
            max_bytes: 1024,
        })
        .await
        .expect("the next transaction must not consume stale download packets");
    assert_eq!(thumbnail.data, vec![0xff, 0xd8, 0xff, 0xd9]);

    session.disconnect().await.unwrap();
    server.stop().await.unwrap();
}
