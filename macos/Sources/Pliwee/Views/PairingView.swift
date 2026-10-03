// Pairing: the code to scan, then the fingerprint to compare.
//
// The agent does all of it — the one-time token, the TLS handshake, the
// proof that the phone holds the token, the pinning. This sheet shows the QR
// code the agent produced and asks the one question only a person can
// answer: does the fingerprint on this screen match the one on the phone?

import CoreImage
import CoreImage.CIFilterBuiltins
import PliweeKit
import SwiftUI

struct PairingView: View {
    let session: PairingSession
    let close: () -> Void

    var body: some View {
        VStack(spacing: 18) {
            Text("Pair a Device").font(.title2.weight(.semibold))
            content
        }
        .padding(28)
        .frame(width: 440)
    }

    @ViewBuilder
    private var content: some View {
        switch session.phase {
        case .starting:
            ProgressView("Opening a pairing window…")
            Button("Cancel", role: .cancel, action: close).keyboardShortcut(.cancelAction)

        case let .showingCode(payload, expiresAt):
            Text("In the Pliwee app on your phone, tap **Pair device** and scan this code.")
                .multilineTextAlignment(.center)
                .fixedSize(horizontal: false, vertical: true)
            if let image = QRCode.image(for: payload) {
                Image(nsImage: image)
                    .interpolation(.none)
                    .resizable()
                    .frame(width: 240, height: 240)
                    .padding(10)
                    .background(.white, in: RoundedRectangle(cornerRadius: 8))
                    .accessibilityLabel("Pairing code")
            }
            TimelineView(.periodic(from: .now, by: 1)) { context in
                let left = max(0, Int(expiresAt.timeIntervalSince(context.date)))
                Text("The code expires in \(left / 60):\(String(format: "%02d", left % 60)).")
                    .font(.callout).foregroundStyle(.secondary).monospacedDigit()
            }
            Button("Cancel", role: .cancel, action: close).keyboardShortcut(.cancelAction)

        case let .confirming(name, fingerprint, short):
            Image(systemName: "checkmark.shield").font(.system(size: 40)).foregroundStyle(Color.pliweeBlue)
            Text("**\(name)** wants to pair with this Mac.")
                .multilineTextAlignment(.center)
                .fixedSize(horizontal: false, vertical: true)
            Text("Check that this fingerprint matches the one shown on the device.")
                .multilineTextAlignment(.center).foregroundStyle(.secondary)
                .fixedSize(horizontal: false, vertical: true)
            Text(short)
                .font(.title3.monospaced().weight(.semibold))
                .textSelection(.enabled)
            Text(Format.fingerprint(fingerprint))
                .font(.caption.monospaced()).foregroundStyle(.secondary)
                .multilineTextAlignment(.center)
                .textSelection(.enabled)
            HStack {
                // Decline is the default *and* the cancel action: Escape,
                // Return and closing the sheet all decline.
                Button("Decline", role: .cancel) { session.answer(accept: false) }
                    .keyboardShortcut(.cancelAction)
                Button("They Match — Pair") { session.answer(accept: true) }
            }

        case let .finished(message, paired):
            Image(systemName: paired ? "checkmark.circle.fill" : "xmark.circle")
                .font(.system(size: 40))
                .foregroundStyle(paired ? Color.token(DesignTokens.Status.success) : .secondary)
            Text(message).multilineTextAlignment(.center)
            if paired {
                Text("The device can do nothing yet. Allow what it may do on the Devices page.")
                    .font(.callout).foregroundStyle(.secondary).multilineTextAlignment(.center)
            }
            Button("Done", action: close).keyboardShortcut(.defaultAction)

        case let .failed(message):
            Image(systemName: "exclamationmark.triangle").font(.system(size: 36))
                .foregroundStyle(Color.token(DesignTokens.Status.warning))
            Text(message).multilineTextAlignment(.center)
            Button("Close", action: close).keyboardShortcut(.defaultAction)
        }
    }
}

enum QRCode {
    /// Renders the agent's payload. The payload is the agent's; this only
    /// draws it, at medium error correction, the level phone cameras read
    /// reliably from a screen.
    static func image(for payload: String) -> NSImage? {
        let filter = CIFilter.qrCodeGenerator()
        filter.message = Data(payload.utf8)
        filter.correctionLevel = "M"
        guard let output = filter.outputImage else { return nil }
        let scaled = output.transformed(by: CGAffineTransform(scaleX: 10, y: 10))
        let rep = NSCIImageRep(ciImage: scaled)
        let image = NSImage(size: rep.size)
        image.addRepresentation(rep)
        return image
    }
}
