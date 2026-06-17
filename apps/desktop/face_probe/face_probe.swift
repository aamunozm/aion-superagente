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

// Convierte un CGImage a JPEG en base64, redimensionado para que pese poco (la foto va al CHAT,
// no necesita resolución de cámara). Usa CoreGraphics (escalado) + CoreImage (codificación JPEG).
func jpegBase64(_ cg: CGImage, maxDim: Int = 260) -> String? {
    let w = cg.width, h = cg.height
    let scale = min(1.0, Double(maxDim) / Double(max(w, h)))
    let tw = max(1, Int(Double(w) * scale)), th = max(1, Int(Double(h) * scale))
    let space = CGColorSpaceCreateDeviceRGB()
    guard let ctx = CGContext(data: nil, width: tw, height: th, bitsPerComponent: 8,
                              bytesPerRow: 0, space: space,
                              bitmapInfo: CGImageAlphaInfo.noneSkipLast.rawValue) else { return nil }
    ctx.interpolationQuality = .high
    ctx.draw(cg, in: CGRect(x: 0, y: 0, width: tw, height: th))
    guard let scaled = ctx.makeImage() else { return nil }
    let ci = CIImage(cgImage: scaled)
    guard let data = CIContext().jpegRepresentation(of: ci, colorSpace: space, options: [:])
    else { return nil }
    return data.base64EncodedString()
}

// Transformación de SIMILITUD (escala + rotación + traslación) que mapea src→dst por mínimos
// cuadrados (Procrustes 2D, forma cerrada). Es lo que alinea la cara a la plantilla de ArcFace.
func similarityTransform(_ src: [CGPoint], _ dst: [CGPoint]) -> CGAffineTransform? {
    let n = CGFloat(src.count)
    guard src.count >= 2, src.count == dst.count else { return nil }
    var msx: CGFloat = 0, msy: CGFloat = 0, mdx: CGFloat = 0, mdy: CGFloat = 0
    for i in 0..<src.count { msx += src[i].x; msy += src[i].y; mdx += dst[i].x; mdy += dst[i].y }
    msx /= n; msy /= n; mdx /= n; mdy /= n
    var sxx: CGFloat = 0, a: CGFloat = 0, b: CGFloat = 0
    for i in 0..<src.count {
        let sx = src[i].x - msx, sy = src[i].y - msy
        let dx = dst[i].x - mdx, dy = dst[i].y - mdy
        sxx += sx * sx + sy * sy
        a += sx * dx + sy * dy
        b += sx * dy - sy * dx
    }
    guard sxx > 1e-6 else { return nil }
    let c = a / sxx, s = b / sxx // c = escala·cosθ, s = escala·sinθ
    let tx = mdx - (c * msx - s * msy)
    let ty = mdy - (s * msx + c * msy)
    return CGAffineTransform(a: c, b: s, c: -s, d: c, tx: tx, ty: ty)
}

// Aplica la transformación al frame y produce la cara alineada 112×112. Devuelve los bytes RGB
// (base64, lo que come ArcFace vía crop112) y un JPEG del recorte alineado (para verificación).
func alignedFace(_ cg: CGImage, _ W: CGFloat, _ H: CGFloat, _ t: CGAffineTransform)
    -> (rgb: String, jpeg: String?)?
{
    let n = 112
    let space = CGColorSpaceCreateDeviceRGB()
    guard let ctx = CGContext(data: nil, width: n, height: n, bitsPerComponent: 8,
                              bytesPerRow: n * 4, space: space,
                              bitmapInfo: CGImageAlphaInfo.premultipliedLast.rawValue) else { return nil }
    ctx.interpolationQuality = .high
    ctx.concatenate(t) // coords de imagen (origen abajo-izq) → lienzo 112 (origen abajo-izq)
    ctx.draw(cg, in: CGRect(x: 0, y: 0, width: W, height: H))
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
    let jpeg = ctx.makeImage().flatMap { jpegBase64($0, maxDim: 112) }
    return (rgb.base64EncodedString(), jpeg)
}

