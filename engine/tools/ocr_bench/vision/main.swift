// Programme du banc d'essai OCR : lit une image avec Vision, par le code de
// l'application (app/Recognition/VisionTextReader.swift), et écrit les mots en
// JSON (ou, avec --tsv, une ligne par mot : texte, confiance, ligne et les huit
// coordonnées du quadrilatère, séparés par des tabulations) sur la sortie
// standard.
//
//   vision_ocr [--languages fr-FR,en-US] [--min-height 0.01] [--tsv] image.png
//   vision_ocr --version
//
// Compilé par `make ocr-bench` :
//   swiftc -O app/Recognition/VisionTextReader.swift engine/tools/ocr_bench/vision/main.swift

import CoreGraphics
import Foundation
import ImageIO

func fail(_ message: String) -> Never {
    FileHandle.standardError.write(Data((message + "\n").utf8))
    exit(1)
}

var languages = ["fr-FR", "en-US"]
var minimumHeight: Float?
var tsv = false
var path: String?
var arguments = CommandLine.arguments.dropFirst().makeIterator()
while let argument = arguments.next() {
    switch argument {
    case "--version":
        print("macOS \(ProcessInfo.processInfo.operatingSystemVersionString)")
        exit(0)
    case "--languages":
        guard let value = arguments.next() else { fail("--languages : valeur attendue") }
        languages = value.split(separator: ",").map(String.init)
    case "--min-height":
        guard let value = arguments.next().flatMap(Float.init) else { fail("--min-height : nombre attendu") }
        minimumHeight = value
    case "--tsv":
        tsv = true
    default:
        path = argument
    }
}
guard let path else { fail("usage : vision_ocr [--languages fr-FR,en-US] [--min-height h] [--tsv] image") }
guard let source = CGImageSourceCreateWithURL(URL(fileURLWithPath: path) as CFURL, nil),
      let image = CGImageSourceCreateImageAtIndex(source, 0, nil) else {
    fail("image illisible : \(path)")
}
do {
    let words = try VisionTextReader.read(image, languages: languages, minimumTextHeight: minimumHeight)
    if tsv {
        // Un mot ne contient aucune espace, donc aucune tabulation.
        let rows = words.map { word in
            ([word.text, "\(word.confidence)", "\(word.line)"] + word.quad.map { "\($0)" }).joined(separator: "\t")
        }
        FileHandle.standardOutput.write(Data(rows.map { $0 + "\n" }.joined().utf8))
    } else {
        FileHandle.standardOutput.write(try JSONEncoder().encode(words))
    }
} catch {
    fail("Vision : \(error.localizedDescription)")
}
