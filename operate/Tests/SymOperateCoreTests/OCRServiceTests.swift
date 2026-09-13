import XCTest
@testable import SymOperateCore

final class OCRServiceTests: XCTestCase {

    // MARK: - isAXTreeWeak threshold tests

    func testDefaultThresholdIs75() {
        let service = OCRService()
        XCTAssertTrue(service.isAXTreeWeak(nodeCount: 0))
        XCTAssertTrue(service.isAXTreeWeak(nodeCount: 75))
        XCTAssertFalse(service.isAXTreeWeak(nodeCount: 76))
    }

    func testCustomThresholdOverridesDefault() {
        let service = OCRService()
        XCTAssertTrue(service.isAXTreeWeak(nodeCount: 5, threshold: 10))
        XCTAssertFalse(service.isAXTreeWeak(nodeCount: 11, threshold: 10))
    }

    func testSparseAXTreeIsWeak() {
        let service = OCRService()
        XCTAssertTrue(service.isAXTreeWeak(nodeCount: 3))
    }

    func testRichAXTreeIsNotWeak() {
        let service = OCRService()
        XCTAssertFalse(service.isAXTreeWeak(nodeCount: 200))
    }

    func testBoundaryNodeCountAtThreshold() {
        let service = OCRService()
        XCTAssertTrue(service.isAXTreeWeak(nodeCount: 75))
        XCTAssertFalse(service.isAXTreeWeak(nodeCount: 76))
    }

    func testZeroNodesIsWeak() {
        let service = OCRService()
        XCTAssertTrue(service.isAXTreeWeak(nodeCount: 0))
    }

    // MARK: - averageConfidence tests

    func testAverageConfidenceEmptyReturnsZero() {
        let service = OCRService()
        XCTAssertEqual(service.averageConfidence([]), 0, accuracy: 0.001)
    }

    func testAverageConfidenceSingleRegion() {
        let service = OCRService()
        let regions = [OCRTextRegion(text: "hello", frame: RectValue(x: 0, y: 0, width: 100, height: 50), confidence: 0.8)]
        XCTAssertEqual(service.averageConfidence(regions), 0.8, accuracy: 0.001)
    }

    func testAverageConfidenceMultipleRegions() {
        let service = OCRService()
        let regions = [
            OCRTextRegion(text: "a", frame: RectValue(x: 0, y: 0, width: 50, height: 50), confidence: 0.6),
            OCRTextRegion(text: "b", frame: RectValue(x: 50, y: 0, width: 50, height: 50), confidence: 0.8),
        ]
        XCTAssertEqual(service.averageConfidence(regions), 0.7, accuracy: 0.001)
    }

    // MARK: - lowConfidenceThreshold constant

    func testLowConfidenceThresholdValue() {
        XCTAssertEqual(OCRService.lowConfidenceThreshold, 0.5, accuracy: 0.001)
    }
}
