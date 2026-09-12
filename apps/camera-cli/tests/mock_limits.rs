use camera_core::{CameraDriver, CameraErrorCode, ConnectionConfig, MediaQuery, ThumbnailRequest};
use mock_camera::{MockCameraConfig, MockCameraServer};
use nikon_driver::NikonZ6IiDriver;
use std::time::Duration;
use uuid::Uuid;

#[tokio::test]
async fn streamed_data_is_rejected_before_exceeding_caller_limit() {
    let server = MockCameraServer::spawn(MockCameraConfig {
        asset_count: 1,
        chunk_size: 1,
        ..MockCameraConfig::default()
    })
    .await
    .unwrap();
    let session = NikonZ6IiDriver
        .connect(ConnectionConfig {
            host: server.address().ip().to_string(),
            port: server.address().port(),
            client_name: "limit test".into(),
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
    let error = session
        .load_thumbnail(ThumbnailRequest {
            asset_id: asset.id,
            max_bytes: 2,
        })
        .await
        .expect_err("four-byte thumbnail must exceed a two-byte limit");
    assert_eq!(error.code(), CameraErrorCode::Protocol);
    let _ = session.disconnect().await;
    server.stop().await.unwrap();
}
