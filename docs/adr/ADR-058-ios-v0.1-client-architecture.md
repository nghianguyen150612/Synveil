# ADR-058: Synveil iOS v0.1 Client Architecture and Platform Ownership / Kiến trúc client Synveil iOS v0.1 và quyền sở hữu nền tảng

## Status / Trạng thái
Accepted / Chấp thuận — Prompt004 contract

## Date / Ngày
2025-02-23

## Decision owners / Chủ sở hữu quyết định
Synveil Core & iOS Architecture Group / Nhóm kiến trúc Synveil Core & iOS

## Context / Bối cảnh
Prompt001 defined the Android v0.1 product parity contract. Prompt002 established the Android-to-iOS platform translation layer. Prompt003 audited the shared Rust workspace crates for iOS reuse. Synveil requires a definitive, production-grade architecture specification for the native iOS client (`v0.1`) that ensures feature parity with Android while respecting native Apple platform security, application lifecycle, sandbox constraints, and concurrency models.

Prompt001 đã xác định hợp đồng tương đương sản phẩm Android v0.1. Prompt002 đã thiết lập lớp dịch chuyển nền tảng Android sang iOS. Prompt003 đã kiểm toán các crate Rust dùng chung cho việc tái sử dụng trên iOS. Synveil cần một bản quy hoạch kiến trúc chính thức, đạt tiêu chuẩn production cho client native iOS (`v0.1`) nhằm đảm bảo tính tương đương tính năng với Android nhưng vẫn tuân thủ các nguyên tắc bảo mật, vòng đời ứng dụng, giới hạn sandbox và mô hình bất đồng bộ native của nền tảng Apple.

## Decision / Quyết định
1. **Native Swift & SwiftUI Architecture / Kiến trúc Swift & SwiftUI Native**: The iOS application is built strictly as a native Swift application using SwiftUI, Swift Structured Concurrency (`async`/`await`, `Actor`, `@MainActor`), and Apple system frameworks (`URLSession`, `Security`/Keychain, `BackgroundTasks`, `NWPathMonitor`). Heavy third-party frameworks (Redux, TCA, VIPER) are rejected.
2. **Apple System API Ownership / Quyền sở hữu API Apple**: Apple APIs remain native to Swift. Generic Rust crates (`synveil-core`, `synveil-client-sync`) MUST NOT import or depend on Apple platform frameworks (`Security`, `UIKit`, `CFNetwork`).
3. **Narrow Shared Rust Bridge / Cầu nối Rust dùng chung thu hẹp**: Shared Rust logic is limited to pure domain models, identifier generation (UUIDv7), token parsing (`sve1_`, `svd1_`), SHA-256 cryptographic hashing, change feed evaluation, signed ACK calculation, rebaseline snapshot manifest tree hash verification, and conflict classification rules. It is consumed via a narrow, type-safe C-FFI / UniFFI bridge without leaking Rust Tokio tasks, raw database handles (`sqlx`), desktop sockets, or process controllers into Swift.
4. **UI Isolation / Cách ly giao diện**: SwiftUI views are pure functions of state. UI views MUST NOT directly invoke raw network transport, raw FFI functions, Keychain APIs, or SQLite database operations.
5. **URLSession Networking / Mạng truyền tải URLSession**: Networking uses Apple `URLSession` (supporting both foreground calls and background `URLSessionConfiguration.background` transfers) rather than forcing Rust `reqwest`/`rustls` onto iOS.
6. **Keychain Secret Storage / Lưu trữ bí mật bằng Keychain**: Authentication credentials (`svd1_` device tokens) reside strictly in Apple Keychain Services (`Security.framework`) with `kSecAttrAccessibleAfterFirstUnlockThisDeviceOnly` protection and in-memory HTTP header composers. Secrets MUST NEVER appear in `UserDefaults`, SQLite database tables, application logs, or SwiftUI view state.
7. **iOS-Native Lifecycle & Transfers / Vòng đời và truyền tải file trên iOS**: Application lifecycle and background sync use Apple `BGAppRefreshTask`. File transfers (downloads and 4 MiB resumable uploads) are managed by an independent application-level Transfer Engine decoupled from SwiftUI view lifetimes.
8. **Deferred Extensions Boundary / Ranh giới extension hoãn lại**: File Provider Extension (`NSFileProviderExtension`, P053–P055) and PhotoKit Auto-Upload (`PHPhotoLibrary`, P056–P058) are defined as isolated extension subsystems sharing App Group storage, keeping the primary in-app file browser architecture unpolluted.
9. **No Desktop Porting / Không port quy trình desktop**: Desktop process launcher, UNIX domain socket IPC, systemd services, and Qt/QML UI shell are explicitly excluded from iOS.

## Consequences / Hệ quả

### Positive / Tích cực
- Guarantees clean layer separation, high testability, and deterministic mocking across all domain services.
- Eliminates memory leakage and secret exposure risks by keeping credentials inside hardware Secure Enclave keychains.
- Ensures file transfers survive view teardowns and background app transitions.
- Prevents cross-platform protocol drift by sharing critical domain algorithms with Android via `synveil-core`.
- Maintains small iOS binary size by compiling only a minimal, platform-decoupled Rust static library.

### Negative / Bất lợi
- Requires writing Swift wrapper implementations for `URLSession` transport, `GRDB` SQLite cache, and `KeychainVault`.
- Requires decoupling `synveil-client-sync` algorithms from `sqlx`/`reqwest` before FFI bridging in Phase C.

## Alternatives / Phương án khác

1. **Porting Desktop Rust Process Architecture / Port kiến trúc tiến trình Desktop bằng Rust**:
   - *Evaluated*: Running the desktop IPC background daemon process on iOS via embedded Tokio.
   - *Rejected*: iOS mobile sandbox forbids multi-process daemon launching (`fork`/`exec`), background UNIX domain sockets, and unconstrained background processes.
2. **Duplicating Domain Logic in Pure Swift / Nhân bản logic miền hoàn toàn bằng Swift**:
   - *Evaluated*: Writing all hashing, UUIDv7, token parsing, and sync rebaseline algorithms in Swift.
   - *Rejected*: Creates severe risk of cross-platform protocol drift between Android and iOS.
3. **Exposing Broad Rust Crates via FFI / Mở rộng FFI trực tiếp cho toàn bộ Rust Crate**:
   - *Evaluated*: Exporting `synveil-client-sync` wholesale across FFI including Tokio tasks and SQLx database handles.
   - *Rejected*: Causes symbol collisions, memory leaks, and Tokio runtime conflicts on mobile targets.
4. **Embedding Apple Frameworks in Rust Crates / Nhúng framework Apple vào Rust Crate**:
   - *Evaluated*: Calling iOS Keychain and CFNetwork directly from inside Rust crates using Objective-C / Swift bindings.
   - *Rejected*: Pollutes generic Rust crates and prevents clean unit testing on non-Apple platforms.
5. **Using Heavy Architecture Frameworks (TCA / VIPER / Redux) / Dùng framework kiến trúc phức tạp**:
   - *Evaluated*: Introducing TCA or VIPER for state management.
   - *Rejected*: Adds unnecessary framework overhead without demonstrated product need; native Swift `@Observable` and structured concurrency provide complete capability.

## Migration and review trigger / Điều kiện di chuyển và xem xét lại
This architecture is locked for Synveil iOS v0.1 development (Prompts P005–P060). Any proposed architectural revision requires a superseding ADR approved by the project architecture group.
