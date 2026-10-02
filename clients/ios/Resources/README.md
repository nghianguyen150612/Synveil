# Synveil iOS — Resources (`clients/ios/Resources/`)

## Purpose & Ownership
Reserves ownership for iOS application resources, configuration plists, asset catalogs, and localization strings.

### Resource Categories
- **Asset Catalogs**: `Assets.xcassets` (app icons, accent colors, brand imagery).
- **Localizations**: `Localizable.xcstrings` (String catalog supporting multi-language localizations).
- **Configuration & Privacy**: `Info.plist` and privacy usage descriptions (`NSPhotoLibraryUsageDescription`, etc., when needed).

## Rules
- No production assets, secrets, or actual localized string catalog files are created in Prompt006.
- Assets must be organized cleanly without loose unversioned binaries.
