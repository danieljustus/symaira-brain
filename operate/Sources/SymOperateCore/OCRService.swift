import AppKit
import CoreGraphics
import Foundation
import Vision

public struct OCRTextRegion: Codable, Sendable {
    public let text: String
    public let frame: RectValue
    public let confidence: Float

    public init(text: String, frame: RectValue, confidence: Float) {
        self.text = text
        self.frame = frame
        self.confidence = confidence
    }
}

public struct OCRResult: Codable, Sendable {
    public let regions: [OCRTextRegion]
    public let fullText: String

    public init(regions: [OCRTextRegion], fullText: String) {
        self.regions = regions
        self.fullText = fullText
    }
}

public struct OCRService: OCRServiceProtocol {
    public init() {}

    /// Confidence threshold below which the fast pass is considered insufficient
    /// and a second `.accurate` pass is performed.
    public static let lowConfidenceThreshold: Float = 0.5

    /// Recognize text in a screenshot. Starts with `.fast` recognition; if the
    /// average candidate confidence is below `lowConfidenceThreshold`, retries
    /// with `.accurate` for better results at the cost of latency.
    public func recognizeText(in image: CGImage) -> OCRResult {
        let fastResult = runRecognition(in: image, level: .fast)
        let avgConfidence = averageConfidence(fastResult.regions)

        if avgConfidence >= Self.lowConfidenceThreshold {
            return fastResult
        }

        let accurateResult = runRecognition(in: image, level: .accurate)
        if averageConfidence(accurateResult.regions) >= avgConfidence {
            return accurateResult
        }
        return fastResult
    }

    func runRecognition(in image: CGImage, level: VNRequestTextRecognitionLevel) -> OCRResult {
        let request = VNRecognizeTextRequest()
        request.recognitionLevel = level
        request.usesLanguageCorrection = true

        let handler = VNImageRequestHandler(cgImage: image, options: [:])
        do {
            try handler.perform([request])
        } catch {
            return OCRResult(regions: [], fullText: "")
        }

        guard let observations = request.results else {
            return OCRResult(regions: [], fullText: "")
        }

        var regions: [OCRTextRegion] = []
        var fullTextParts: [String] = []

        for observation in observations {
            guard let candidate = observation.topCandidates(1).first else { continue }
            let text = candidate.string
            let confidence = candidate.confidence

            let boundingBox = observation.boundingBox
            let pixelWidth = Double(image.width)
            let pixelHeight = Double(image.height)

            let x = boundingBox.origin.x * pixelWidth
            let y = (1.0 - boundingBox.origin.y - boundingBox.height) * pixelHeight
            let width = boundingBox.width * pixelWidth
            let height = boundingBox.height * pixelHeight

            let frame = RectValue(x: x, y: y, width: width, height: height)
            regions.append(OCRTextRegion(text: text, frame: frame, confidence: confidence))
            fullTextParts.append(text)
        }

        return OCRResult(regions: regions, fullText: fullTextParts.joined(separator: "\n"))
    }

    func averageConfidence(_ regions: [OCRTextRegion]) -> Float {
        guard !regions.isEmpty else { return 0 }
        return regions.map(\.confidence).reduce(0, +) / Float(regions.count)
    }

    /// Apps with very few AX nodes likely rely on custom drawing — OCR can help.
    /// Threshold raised to 75 so that real-world apps with a modest number of
    /// accessible elements still get OCR supplementation.
    public func isAXTreeWeak(nodeCount: Int, threshold: Int = 75) -> Bool {
        nodeCount <= threshold
    }
}
