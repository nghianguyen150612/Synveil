# ADR-022: Progressive disclosure and complexity boundary / Progressive disclosure và ranh giới phức tạp

- Status / Trạng thái: **Accepted / Chấp thuận**
- Date / Ngày: 2026-08-22
- Owners / Chủ sở hữu: Product, UX / Accessibility, Architecture, Security

## Context (English)

Synveil contains unavoidable infrastructure concepts, but exposing them in the
primary journey makes a private cloud inaccessible to ordinary users. Hiding
them entirely would remove useful control from self-hosters and administrators.

## Decision (English)

Use progressive disclosure. Normal flows use Files, Photos, Backups, Devices,
Shared, Search, Activity, Storage, and Health. `Settings → Advanced` and
administrator diagnostics may expose PostgreSQL, object backends, sync cursors,
GC, compression, S3, filesystem capabilities, service logs, and maintenance.
Every advanced control retains the same authorization, inspect/plan/validate/
execute/verify, audit, and recovery contract. Stable machine-readable errors
remain available below user-friendly messages.

The core user-facing rule is: an ordinary user must not need a terminal for a
core feature. Accessibility, understandable health states, destructive
confirmation, and recovery guidance are release concerns, not cosmetic work.

## Hệ quả (Tiếng Việt)

Synveil có các khái niệm infrastructure không thể tránh, nhưng đưa chúng vào
journey chính làm private cloud khó tiếp cận với user thường. Ẩn hoàn toàn lại
lấy mất quyền kiểm soát cần thiết của self-hoster/admin.

## Quyết định (Tiếng Việt)

Dùng progressive disclosure. Luồng thường dùng Files, Photos, Backups,
Devices, Shared, Search, Activity, Storage và Health. `Settings → Advanced` và
admin diagnostics có thể hiện PostgreSQL, object backend, sync cursor, GC,
compression, S3, filesystem capability, service log và maintenance. Mọi control
nâng cao vẫn giữ contract authorization, inspect/plan/validate/execute/verify,
audit và recovery. Stable machine-readable error vẫn nằm dưới user message dễ
hiểu.

Quy tắc core là: user bình thường không cần terminal cho core feature.
Accessibility, health dễ hiểu, destructive confirmation và recovery guidance
là concern của release, không chỉ là cosmetic.

## Alternatives rejected / Phương án loại bỏ

Infrastructure-first UI for every user; removing all advanced controls; replacing
stable error contracts with prose-only messages; hiding destructive effects.

UI infrastructure-first cho mọi user; xóa toàn bộ control nâng cao; thay stable
error contract bằng prose-only message; che giấu hậu quả destructive.

## Migration and review trigger / Điều kiện di chuyển và xem xét lại

Any new user-facing flow reviews its terminology, disclosure level,
accessibility, human remediation, advanced escape hatch, and bilingual copy.
Promoting a core feature requires a no-terminal primary path or a documented
exception approved by Product, Security, and UX / Accessibility.

Mọi flow user-facing mới phải review terminology, mức disclosure, accessibility,
remediation cho người dùng, advanced escape hatch và copy song ngữ. Promote core
feature cần primary path không terminal hoặc ngoại lệ có Product, Security và UX
/ Accessibility chấp thuận.
