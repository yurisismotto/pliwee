// The incoming-file prompt.
//
// Consent, not a data path: the only thing that leaves this view is a
// boolean and the transfer id it belongs to. The bytes travel on the agent's
// own TLS stream and are written by the agent. Accepting approves one offer;
// there is no "always allow", and the next offer asks again.
//
// Every way out is a decline: Decline is the default and cancel action, and
// closing the panel declines whatever it was showing.

import PliweeKit
import SwiftUI

struct FileOfferView: View {
    let model: AppModel

    var body: some View {
        if let offer = model.offers.first {
            VStack(alignment: .leading, spacing: 14) {
                HStack(spacing: 12) {
                    Image(systemName: "arrow.down.doc")
                        .font(.system(size: 30))
                        .foregroundStyle(Color.pliweeBlue)
                    VStack(alignment: .leading, spacing: 2) {
                        Text("Incoming file").font(.headline)
                        Text("**\(offer.deviceName)** wants to send you a file.")
                    }
                }
                VStack(alignment: .leading, spacing: 4) {
                    Text(offer.filename).font(.body.weight(.semibold)).lineLimit(2).truncationMode(.middle)
                    Text("\(Format.bytes(offer.sizeBytes)) · \(offer.mimeType)")
                        .font(.callout).foregroundStyle(.secondary)
                    Label("Verified device · \(offer.fingerprintShort)", systemImage: "checkmark.shield")
                        .font(.caption.monospaced()).foregroundStyle(.secondary)
                }
                if model.offers.count > 1 {
                    Text("\(model.offers.count - 1) more waiting.").font(.caption).foregroundStyle(.secondary)
                }
                HStack {
                    Spacer()
                    Button("Decline", role: .cancel) { model.decide(offer, accept: false) }
                        .keyboardShortcut(.cancelAction)
                    Button("Accept") { model.decide(offer, accept: true) }
                }
            }
            .padding(22)
            .frame(width: 420)
        } else {
            Text("No incoming files.").padding(22).frame(width: 420)
        }
    }
}
