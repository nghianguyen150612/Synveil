# ADR-012: Integrate Forgejo; do not reimplement Git / Tích hợp Forgejo, không viết lại Git

- Status / Trạng thái: **Accepted / Chấp thuận**
- Date / Ngày: 2026-08-21
- Owners / Chủ sở hữu: Integrations, Backup, Security

## Context (English)

Git smart HTTP/SSH, refs, packfiles, LFS, permissions, issues, and pull requests
are mature forge responsibilities. Rebuilding them would distract from storage,
sync, and restore correctness.

## Decision (English)

Forgejo is the first connector target. Synveil may list repository metadata,
associate repositories with `Project`, monitor health, and back up/restore
repositories, Git LFS objects, and selected release artifacts through documented
Forgejo/Git interfaces. Forgejo remains authoritative for Git protocols and
forge collaboration. Connectors use least-privilege credentials, encrypted
secret storage, bounded SSRF-safe endpoints, asynchronous polling/webhooks, and
staleness/error labels. GitHub, GitLab, and Gitea are future adapters.

## Consequences (English)

Forgejo outage never blocks core files. Repository backup must capture a
restorable, self-consistent form and verify a restore drill; metadata inventory
alone is not backup. Webhook input is authenticated, replay-limited, and treated
as a hint that schedules reconciliation, not trusted truth. Synveil does not
expose custom Git smart HTTP or SSH.

## Bối cảnh (Tiếng Việt)

Git smart HTTP/SSH, ref, packfile, LFS, permission, issue và pull request là
trách nhiệm đã trưởng thành của forge. Viết lại sẽ làm lệch ưu tiên storage,
sync và restore.

## Quyết định (Tiếng Việt)

Forgejo là connector đầu tiên. Synveil có thể liệt kê metadata repository, gắn
với `Project`, theo dõi health và backup/restore repository, Git LFS, release
artifact chọn lọc qua giao diện Forgejo/Git đã công bố. Forgejo vẫn là nguồn sự
thật cho giao thức Git và cộng tác. Connector dùng credential tối thiểu, lưu
secret được mã hóa, endpoint chống SSRF có giới hạn, polling/webhook bất đồng bộ
và nhãn stale/error. GitHub, GitLab, Gitea là adapter tương lai.

## Hệ quả (Tiếng Việt)

Forgejo lỗi không chặn Files. Backup repository phải thu dạng tự nhất quán có
thể restore và qua diễn tập restore; inventory metadata không phải backup.
Webhook phải xác thực, hạn chế replay và chỉ là tín hiệu lên lịch đối soát, không
phải sự thật đáng tin. Synveil không mở Git smart HTTP/SSH tự viết.

Alternatives rejected / Phương án loại bỏ: build a custom forge; synchronous
Forgejo dependency in file operations; treating API metadata as repository
backup.
