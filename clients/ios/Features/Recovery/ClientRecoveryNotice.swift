import SwiftUI

struct ClientRecoveryNotice: View {
    let presentation: ClientRecoveryPresentation

    var body: some View {
        VStack(alignment: .leading, spacing: 6) {
            Label(
                presentation.classification.title, systemImage: presentation.classification.symbol
            )
            .font(.headline)
            Text(presentation.message).font(.footnote)
        }
        .fixedSize(horizontal: false, vertical: true)
        .accessibilityElement(children: .combine)
        .accessibilityIdentifier("synveil.recovery.\(presentation.classification.rawValue)")
    }
}
