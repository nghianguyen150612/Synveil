# Prompt019 evidence manifest — Linux first launch

Status: **source gates defined; final-head hosted execution required**.

| Evidence | Implementation / proof |
| --- | --- |
| FIRST-LAUNCH-1–3, 28–29 | Desktop startup asynchronously reuses `BackgroundClientManager::ensure_running`; startup registration is a separate worker operation and GUI shutdown remains controller-only. |
| FIRST-LAUNCH-4–8, 16–19, 27 | Translatable first-launch UI, explicit Continue, versioned non-secret QSettings choice, post-mutation authoritative verification, and the shared Settings operation. |
| FIRST-LAUNCH-9–15 | Existing current-user native unit/backend and non-starting DEB/RPM/quick-install contracts remain unchanged; unavailable user systemd has no alternative registration. |
| FIRST-LAUNCH-20–26 | Portable no-mutation Off path and explicit On composition of P016 validation/integration; unhealthy integration is rejected and the stable outer AppImage remains authoritative. |
| FIRST-LAUNCH-30 | P015–P018 validators plus focused Prompt019 validator are final-head gates. |

Focused static validation is `./scripts/validate-linux-first-launch.sh`. Native
Ubuntu 24.04, Fedora 42, and real AppImage lifecycle results must be taken from
the final PR head; this manifest does not misclassify local/static evidence as
hosted evidence. P020 remains pending.
