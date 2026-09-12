use camera_core::{
    CameraDriver, ConnectionConfig, DownloadRequest, MediaQuery, ThumbnailRequest, TransferPhase,
};
use mock_camera::{MockCameraConfig, MockCameraServer};
use nikon_driver::NikonZ6IiDriver;
use std::time::Duration;
use uuid::Uuid;

#[tokio::test]
async fn mock_camera_lists_pages_thumbnails_and_downloads() {
    let server = MockCameraServer::spawn(MockCameraConfig {
        asset_count: 125,
        ..MockCameraConfig::default()
    })
    .await
    .unwrap();
    let session = NikonZ6IiDriver
        .connect(ConnectionConfig {
            host: server.address().ip().to_string(),
            port: server.address().port(),
            client_name: "integration test".into(),
            client_id: Uuid::new_v4(),
            connect_timeout: Duration::from_secs(2),
            request_timeout: Duration::from_secs(5),
        })
        .await
        .unwrap();
    let first = session.list_media(MediaQuery::default()).await.unwrap();
    assert_eq!(first.items.len(), 60);
    assert_eq!(first.total_items, Some(125));
    assert!(first.next_cursor.is_some());
    let asset = first.items[0].clone();
    let thumb = session
        .load_thumbnail(ThumbnailRequest {
            asset_id: asset.id.clone(),
            max_bytes: 1024,
        })
        .await
        .unwrap();
    assert_eq!(thumb.data, vec![0xff, 0xd8, 0xff, 0xd9]);
    let directory = tempfile::tempdir().unwrap();
    let destination = directory.path().join(&asset.filename);
    let job = session
        .start_download(DownloadRequest {
            asset_id: asset.id,
            destination_path: destination.clone(),
            expected_size: asset.size_bytes,
            resume_partial: false,
            overwrite: false,
        })
        .await
        .unwrap();
    assert_eq!(job.wait().await.phase, TransferPhase::Completed);
    assert_eq!(
        tokio::fs::metadata(destination).await.unwrap().len(),
        asset.size_bytes
    );
    session.disconnect().await.unwrap();
    server.stop().await.unwrap();
}
