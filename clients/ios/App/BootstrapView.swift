import SwiftUI

struct BootstrapView: View {
    var body: some View {
        VStack(spacing: 12) {
            Text("Synveil")
                .font(.largeTitle)
                .bold()
            Text("iOS v0.1 Bootstrap")
                .font(.subheadline)
                .foregroundColor(.secondary)
        }
        .padding()
    }
}

#Preview {
    BootstrapView()
}
