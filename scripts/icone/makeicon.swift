import AppKit

// Ícone do Skiá (1024 px), no estilo da família (Antígraphon, Katágo, Phonika): figura negra sobre terracota.
// Uma folha de autos em negro com as linhas "incisas" na cor do fundo; em duas linhas, o nome some numa sombra
// (σκιά): entre « e », a linha não é incisa — o nome sai, a forma fica.
let out = CommandLine.arguments.count > 1 ? CommandLine.arguments[1] : "icone.png"
let S: CGFloat = 1024
let inset: CGFloat = 90
let box = NSRect(x: inset, y: inset, width: S - 2 * inset, height: S - 2 * inset)
let terra = NSColor(red: 0xD8/255, green: 0x50/255, blue: 0x10/255, alpha: 1)
let preto = NSColor.black

let rep = NSBitmapImageRep(bitmapDataPlanes: nil, pixelsWide: Int(S), pixelsHigh: Int(S), bitsPerSample: 8,
                           samplesPerPixel: 4, hasAlpha: true, isPlanar: false, colorSpaceName: .deviceRGB,
                           bytesPerRow: 0, bitsPerPixel: 0)!
NSGraphicsContext.saveGraphicsState()
NSGraphicsContext.current = NSGraphicsContext(bitmapImageRep: rep)!

terra.setFill()
NSBezierPath(roundedRect: box, xRadius: 190, yRadius: 190).fill()
preto.setStroke()
let filete = NSBezierPath(roundedRect: box.insetBy(dx: 44, dy: 44), xRadius: 150, yRadius: 150)
filete.lineWidth = 14
filete.stroke()

// a folha, com a ponta dobrada
preto.setFill()
let folha = NSBezierPath()
folha.move(to: NSPoint(x: 300, y: 250))
folha.line(to: NSPoint(x: 724, y: 250))
folha.line(to: NSPoint(x: 724, y: 670))
folha.line(to: NSPoint(x: 620, y: 774))
folha.line(to: NSPoint(x: 300, y: 774))
folha.close()
folha.fill()
terra.setFill()
let dobra = NSBezierPath()
dobra.move(to: NSPoint(x: 620, y: 774))
dobra.line(to: NSPoint(x: 620, y: 670))
dobra.line(to: NSPoint(x: 724, y: 670))
dobra.close()
dobra.fill()

// linhas incisas; nas linhas 2 e 4, o trecho do nome fica negro entre « e »
func linha(_ x: CGFloat, _ y: CGFloat, _ w: CGFloat) {
    NSBezierPath(roundedRect: NSRect(x: x, y: y, width: w, height: 18), xRadius: 9, yRadius: 9).fill()
}
func aspas(_ x: CGFloat, _ y: CGFloat, abre: Bool) {
    let p = NSBezierPath()
    p.lineWidth = 12
    p.lineCapStyle = .round
    p.lineJoinStyle = .round
    for d in [CGFloat(0), 22] {
        let x0 = abre ? x + d + 16 : x + d
        let x1 = abre ? x + d : x + d + 16
        p.move(to: NSPoint(x: x0, y: y + 22))
        p.line(to: NSPoint(x: x1, y: y + 9))
        p.line(to: NSPoint(x: x0, y: y - 4))
    }
    terra.setStroke()
    p.stroke()
}
terra.setFill()
let ys: [CGFloat] = [600, 528, 456, 384, 312]
linha(345, ys[0], 230)
linha(345, ys[1], 60); aspas(420, ys[1], abre: true); aspas(590, ys[1], abre: false); linha(645, ys[1] , 34)
linha(345, ys[2], 334)
linha(345, ys[3], 150); aspas(510, ys[3], abre: true); aspas(630, ys[3], abre: false)
linha(345, ys[4], 210)

NSGraphicsContext.restoreGraphicsState()
try! rep.representation(using: .png, properties: [:])!.write(to: URL(fileURLWithPath: out))
