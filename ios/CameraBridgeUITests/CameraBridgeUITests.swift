import XCTest

final class CameraBridgeUITests: XCTestCase {
    @MainActor
    func testPhotoGridSelectionAndFormatFilter() {
        let app = XCUIApplication()
        app.launchEnvironment["CAMERABRIDGE_PREVIEW_PHOTOS"] = "1"
        app.launch()
        let jpeg = app.buttons["asset-mock-jpeg-0"]
        XCTAssertTrue(jpeg.waitForExistence(timeout: 15))
        XCTAssertFalse((jpeg.value as? String)?.contains("预览已加载") == true)
        let screenshot = XCTAttachment(screenshot: app.screenshot())
        screenshot.name = "Protocol simulation grid without sample photos"
        screenshot.lifetime = .keepAlways
        add(screenshot)
        jpeg.tap()
        XCTAssertTrue(app.buttons["下载所选"].isEnabled)
        XCTAssertTrue((jpeg.value as? String)?.contains("已选择") == true)
        app.segmentedControls.buttons["NEF"].tap()
        let nef = app.buttons["asset-mock-nef-0"]
        XCTAssertTrue(nef.waitForExistence(timeout: 10))
        XCTAssertFalse(app.buttons["asset-mock-jpeg-0"].exists)
    }

    @MainActor
    func testCameraConnectionOpensPhotoGrid() {
        let app = XCUIApplication()
        app.launch()
        let connect = app.buttons["连接相机"]
        XCTAssertTrue(connect.waitForExistence(timeout: 10))
        connect.tap()
        let browse = app.buttons["浏览照片"]
        XCTAssertTrue(browse.waitForExistence(timeout: 15))
        browse.tap()
        XCTAssertTrue(app.buttons["asset-mock-jpeg-0"].waitForExistence(timeout: 10))
    }
}