final class Grabber: NSObject, AVCaptureVideoDataOutputSampleBufferDelegate {
    let sem = DispatchSemaphore(value: 0)
    var image: CGImage?
    var seen = 0
    private var done = false
    // Descartamos los primeros frames: una webcam recién encendida entrega frames de WARM-UP
    // (negros/oscuros, autoexposición sin asentar) donde Vision no detecta ninguna cara.
    private let warmup = 8
    func captureOutput(_ output: AVCaptureOutput, didOutput sampleBuffer: CMSampleBuffer,
                       from connection: AVCaptureConnection) {
        if done { return }
        seen += 1
        if seen < warmup { return }
        guard let pb = CMSampleBufferGetImageBuffer(sampleBuffer) else { return }
        let ci = CIImage(cvPixelBuffer: pb)
        if let cg = CIContext().createCGImage(ci, from: ci.extent) {
            image = cg; done = true; sem.signal()
        }
    }
}

// 0) Permiso de cámara (TCC). SIN `requestAccess` explícito, el primer uso NO muestra el diálogo
//    de macOS de forma fiable y la sesión expira sin frame. Pedimos acceso y esperamos el veredicto;
//    el diálogo se atribuye a AION (NSCameraUsageDescription embebido). Si ya está denegado, macOS no
//    vuelve a preguntar → hay que activarlo a mano en Ajustes.
switch AVCaptureDevice.authorizationStatus(for: .video) {
case .authorized:
    break
case .notDetermined:
    let psem = DispatchSemaphore(value: 0)
    var granted = false
    AVCaptureDevice.requestAccess(for: .video) { ok in granted = ok; psem.signal() }
    _ = psem.wait(timeout: .now() + 45)  // espera a que Ariel acepte el diálogo
    if !granted {
        emit(["faces": [], "error": "permiso de cámara no concedido (acéptalo en el diálogo, o en Ajustes del Sistema → Privacidad y seguridad → Cámara → AION)"])
        exit(0)
    }
case .denied, .restricted:
    emit(["faces": [], "error": "permiso de cámara DENEGADO. Actívalo en Ajustes del Sistema → Privacidad y seguridad → Cámara → AION, y reintenta."])
    exit(0)
@unknown default:
    break
}

// 1) Capturar un frame.
let session = AVCaptureSession()
session.sessionPreset = .high
guard let device = AVCaptureDevice.default(for: .video),
      let input = try? AVCaptureDeviceInput(device: device),
      session.canAddInput(input) else {
    emit(["faces": [], "error": "sin cámara o sin acceso"]); exit(0)
}
let camName = device.localizedName
session.addInput(input)
let out = AVCaptureVideoDataOutput()
out.videoSettings = [kCVPixelBufferPixelFormatTypeKey as String: kCVPixelFormatType_32BGRA]
let grab = Grabber()
out.setSampleBufferDelegate(grab, queue: DispatchQueue(label: "aion.face.cap"))
guard session.canAddOutput(out) else { emit(["faces": [], "error": "sin salida"]); exit(0) }
session.addOutput(out)
session.startRunning()
let got = grab.sem.wait(timeout: .now() + 8)  // margen para el warm-up del sensor
session.stopRunning()
guard got == .success, let cg = grab.image else {
    emit(["faces": [], "error": "no llegó frame de la cámara",
          "diag": ["camera": camName, "frames": grab.seen]]); exit(0)
}

// 2) Detectar rostros + LANDMARKS, ALINEAR cada cara a la plantilla de ArcFace, y generar faceprint.
let W = CGFloat(cg.width), H = CGFloat(cg.height)
let handler = VNImageRequestHandler(cgImage: cg, options: [:])
let faceReq = VNDetectFaceLandmarksRequest()
try? handler.perform([faceReq])

// Puntos canónicos de InsightFace/ArcFace en 112×112 (origen ARRIBA-izq), convertidos a sistema
// ABAJO-izquierda (el nativo de Vision y CGContext): y' = 112 − y. Orden: ojo-izq-imagen,
// ojo-der-imagen, nariz, comisura-izq, comisura-der.
let dstBL: [CGPoint] = [
    CGPoint(x: 38.2946, y: 112 - 51.6963),
    CGPoint(x: 73.5318, y: 112 - 51.5014),
    CGPoint(x: 56.0252, y: 112 - 71.7366),
    CGPoint(x: 41.5493, y: 112 - 92.3655),
    CGPoint(x: 70.7299, y: 112 - 92.2041),
]

