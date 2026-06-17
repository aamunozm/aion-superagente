// face-probe — helper nativo de AION para reconocimiento facial (Anillo cámara).
//
// Captura UN frame de la cámara por defecto, detecta rostros con Apple Vision y genera un
// "faceprint" (VNGenerateImageFeaturePrintRequest) por cara. Imprime JSON en stdout:
//   {"faces":[{"embedding":[...float...],"bbox":[x,y,w,h]}], "error": null}
// El lado Rust (faces::scan) decide a QUIÉN pertenece cada faceprint. Sin red, sin modelo externo.
//
// Privacidad: solo se ejecuta bajo demanda y con permiso (gobernanza Camera + TCC de macOS).

import AVFoundation
import Vision
import CoreImage
import CoreGraphics
import Foundation

func emit(_ obj: [String: Any]) {
    if let data = try? JSONSerialization.data(withJSONObject: obj),
       let s = String(data: data, encoding: .utf8) {
        print(s)
    }
    fflush(stdout)
}

// Redimensiona una cara a 112×112 y devuelve sus bytes RGB (37632) en base64.
// Es lo que come ArcFace (faceprint potente); el lado Rust lo normaliza a [-1,1] NCHW.
func crop112(_ img: CGImage) -> String? {
    let n = 112
    let space = CGColorSpaceCreateDeviceRGB()
    guard let ctx = CGContext(
        data: nil, width: n, height: n, bitsPerComponent: 8, bytesPerRow: n * 4,
        space: space, bitmapInfo: CGImageAlphaInfo.premultipliedLast.rawValue
    ) else { return nil }
    ctx.interpolationQuality = .high
    ctx.draw(img, in: CGRect(x: 0, y: 0, width: n, height: n))
    guard let px = ctx.data else { return nil }
    let p = px.bindMemory(to: UInt8.self, capacity: n * n * 4)
    var rgb = Data(count: n * n * 3)
    rgb.withUnsafeMutableBytes { (raw: UnsafeMutableRawBufferPointer) in
        let out = raw.bindMemory(to: UInt8.self)
        for i in 0..<(n * n) {
            out[i * 3 + 0] = p[i * 4 + 0]
            out[i * 3 + 1] = p[i * 4 + 1]
            out[i * 3 + 2] = p[i * 4 + 2]
        }
    }
    return rgb.base64EncodedString()
}

final class Grabber: NSObject, AVCaptureVideoDataOutputSampleBufferDelegate {
    let sem = DispatchSemaphore(value: 0)
    var image: CGImage?
    private var done = false
    func captureOutput(_ output: AVCaptureOutput, didOutput sampleBuffer: CMSampleBuffer,
                       from connection: AVCaptureConnection) {
        if done { return }
        guard let pb = CMSampleBufferGetImageBuffer(sampleBuffer) else { return }
        let ci = CIImage(cvPixelBuffer: pb)
        if let cg = CIContext().createCGImage(ci, from: ci.extent) {
            image = cg; done = true; sem.signal()
        }
    }
}

// 1) Capturar un frame.
let session = AVCaptureSession()
session.sessionPreset = .high
guard let device = AVCaptureDevice.default(for: .video),
      let input = try? AVCaptureDeviceInput(device: device),
      session.canAddInput(input) else {
    emit(["faces": [], "error": "sin cámara o sin acceso"]); exit(0)
}
session.addInput(input)
let out = AVCaptureVideoDataOutput()
out.videoSettings = [kCVPixelBufferPixelFormatTypeKey as String: kCVPixelFormatType_32BGRA]
let grab = Grabber()
out.setSampleBufferDelegate(grab, queue: DispatchQueue(label: "aion.face.cap"))
guard session.canAddOutput(out) else { emit(["faces": [], "error": "sin salida"]); exit(0) }
session.addOutput(out)
session.startRunning()
let got = grab.sem.wait(timeout: .now() + 4)
session.stopRunning()
guard got == .success, let cg = grab.image else {
    emit(["faces": [], "error": "no llegó frame (¿permiso de cámara denegado?)"]); exit(0)
}

// 2) Detectar rostros y, por cada uno, generar el faceprint.
let W = CGFloat(cg.width), H = CGFloat(cg.height)
let handler = VNImageRequestHandler(cgImage: cg, options: [:])
let faceReq = VNDetectFaceRectanglesRequest()
try? handler.perform([faceReq])

var faces: [[String: Any]] = []
for obs in (faceReq.results ?? []) {
    let bb = obs.boundingBox  // normalizado, origen abajo-izquierda
    let rx = bb.origin.x * W
    let ry = (1 - bb.origin.y - bb.height) * H
    let rw = bb.width * W
    let rh = bb.height * H
    let rect = CGRect(x: rx, y: ry, width: rw, height: rh).integral
    guard rw > 20, rh > 20, let faceCG = cg.cropping(to: rect) else { continue }

    let fpHandler = VNImageRequestHandler(cgImage: faceCG, options: [:])
    let fpReq = VNGenerateImageFeaturePrintRequest()
    try? fpHandler.perform([fpReq])
    guard let fp = fpReq.results?.first as? VNFeaturePrintObservation else { continue }

    var emb = [Float](repeating: 0, count: fp.elementCount)
    if fp.elementType == .float {
        fp.data.withUnsafeBytes { (raw: UnsafeRawBufferPointer) in
            let p = raw.bindMemory(to: Float.self)
            for i in 0..<min(fp.elementCount, p.count) { emb[i] = p[i] }
        }
    }
    var face: [String: Any] = ["embedding": emb, "bbox": [Double(rx), Double(ry), Double(rw), Double(rh)]]
    if let c = crop112(faceCG) { face["crop112"] = c }
    faces.append(face)
}
emit(["faces": faces, "error": NSNull()])
exit(0)