var faces: [[String: Any]] = []
var alignedPreview: String? = nil
for obs in (faceReq.results ?? []) {
    let bb = obs.boundingBox  // normalizado, origen abajo-izquierda
    let rx = bb.origin.x * W
    let ry = (1 - bb.origin.y - bb.height) * H
    let rw = bb.width * W
    let rh = bb.height * H
    guard rw > 20, rh > 20,
          let faceCG = cg.cropping(to: CGRect(x: rx, y: ry, width: rw, height: rh).integral)
    else { continue }

    // Featureprint genérico de Vision (fallback si faltara el modelo ArcFace) — se mantiene.
    var emb = [Float]()
    let fpHandler = VNImageRequestHandler(cgImage: faceCG, options: [:])
    let fpReq = VNGenerateImageFeaturePrintRequest()
    try? fpHandler.perform([fpReq])
    if let fp = fpReq.results?.first as? VNFeaturePrintObservation, fp.elementType == .float {
        emb = [Float](repeating: 0, count: fp.elementCount)
        fp.data.withUnsafeBytes { (raw: UnsafeRawBufferPointer) in
            let p = raw.bindMemory(to: Float.self)
            for i in 0..<min(fp.elementCount, p.count) { emb[i] = p[i] }
        }
    }
    var face: [String: Any] = ["embedding": emb, "bbox": [Double(rx), Double(ry), Double(rw), Double(rh)]]

    // Punto normalizado de un landmark (relativo al bbox, origen abajo-izq) → píxel en sistema
    // ABAJO-izquierda de la imagen.
    func toPx(_ nx: CGFloat, _ ny: CGFloat) -> CGPoint {
        CGPoint(x: (bb.origin.x + nx * bb.width) * W, y: (bb.origin.y + ny * bb.height) * H)
    }
    func mean(_ r: VNFaceLandmarkRegion2D?) -> CGPoint? {
        guard let r = r, r.pointCount > 0 else { return nil }
        var sx: CGFloat = 0, sy: CGFloat = 0
        for p in r.normalizedPoints { sx += CGFloat(p.x); sy += CGFloat(p.y) }
        return toPx(sx / CGFloat(r.pointCount), sy / CGFloat(r.pointCount))
    }
    func corners(_ r: VNFaceLandmarkRegion2D?) -> (CGPoint, CGPoint)? {
        guard let r = r, r.pointCount >= 2 else { return nil }
        let pts = r.normalizedPoints.map { toPx(CGFloat($0.x), CGFloat($0.y)) }
        guard let lo = pts.min(by: { $0.x < $1.x }), let hi = pts.max(by: { $0.x < $1.x })
        else { return nil }
        return (lo, hi)
    }

    // crop112 ALINEADO por landmarks (lo que de verdad reconoce, robusto al ángulo).
    var aligned = false
    if let lm = obs.landmarks,
       let eyeA = mean(lm.leftEye), let eyeB = mean(lm.rightEye),
       let nose = mean(lm.nose), let mouth = corners(lm.outerLips ?? lm.innerLips) {
        // Mapeo por POSICIÓN en la imagen (no por nombre anatómico) → evita el lío izq/der.
        let eyeL = eyeA.x <= eyeB.x ? eyeA : eyeB
        let eyeR = eyeA.x <= eyeB.x ? eyeB : eyeA
        let src = [eyeL, eyeR, nose, mouth.0, mouth.1]
        if let t = similarityTransform(src, dstBL), let af = alignedFace(cg, W, H, t) {
            face["crop112"] = af.rgb
            if alignedPreview == nil { alignedPreview = af.jpeg }
            aligned = true
        }
    }
    if !aligned {
        // Cara muy ladeada/parcial sin landmarks fiables: recorte simple, mejor que nada.
        if let c = crop112(faceCG) { face["crop112"] = c }
    }
    face["aligned"] = aligned

    // Foto para MOSTRAR en el chat: recorte de la cara con un margen, en JPEG.
    let mx = rw * 0.35, my = rh * 0.35
    let photoRect = CGRect(x: rx - mx, y: ry - my, width: rw + 2 * mx, height: rh + 2 * my)
        .integral
        .intersection(CGRect(x: 0, y: 0, width: W, height: H))
    let photoCG = cg.cropping(to: photoRect) ?? faceCG
    if let j = jpegBase64(photoCG) { face["face_jpeg"] = j }
    faces.append(face)
}
var diag: [String: Any] = ["camera": camName, "frames": grab.seen,
                           "frame": ["w": cg.width, "h": cg.height]]
if let ap = alignedPreview { diag["aligned_jpeg"] = ap }
emit(["faces": faces, "error": NSNull(), "diag": diag])
exit(0)
